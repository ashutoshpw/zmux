//! Trusted user-file I/O for the file panel. Everything here may block and therefore runs
//! only on the bounded `zmux-files` worker, never on the workspace executor or a UI thread.
//!
//! User files are not private state: the owner/`0600` rules of `persistence.rs` deliberately
//! do not apply. Safety comes from component-wise `openat` walks that refuse symlinks,
//! `O_NONBLOCK` opens plus a regular-file check, hard size caps and atomic same-directory saves.
use rustix::{
    fs::{self, Access, AtFlags, FileType, Mode, OFlags},
    io::Errno,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fmt,
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;
use zmux_workspace_core::FileMode;

/// Largest text or Markdown file that is opened, validated or saved.
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Largest image or PDF that is opened.
pub const MAX_PREVIEW_BYTES: usize = 7_340_032;
/// Fast pre-check window; the complete file is still validated before a tab exists.
pub const SNIFF_BYTES: usize = 8192;
/// Header window read to check an image or PDF signature (the PDF marker may sit up to 1024
/// bytes in).
pub const SIGNATURE_BYTES: usize = 1024;
pub const MAX_PATH_BYTES: usize = 4096;
/// Permission bits for a file that did not exist when it was saved with `force`.
pub const NEW_FILE_MODE: u32 = 0o644;
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// A stable machine code plus a human message. Messages never contain file content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError {
    pub code: &'static str,
    pub message: String,
}
impl FileError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for FileError {}

const PATH_CHANGED: &str =
    "path no longer resolves to the stored location; reopen it with `zmux open`";

fn from_errno(context: &str, errno: Errno) -> FileError {
    match errno {
        Errno::NOENT => {
            FileError::new("not_found", format!("{context}: no such file or directory"))
        }
        Errno::ACCESS | Errno::PERM => {
            FileError::new("permission_denied", format!("{context}: permission denied"))
        }
        Errno::LOOP | Errno::NOTDIR => FileError::new("path_changed", PATH_CHANGED),
        Errno::ISDIR => FileError::new("is_directory", directory_message()),
        Errno::NXIO => FileError::new("not_regular_file", "not a regular file"),
        Errno::NAMETOOLONG => {
            FileError::new("invalid_operand", format!("{context}: name too long"))
        }
        other => FileError::new(
            "io_error",
            format!(
                "{context}: {}",
                io::Error::from_raw_os_error(other.raw_os_error())
            ),
        ),
    }
}
fn from_io(context: &str, error: &io::Error) -> FileError {
    match error.raw_os_error() {
        Some(raw) => from_errno(context, Errno::from_raw_os_error(raw)),
        None => match error.kind() {
            io::ErrorKind::NotFound => FileError::new("not_found", format!("{context}: not found")),
            io::ErrorKind::PermissionDenied => {
                FileError::new("permission_denied", format!("{context}: permission denied"))
            }
            _ => FileError::new("io_error", format!("{context}: {error}")),
        },
    }
}
fn directory_message() -> &'static str {
    "directories cannot be opened; use `zmux new-workspace` and cd, or open a file"
}

/// Cheap change detector taken from `fstat`; equal fingerprints mean "no new content check".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub fingerprint: Fingerprint,
    pub mode: u32,
    pub nlink: u64,
    pub regular: bool,
    pub directory: bool,
}
fn nanos(seconds: i64, nanoseconds: i64) -> i64 {
    i64::try_from(i128::from(seconds) * 1_000_000_000 + i128::from(nanoseconds)).unwrap_or(i64::MAX)
}
pub fn meta_of(file: &File) -> Result<Meta, FileError> {
    let metadata = file
        .metadata()
        .map_err(|error| from_io("inspect file", &error))?;
    Ok(Meta {
        fingerprint: Fingerprint {
            dev: metadata.dev(),
            ino: metadata.ino(),
            size: metadata.len(),
            mtime_ns: nanos(metadata.mtime(), metadata.mtime_nsec()),
            ctime_ns: nanos(metadata.ctime(), metadata.ctime_nsec()),
        },
        mode: metadata.mode(),
        nlink: metadata.nlink(),
        regular: metadata.file_type().is_file(),
        directory: metadata.file_type().is_dir(),
    })
}

/// The narrow set of OS operations the file worker needs. Production uses [`RealFs`]; tests
/// wrap it to gate, count or fail individual calls deterministically.
pub trait FileBackend: Send + Sync {
    /// Resolve symlinks once, at open time. Later operations never follow links.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FileError>;
    /// Walk an absolute path from `/`, opening every component with `O_NOFOLLOW`.
    fn open_dir(&self, path: &Path) -> Result<File, FileError>;
    /// `O_NOFOLLOW | O_NONBLOCK` open of a leaf, so a FIFO cannot hang the worker.
    fn open_leaf(&self, dir: &File, name: &OsStr) -> Result<File, FileError>;
    /// Advisory write permission hint for the leaf and its directory.
    fn writable(&self, dir: &File, name: &OsStr) -> bool;
    /// Read at most `max_bytes` from the current position.
    fn read(&self, file: &mut File, max_bytes: usize) -> Result<Vec<u8>, FileError>;
    /// Exclusive private temporary file in `dir`.
    fn create_temp(&self, dir: &File, name: &str) -> Result<File, FileError>;
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError>;
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError>;
    fn sync_file(&self, file: &File) -> Result<(), FileError>;
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError>;
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError>;
    fn sync_dir(&self, dir: &File) -> Result<(), FileError>;
}

/// Flags of the leaf open. `NOCTTY` keeps a node that turns into a terminal between the
/// inspection and the open (or a platform that disagrees about its type) from becoming the
/// app's controlling terminal; `NONBLOCK` keeps a FIFO from hanging the worker.
const LEAF_OPEN_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOCTTY)
    .union(OFlags::CLOEXEC);

/// Flags that open a directory for traversal only, requiring search (`x`) permission on the
/// directory and no read (`r`) permission. Rationale: home and shared directories are commonly
/// mode `0711` or `0311`; a file inside is still readable and writable by its owner, `realpath`
/// and plain `open(2)` reach it, so refusing the walk with `EACCES` would reject files the user
/// can use. `O_NOFOLLOW | O_DIRECTORY` keep the no-symlink guarantee of the ordinary walk.
/// Linux: `O_PATH` (the descriptor can be used as an `openat`/`renameat`/`unlinkat` base but not
/// read or fsynced). macOS: `O_SEARCH` (`O_EXEC | O_DIRECTORY`, constant from `<sys/fcntl.h>`;
/// this branch could not be compiled or run on the Linux development machine).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn search_only_flags() -> Option<OFlags> {
    Some(OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC)
}
#[cfg(target_os = "macos")]
fn search_only_flags() -> Option<OFlags> {
    const O_EXEC: u32 = 0x4000_0000;
    Some(OFlags::from_bits_retain(O_EXEC) | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC)
}
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn search_only_flags() -> Option<OFlags> {
    None
}

/// Open one directory component relative to `parent` without following symlinks. A directory
/// that cannot be read but can be searched is opened traversal-only (see [`search_only_flags`]).
fn open_directory_at(parent: &File, name: &OsStr) -> Result<File, FileError> {
    let readable = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match fs::openat(parent, name, readable, Mode::empty()) {
        Ok(fd) => Ok(File::from(fd)),
        Err(Errno::ACCESS) => match search_only_flags() {
            // The traversal-only open reports the real obstacle (no search permission).
            Some(flags) => fs::openat(parent, name, flags, Mode::empty())
                .map(File::from)
                .map_err(|errno| from_errno("open directory", errno)),
            None => Err(from_errno("open directory", Errno::ACCESS)),
        },
        Err(errno) => Err(from_errno("open directory", errno)),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RealFs;
impl FileBackend for RealFs {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FileError> {
        std::fs::canonicalize(path).map_err(|error| from_io("resolve path", &error))
    }
    fn open_dir(&self, path: &Path) -> Result<File, FileError> {
        let mut directory = File::from(
            fs::open(
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|errno| from_errno("open filesystem root", errno))?,
        );
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => directory = open_directory_at(&directory, name)?,
                _ => {
                    return Err(FileError::new(
                        "invalid_operand",
                        "path must be absolute without `.` or `..` components",
                    ));
                }
            }
        }
        Ok(directory)
    }
    fn open_leaf(&self, dir: &File, name: &OsStr) -> Result<File, FileError> {
        // Inspect the node before opening it: opening a terminal, FIFO, socket or device must
        // never happen (design D13 step 1). A symlink is left to `O_NOFOLLOW` to refuse.
        let stat = fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|errno| from_errno("inspect file", errno))?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile | FileType::Directory | FileType::Symlink => {}
            _ => {
                return Err(FileError::new(
                    "not_regular_file",
                    "only regular files can be opened (not FIFOs, sockets or devices)",
                ));
            }
        }
        fs::openat(dir, name, LEAF_OPEN_FLAGS, Mode::empty())
            .map(File::from)
            .map_err(|errno| from_errno("open file", errno))
    }
    fn writable(&self, dir: &File, name: &OsStr) -> bool {
        fs::accessat(dir, name, Access::WRITE_OK, AtFlags::empty()).is_ok()
            && fs::accessat(
                dir,
                ".",
                Access::WRITE_OK | Access::EXEC_OK,
                AtFlags::empty(),
            )
            .is_ok()
    }
    fn read(&self, file: &mut File, max_bytes: usize) -> Result<Vec<u8>, FileError> {
        let mut bytes = Vec::new();
        file.take(max_bytes as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| from_io("read file", &error))?;
        Ok(bytes)
    }
    fn create_temp(&self, dir: &File, name: &str) -> Result<File, FileError> {
        fs::openat(
            dir,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map(File::from)
        .map_err(|errno| match errno {
            Errno::ACCESS | Errno::PERM | Errno::ROFS => FileError::new(
                "directory_not_writable",
                "directory is not writable; nothing was written",
            ),
            other => from_errno("create temporary file", other),
        })
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError> {
        file.write_all(bytes)
            .map_err(|error| from_io("write temporary file", &error))
    }
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError> {
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|error| from_io("set file permissions", &error))
    }
    fn sync_file(&self, file: &File) -> Result<(), FileError> {
        file.sync_all()
            .map_err(|error| from_io("sync temporary file", &error))
    }
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError> {
        fs::renameat(dir, from, dir, to).map_err(|errno| from_errno("replace file", errno))
    }
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError> {
        fs::unlinkat(dir, name, AtFlags::empty())
            .map_err(|errno| from_errno("remove temporary file", errno))
    }
    fn sync_dir(&self, dir: &File) -> Result<(), FileError> {
        match dir.sync_all() {
            Ok(()) => Ok(()),
            // A traversal-only descriptor (search-only directory, see `search_only_flags`)
            // cannot be fsynced. Try a readable reopen; if the directory really cannot be read
            // the entry cannot be flushed by any POSIX call, so the (already atomic) rename
            // stands without a directory fsync. This is the only case treated as success.
            Err(error) if error.raw_os_error() == Some(Errno::BADF.raw_os_error()) => {
                match fs::openat(
                    dir,
                    ".",
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(fd) => File::from(fd)
                        .sync_all()
                        .map_err(|error| from_io("sync directory", &error)),
                    Err(Errno::ACCESS) => Ok(()),
                    Err(errno) => Err(from_errno("reopen directory to sync it", errno)),
                }
            }
            Err(error) => Err(from_io("sync directory", &error)),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Extension table of design D4. A basename without a dot, or with only a leading dot, has none.
pub fn classify(path: &Path, force: Option<FileMode>) -> Result<FileMode, FileError> {
    let natural = natural_mode(path);
    match force {
        None => Ok(natural),
        Some(forced) if forced.is_editable() => {
            if natural.is_editable() {
                Ok(forced)
            } else {
                Err(FileError::new(
                    "content_mismatch",
                    "this command accepts text files only; images and PDFs open with `zmux open`",
                ))
            }
        }
        Some(_) => Err(FileError::new("invalid_operand", "unsupported forced mode")),
    }
}
fn extension(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() {
        return None;
    }
    Some(extension.to_ascii_lowercase())
}
fn natural_mode(path: &Path) -> FileMode {
    match extension(path).as_deref() {
        Some("md" | "markdown" | "mdown" | "mkd" | "mkdn") => FileMode::Markdown,
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif"
            | "ico",
        ) => FileMode::Image,
        Some("pdf") => FileMode::Pdf,
        _ => FileMode::Text,
    }
}
fn signature_check(path: &Path, mode: FileMode, head: &[u8]) -> Result<(), FileError> {
    let mismatch = || {
        Err(FileError::new(
            "content_mismatch",
            "file content does not match its extension",
        ))
    };
    match mode {
        FileMode::Pdf => {
            let window = &head[..head.len().min(1024)];
            if window.windows(5).any(|w| w == b"%PDF-") {
                Ok(())
            } else {
                mismatch()
            }
        }
        FileMode::Image => match extension(path).as_deref() {
            Some("png") if !head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) => {
                mismatch()
            }
            Some("jpg" | "jpeg") if !head.starts_with(&[0xFF, 0xD8, 0xFF]) => mismatch(),
            Some("gif") if !(head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a")) => {
                mismatch()
            }
            _ => Ok(()),
        },
        FileMode::Text | FileMode::Markdown => Ok(()),
    }
}

/// Fast pre-check on the first window. A multibyte sequence cut by the window is not an error.
pub fn sniff_text(window: &[u8]) -> Result<(), FileError> {
    if window.contains(&0) {
        return Err(binary_error());
    }
    match std::str::from_utf8(window) {
        Err(error) if error.error_len().is_some() => Err(utf8_error(error.valid_up_to())),
        _ => Ok(()),
    }
}
/// Complete validation: any NUL anywhere is binary, any invalid UTF-8 anywhere is rejected.
pub fn validate_text(bytes: &[u8]) -> Result<(), FileError> {
    if bytes.contains(&0) {
        return Err(binary_error());
    }
    std::str::from_utf8(bytes)
        .map(|_| ())
        .map_err(|error| utf8_error(error.valid_up_to()))
}
fn binary_error() -> FileError {
    FileError::new(
        "binary_file",
        "file contains NUL bytes and is not text; media preview is not implemented",
    )
}
fn utf8_error(offset: usize) -> FileError {
    FileError::new(
        "invalid_utf8",
        format!("file is not valid UTF-8 (first invalid byte at offset {offset})"),
    )
}

/// An absolute path split into its anchored parent and leaf name.
pub fn split(path: &Path) -> Result<(&Path, &OsStr), FileError> {
    let invalid = || {
        FileError::new(
            "invalid_operand",
            "path must be absolute without `.` or `..` components",
        )
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid());
    }
    let name = path.file_name().ok_or_else(invalid)?;
    Ok((path.parent().ok_or_else(invalid)?, name))
}

/// A fully read and validated file.
#[derive(Clone, Debug)]
pub struct LoadedFile {
    /// Exact on-disk bytes, including any UTF-8 BOM.
    pub bytes: Vec<u8>,
    pub meta: Meta,
    pub sha256: String,
    pub bom: bool,
    pub writable: bool,
}
impl LoadedFile {
    /// The text/preview body handed to the view: the BOM, if any, is carried as a flag.
    pub fn body(&self) -> &[u8] {
        if self.bom {
            &self.bytes[UTF8_BOM.len()..]
        } else {
            &self.bytes
        }
    }
}

fn limit_for(mode: FileMode) -> usize {
    if mode.is_editable() {
        MAX_TEXT_BYTES
    } else {
        MAX_PREVIEW_BYTES
    }
}
fn too_large(limit: usize) -> FileError {
    FileError::new(
        "too_large",
        format!("file is larger than the {limit} byte limit"),
    )
}

/// Open `path` (absolute, canonical) and require a regular file within the size limit of
/// `mode`, judged by `fstat` of the opened descriptor. Nothing is read.
fn open_regular<'a>(
    backend: &dyn FileBackend,
    path: &'a Path,
    mode: FileMode,
) -> Result<(File, &'a OsStr, File, Meta), FileError> {
    let (parent, name) = split(path)?;
    let directory = backend.open_dir(parent)?;
    let file = backend.open_leaf(&directory, name)?;
    let meta = meta_of(&file)?;
    if meta.directory {
        return Err(FileError::new("is_directory", directory_message()));
    }
    if !meta.regular {
        return Err(FileError::new(
            "not_regular_file",
            "only regular files can be opened (not FIFOs, sockets or devices)",
        ));
    }
    let limit = limit_for(mode);
    if meta.fingerprint.size > limit as u64 {
        return Err(too_large(limit));
    }
    Ok((directory, name, file, meta))
}

/// Size-check, read completely and validate `path` (an absolute, canonical path).
pub fn load(
    backend: &dyn FileBackend,
    path: &Path,
    mode: FileMode,
) -> Result<LoadedFile, FileError> {
    let (directory, name, mut file, meta) = open_regular(backend, path, mode)?;
    let limit = limit_for(mode);
    let mut bytes = backend.read(&mut file, SNIFF_BYTES)?;
    if mode.is_editable() {
        sniff_text(&bytes)?;
    } else {
        signature_check(path, mode, &bytes)?;
    }
    if bytes.len() == SNIFF_BYTES {
        let rest = backend.read(&mut file, limit + 1 - bytes.len())?;
        bytes.extend_from_slice(&rest);
    }
    if bytes.len() > limit {
        return Err(too_large(limit));
    }
    if mode.is_editable() {
        validate_text(&bytes)?;
    }
    let bom = mode.is_editable() && bytes.starts_with(&UTF8_BOM);
    Ok(LoadedFile {
        sha256: sha256_hex(&bytes),
        writable: mode.is_editable() && backend.writable(&directory, name),
        bytes,
        meta,
        bom,
    })
}

/// The result of probing one `open` operand.
#[derive(Clone, Debug)]
pub struct Probe {
    /// Canonical absolute path that the model will store.
    pub path: String,
    pub mode: FileMode,
    pub size: u64,
    /// Content hash of a fully validated text or Markdown file. `None` for images and PDFs,
    /// which are judged by `fstat` size and signature alone and never read in full (design D4).
    pub sha256: Option<String>,
}
/// Lexical operand validation plus canonicalisation and content validation: the whole file for
/// text and Markdown, `fstat` size plus signature window for images and PDFs.
pub fn probe(
    backend: &dyn FileBackend,
    operand: &str,
    force: Option<FileMode>,
) -> Result<Probe, FileError> {
    if operand.is_empty()
        || operand.len() > MAX_PATH_BYTES
        || operand.chars().any(char::is_control)
        || !operand.starts_with('/')
    {
        return Err(FileError::new(
            "invalid_operand",
            "path must be absolute, at most 4096 bytes and free of control characters",
        ));
    }
    let canonical = backend.canonicalize(Path::new(operand))?;
    let text = canonical
        .to_str()
        .ok_or_else(|| FileError::new("path_not_utf8", "path is not valid UTF-8"))?
        .to_owned();
    if text.len() > MAX_PATH_BYTES {
        return Err(FileError::new(
            "invalid_operand",
            "canonical path is longer than 4096 bytes",
        ));
    }
    let mode = classify(&canonical, force)?;
    if mode.is_editable() {
        let loaded = load(backend, &canonical, mode)?;
        return Ok(Probe {
            path: text,
            mode,
            size: loaded.meta.fingerprint.size,
            sha256: Some(loaded.sha256),
        });
    }
    // Images and PDFs can be 7 MiB: the probe runs under a 1 s budget and the attach reads the
    // file anyway, so only the size (already checked against the cap from `fstat`) and the
    // signature window are examined here.
    let (_directory, _name, mut file, meta) = open_regular(backend, &canonical, mode)?;
    let head = backend.read(&mut file, SIGNATURE_BYTES)?;
    signature_check(&canonical, mode, &head)?;
    Ok(Probe {
        path: text,
        mode,
        size: meta.fingerprint.size,
        sha256: None,
    })
}

/// What the poller sees without reading content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation {
    Missing,
    Present(Meta),
    Unreadable(FileError),
}
pub fn observe(backend: &dyn FileBackend, path: &Path) -> Observation {
    let opened = split(path).and_then(|(parent, name)| {
        let directory = backend.open_dir(parent)?;
        let file = backend.open_leaf(&directory, name)?;
        meta_of(&file)
    });
    match opened {
        Ok(meta) if meta.regular => Observation::Present(meta),
        Ok(meta) if meta.directory => {
            Observation::Unreadable(FileError::new("is_directory", directory_message()))
        }
        Ok(_) => Observation::Unreadable(FileError::new(
            "not_regular_file",
            "path is no longer a regular file",
        )),
        Err(error) if error.code == "not_found" => Observation::Missing,
        Err(error) => Observation::Unreadable(error),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// `modified` or `deleted`.
    pub kind: &'static str,
    pub disk_sha256: Option<String>,
    pub disk_size: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSuccess {
    pub meta: Meta,
    pub sha256: String,
}
#[derive(Clone, Debug)]
pub struct SaveFailure {
    pub error: FileError,
    /// True only when the result could not be confirmed (post-rename failures).
    pub outcome_unknown: bool,
    pub conflict: Option<Conflict>,
    /// The new content is visible on disk even though the save reports an error.
    pub committed: Option<SaveSuccess>,
}
impl SaveFailure {
    fn plain(error: FileError) -> Box<Self> {
        Box::new(Self {
            error,
            outcome_unknown: false,
            conflict: None,
            committed: None,
        })
    }
    fn conflict(conflict: Conflict) -> Box<Self> {
        Box::new(Self {
            error: FileError::new("conflict", "file changed on disk since it was loaded"),
            outcome_unknown: false,
            conflict: Some(conflict),
            committed: None,
        })
    }
}

/// On-disk size of a save: the body plus the UTF-8 BOM when the baseline had one.
pub fn saved_size(body_len: usize, bom: bool) -> usize {
    body_len + if bom { UTF8_BOM.len() } else { 0 }
}

/// Atomic same-directory save (design D13). `fence` is consulted after the temporary file is
/// durable and immediately before the rename, the last point where a write can still be undone.
pub fn save(
    backend: &dyn FileBackend,
    path: &Path,
    body: &[u8],
    bom: bool,
    expected_sha256: &str,
    force: bool,
    fence: &dyn Fn() -> bool,
) -> Result<SaveSuccess, Box<SaveFailure>> {
    // The cap applies to the bytes that land on disk: a file the service writes must stay one
    // it can read back, so a preserved BOM counts against the same limit as the body.
    if saved_size(body.len(), bom) > MAX_TEXT_BYTES {
        return Err(SaveFailure::plain(too_large(MAX_TEXT_BYTES)));
    }
    validate_text(body).map_err(SaveFailure::plain)?;
    let (parent, name) = split(path).map_err(SaveFailure::plain)?;
    let directory = backend.open_dir(parent).map_err(SaveFailure::plain)?;

    let mut existing = match backend.open_leaf(&directory, name) {
        Ok(file) => {
            let meta = meta_of(&file).map_err(SaveFailure::plain)?;
            if meta.directory {
                return Err(SaveFailure::plain(FileError::new(
                    "is_directory",
                    directory_message(),
                )));
            }
            if !meta.regular {
                return Err(SaveFailure::plain(FileError::new(
                    "not_regular_file",
                    "only regular files can be saved",
                )));
            }
            if meta.nlink > 1 {
                return Err(SaveFailure::plain(FileError::new(
                    "hard_linked",
                    "file has other hard links; an atomic replace would detach them, so it was not written",
                )));
            }
            Some((file, meta))
        }
        Err(error) if error.code == "not_found" => None,
        Err(error) => return Err(SaveFailure::plain(error)),
    };
    if !force {
        match &mut existing {
            None => {
                return Err(SaveFailure::conflict(Conflict {
                    kind: "deleted",
                    disk_sha256: None,
                    disk_size: None,
                }));
            }
            Some((file, meta)) => {
                let current = backend
                    .read(file, MAX_TEXT_BYTES + 1)
                    .map_err(SaveFailure::plain)?;
                let modified = |sha: Option<String>| {
                    SaveFailure::conflict(Conflict {
                        kind: "modified",
                        disk_sha256: sha,
                        disk_size: Some(meta.fingerprint.size.max(current.len() as u64)),
                    })
                };
                if current.len() > MAX_TEXT_BYTES {
                    return Err(modified(None));
                }
                let sha = sha256_hex(&current);
                if sha != expected_sha256 {
                    return Err(modified(Some(sha)));
                }
            }
        }
    }
    let original = existing.as_ref().map(|(_, meta)| *meta);
    let mode = original.map_or(NEW_FILE_MODE, |meta| meta.mode & 0o7777);
    drop(existing);

    let temporary = format!(".zmux-save-{}.tmp", Uuid::new_v4());
    let mut file = backend
        .create_temp(&directory, &temporary)
        .map_err(SaveFailure::plain)?;
    let cleanup = |error: FileError| -> Box<SaveFailure> {
        match backend.unlink(&directory, &temporary) {
            Ok(()) => SaveFailure::plain(error),
            Err(unlink) if unlink.code == "not_found" => SaveFailure::plain(error),
            Err(unlink) => SaveFailure::plain(FileError::new(
                error.code,
                format!(
                    "{}; the temporary file `{temporary}` could not be removed: {}",
                    error.message, unlink.message
                ),
            )),
        }
    };
    let mut content = Vec::new();
    if bom {
        content.extend_from_slice(&UTF8_BOM);
    }
    content.extend_from_slice(body);
    if let Err(error) = backend
        .write_all(&mut file, &content)
        .and_then(|()| backend.set_mode(&file, mode))
        .and_then(|()| backend.sync_file(&file))
    {
        drop(file);
        return Err(cleanup(error));
    }
    // `file` stays open: after the rename it is the committed file, and its descriptor is the
    // only race-free source of the committed fingerprint (a path lookup could already see an
    // external replacement).
    if !fence() {
        drop(file);
        return Err(cleanup(FileError::new(
            "stale_owner",
            "the view that requested this save was closed or replaced; nothing was written",
        )));
    }
    if !force {
        let unchanged = match backend.open_leaf(&directory, name) {
            Ok(file) => match meta_of(&file) {
                Ok(now) if Some(now.fingerprint) == original.map(|m| m.fingerprint) => Ok(None),
                Ok(now) => Ok(Some(Conflict {
                    kind: "modified",
                    disk_sha256: None,
                    disk_size: Some(now.fingerprint.size),
                })),
                Err(error) => Err(error),
            },
            Err(error) if error.code == "not_found" => Ok(Some(Conflict {
                kind: "deleted",
                disk_sha256: None,
                disk_size: None,
            })),
            Err(error) => Err(error),
        };
        match unchanged {
            Ok(None) => {}
            Ok(Some(conflict)) => {
                // Keep the conflict detail, but surface a temporary that could not be removed
                // in the message exactly as every other cleanup path does.
                let mut failure = cleanup(FileError::new(
                    "conflict",
                    "file changed on disk since it was loaded",
                ));
                failure.conflict = Some(conflict);
                return Err(failure);
            }
            Err(error) => return Err(cleanup(error)),
        }
    }
    if let Err(error) = backend.rename(&directory, &temporary, name) {
        return Err(cleanup(error));
    }
    // Past this point the new content may be visible: never report a plain failure.
    let sha256 = sha256_hex(&content);
    // The fingerprint comes from the descriptor that was written, never from the path: an
    // external overwrite after `renameat` then shows up as a change at the next poll instead
    // of being adopted as our own baseline. (`fstat` after the rename also carries the
    // rename's ctime update.)
    let committed = meta_of(&file).map(|meta| SaveSuccess {
        meta,
        sha256: sha256.clone(),
    });
    drop(file);
    if let Err(error) = backend.sync_dir(&directory) {
        return Err(Box::new(SaveFailure {
            error: FileError::new(
                "sync_failed",
                format!(
                    "the file was replaced but the directory could not be synced: {}",
                    error.message
                ),
            ),
            outcome_unknown: true,
            conflict: None,
            committed: committed.ok(),
        }));
    }
    committed.map_err(|error| {
        Box::new(SaveFailure {
            error: FileError::new(
                "io_error",
                format!(
                    "the file was replaced but could not be inspected: {}",
                    error.message
                ),
            ),
            outcome_unknown: true,
            conflict: None,
            committed: None,
        })
    })
}
#[cfg(test)]
#[path = "file_io_tests.rs"]
mod tests;
