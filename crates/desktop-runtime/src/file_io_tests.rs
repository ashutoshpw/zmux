//! Real-file tests: every behaviour here runs against a temporary directory on this machine.
use super::*;
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("zmux-file-io-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(std::fs::canonicalize(path).unwrap())
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".zmux-save-"))
            .collect()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn text(path: &Path) -> Result<Probe, FileError> {
    probe(&RealFs, path.to_str().unwrap(), None)
}
fn code<T: fmt::Debug>(result: Result<T, FileError>) -> &'static str {
    result.expect_err("expected a file error").code
}

/// Wraps the real backend and fails exactly one operation.
struct Faulty {
    fail: &'static str,
    calls: AtomicUsize,
}
impl Faulty {
    fn new(fail: &'static str) -> Self {
        Self {
            fail,
            calls: AtomicUsize::new(0),
        }
    }
    fn check(&self, op: &'static str) -> Result<(), FileError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if op == self.fail {
            Err(FileError::new("io_error", format!("injected {op} failure")))
        } else {
            Ok(())
        }
    }
}
impl FileBackend for Faulty {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FileError> {
        RealFs.canonicalize(path)
    }
    fn open_dir(&self, path: &Path) -> Result<File, FileError> {
        RealFs.open_dir(path)
    }
    fn open_leaf(&self, dir: &File, name: &OsStr) -> Result<File, FileError> {
        RealFs.open_leaf(dir, name)
    }
    fn writable(&self, dir: &File, name: &OsStr) -> bool {
        RealFs.writable(dir, name)
    }
    fn read(&self, file: &mut File, max_bytes: usize) -> Result<Vec<u8>, FileError> {
        RealFs.read(file, max_bytes)
    }
    fn create_temp(&self, dir: &File, name: &str) -> Result<File, FileError> {
        self.check("create_temp")?;
        RealFs.create_temp(dir, name)
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError> {
        self.check("write_all")?;
        // A torn write: half the bytes land before the failure is reported.
        if self.fail == "torn_write" {
            RealFs.write_all(file, &bytes[..bytes.len() / 2])?;
            return Err(FileError::new("io_error", "injected torn write"));
        }
        RealFs.write_all(file, bytes)
    }
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError> {
        self.check("set_mode")?;
        RealFs.set_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> Result<(), FileError> {
        self.check("sync_file")?;
        RealFs.sync_file(file)
    }
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError> {
        self.check("rename")?;
        RealFs.rename(dir, from, to)
    }
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError> {
        RealFs.unlink(dir, name)
    }
    fn sync_dir(&self, dir: &File) -> Result<(), FileError> {
        self.check("sync_dir")?;
        RealFs.sync_dir(dir)
    }
}
fn save_with(
    backend: &dyn FileBackend,
    path: &Path,
    body: &str,
    expected: &[u8],
    force: bool,
) -> Result<SaveSuccess, Box<SaveFailure>> {
    save(
        backend,
        path,
        body.as_bytes(),
        false,
        &sha256_hex(expected),
        force,
        &|| true,
    )
}

#[test]
fn classification_follows_the_extension_table() {
    let mode = |name: &str| natural_mode(Path::new(name));
    for name in [
        "/a/README.md",
        "/a/x.MARKDOWN",
        "/a/x.mdown",
        "/a/x.mkd",
        "/a/x.mkdn",
    ] {
        assert_eq!(mode(name), FileMode::Markdown, "{name}");
    }
    for name in [
        "/a/x.png",
        "/a/x.JPG",
        "/a/x.jpeg",
        "/a/x.gif",
        "/a/x.webp",
        "/a/x.bmp",
        "/a/x.tif",
        "/a/x.tiff",
        "/a/x.heic",
        "/a/x.heif",
        "/a/x.ico",
    ] {
        assert_eq!(mode(name), FileMode::Image, "{name}");
    }
    assert_eq!(mode("/a/doc.PDF"), FileMode::Pdf);
    // Source, markup, no extension, dotfiles and a trailing dot are all text.
    for name in [
        "/a/main.rs",
        "/a/page.html",
        "/a/p.htm",
        "/a/i.svg",
        "/a/d.xml",
        "/a/Makefile",
        "/a/.bashrc",
        "/a/trailing.",
        "/a/.md",
    ] {
        assert_eq!(mode(name), FileMode::Text, "{name}");
    }
    assert_eq!(
        classify(Path::new("/a/README"), Some(FileMode::Markdown)).unwrap(),
        FileMode::Markdown
    );
    assert_eq!(
        classify(Path::new("/a/x.png"), Some(FileMode::Markdown))
            .unwrap_err()
            .code,
        "content_mismatch"
    );
}

#[test]
fn sniff_and_validation_cover_nul_utf8_and_window_boundaries() {
    assert_eq!(sniff_text(b"plain text").unwrap(), ());
    assert_eq!(sniff_text(b"a\0b").unwrap_err().code, "binary_file");
    assert_eq!(sniff_text(&[b'a', 0xFF]).unwrap_err().code, "invalid_utf8");
    // A multibyte character cut by the window is not an error for the sniff, but is for the file.
    let cut = "é".as_bytes()[..1].to_vec();
    assert!(sniff_text(&cut).is_ok());
    assert_eq!(validate_text(&cut).unwrap_err().code, "invalid_utf8");
    assert!(validate_text("héllo 🌍".as_bytes()).is_ok());
    assert_eq!(validate_text(b"x\0").unwrap_err().code, "binary_file");
}

#[test]
fn probe_validates_the_whole_file_not_only_the_sniff_window() {
    let dir = Dir::new();
    let mut late_nul = vec![b'a'; SNIFF_BYTES + 1];
    late_nul.push(0);
    assert_eq!(
        code(text(&dir.file("late-nul.txt", &late_nul))),
        "binary_file"
    );

    let mut last_byte_nul = vec![b'a'; MAX_TEXT_BYTES - 1];
    last_byte_nul.push(0);
    assert_eq!(
        code(text(&dir.file("last.txt", &last_byte_nul))),
        "binary_file"
    );

    let mut late_invalid = vec![b'a'; 20_000];
    late_invalid.push(0xFF);
    late_invalid.extend(vec![b'a'; 100]);
    assert_eq!(
        code(text(&dir.file("late-bad.txt", &late_invalid))),
        "invalid_utf8"
    );

    // Exactly the cap is accepted; one byte more is not.
    let exact = text(&dir.file("exact.txt", &vec![b'a'; MAX_TEXT_BYTES])).unwrap();
    assert_eq!(exact.size, MAX_TEXT_BYTES as u64);
    assert_eq!(exact.mode, FileMode::Text);
    assert_eq!(
        code(text(&dir.file("over.txt", &vec![b'a'; MAX_TEXT_BYTES + 1]))),
        "too_large"
    );
    assert_eq!(
        code(text(&dir.file("over.md", &vec![b'a'; MAX_TEXT_BYTES + 1]))),
        "too_large"
    );
}

#[test]
fn preview_modes_check_size_and_signature() {
    let dir = Dir::new();
    let png = [
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..],
        b"body",
    ]
    .concat();
    assert_eq!(
        text(&dir.file("a.png", &png)).unwrap().mode,
        FileMode::Image
    );
    assert_eq!(
        code(text(&dir.file("bad.png", b"not a png"))),
        "content_mismatch"
    );
    assert_eq!(
        text(&dir.file("a.jpg", &[0xFF, 0xD8, 0xFF, 0xE0]))
            .unwrap()
            .mode,
        FileMode::Image
    );
    assert_eq!(
        code(text(&dir.file("bad.jpeg", b"nope"))),
        "content_mismatch"
    );
    assert_eq!(
        text(&dir.file("a.gif", b"GIF89a....")).unwrap().mode,
        FileMode::Image
    );
    assert_eq!(
        code(text(&dir.file("bad.gif", b"GIF90a"))),
        "content_mismatch"
    );
    // No signature is required for the other raster formats.
    assert_eq!(
        text(&dir.file("a.webp", b"anything")).unwrap().mode,
        FileMode::Image
    );

    let mut pdf = vec![b' '; 900];
    pdf.extend_from_slice(b"%PDF-1.7\n");
    assert_eq!(text(&dir.file("a.pdf", &pdf)).unwrap().mode, FileMode::Pdf);
    let mut late = vec![b' '; 1100];
    late.extend_from_slice(b"%PDF-1.7");
    assert_eq!(code(text(&dir.file("late.pdf", &late))), "content_mismatch");

    let mut big = png.clone();
    big.resize(MAX_PREVIEW_BYTES, 0);
    assert!(text(&dir.file("max.png", &big)).is_ok());
    big.push(0);
    assert_eq!(code(text(&dir.file("max-plus-one.png", &big))), "too_large");
}

#[test]
fn media_and_binary_files_are_explicit_errors() {
    let dir = Dir::new();
    let error = text(&dir.file("song.mp3", &[0x49, 0x44, 0x33, 0, 0, 0])).unwrap_err();
    assert_eq!(error.code, "binary_file");
    assert!(error.message.contains("media preview is not implemented"));
}

#[test]
fn probe_rejects_bad_operands_directories_and_missing_files() {
    let dir = Dir::new();
    for operand in ["", "relative.txt", "/with\0nul", "/with\ncontrol"] {
        assert_eq!(
            code(probe(&RealFs, operand, None)),
            "invalid_operand",
            "{operand:?}"
        );
    }
    let long = format!("/{}", "a".repeat(MAX_PATH_BYTES));
    assert_eq!(code(probe(&RealFs, &long, None)), "invalid_operand");
    assert_eq!(code(text(&dir.0.join("missing.txt"))), "not_found");
    let error = text(&dir.0).unwrap_err();
    assert_eq!(error.code, "is_directory");
    assert!(error.message.contains("directories cannot be opened"));
}

#[test]
fn probe_stores_the_canonical_path_of_a_symlink_target() {
    let dir = Dir::new();
    let target = dir.file("target.txt", b"hello");
    let link = dir.0.join("link.txt");
    symlink(&target, &link).unwrap();
    let probed = text(&link).unwrap();
    assert_eq!(probed.path, target.to_str().unwrap());
    // A directory reached through a symlinked component resolves once, at open time.
    std::fs::create_dir(dir.0.join("real")).unwrap();
    dir.file("real/inner.txt", b"x");
    symlink(dir.0.join("real"), dir.0.join("alias")).unwrap();
    assert_eq!(
        text(&dir.0.join("alias/inner.txt")).unwrap().path,
        dir.0.join("real/inner.txt").to_str().unwrap()
    );
}

#[test]
fn later_operations_refuse_symlink_swaps_with_path_changed() {
    let dir = Dir::new();
    let canonical = dir.file("file.txt", b"hello");
    assert!(load(&RealFs, &canonical, FileMode::Text).is_ok());
    // The leaf becomes a symlink after the probe.
    std::fs::remove_file(&canonical).unwrap();
    let elsewhere = dir.file("elsewhere.txt", b"other");
    symlink(&elsewhere, &canonical).unwrap();
    assert_eq!(
        code(load(&RealFs, &canonical, FileMode::Text)),
        "path_changed"
    );
    assert_eq!(
        observe(&RealFs, &canonical),
        Observation::Unreadable(FileError::new("path_changed", PATH_CHANGED))
    );
    let failure = save_with(&RealFs, &canonical, "overwrite", b"hello", true).unwrap_err();
    assert_eq!(failure.error.code, "path_changed");
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"other");

    // A directory component becomes a symlink.
    std::fs::create_dir(dir.0.join("sub")).unwrap();
    let inner = dir.file("sub/in.txt", b"inner");
    assert!(load(&RealFs, &inner, FileMode::Text).is_ok());
    std::fs::rename(dir.0.join("sub"), dir.0.join("sub-moved")).unwrap();
    symlink(dir.0.join("sub-moved"), dir.0.join("sub")).unwrap();
    assert_eq!(code(load(&RealFs, &inner, FileMode::Text)), "path_changed");
}

#[test]
fn fifo_directory_and_device_are_rejected_without_blocking() {
    let dir = Dir::new();
    let fifo = dir.0.join("pipe");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    // A blocking open of a FIFO with no writer would hang this test forever.
    assert_eq!(code(text(&fifo)), "not_regular_file");
    assert_eq!(
        code(load(&RealFs, &fifo, FileMode::Text)),
        "not_regular_file"
    );
    assert_eq!(
        save_with(&RealFs, &fifo, "x", b"", true)
            .unwrap_err()
            .error
            .code,
        "not_regular_file"
    );
    assert_eq!(code(text(Path::new("/dev/null"))), "not_regular_file");
    assert_eq!(code(text(&dir.0)), "is_directory");
    assert!(matches!(
        observe(&RealFs, &fifo),
        Observation::Unreadable(error) if error.code == "not_regular_file"
    ));
}

#[test]
fn bom_is_a_flag_and_survives_a_save_round_trip() {
    let dir = Dir::new();
    let path = dir.file("bom.txt", b"\xEF\xBB\xBFhello");
    let loaded = load(&RealFs, &path, FileMode::Text).unwrap();
    assert!(loaded.bom);
    assert_eq!(loaded.body(), b"hello");
    assert_eq!(loaded.sha256, sha256_hex(b"\xEF\xBB\xBFhello"));
    let saved = save(
        &RealFs,
        &path,
        b"changed",
        loaded.bom,
        &loaded.sha256,
        false,
        &|| true,
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBFchanged");
    assert_eq!(saved.sha256, sha256_hex(b"\xEF\xBB\xBFchanged"));
}

#[test]
fn save_preserves_permission_bits_and_leaves_no_temporary_file() {
    let dir = Dir::new();
    for mode in [0o755, 0o600, 0o640, 0o444] {
        let path = dir.file(&format!("m{mode:o}.sh"), b"old");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let saved = save_with(&RealFs, &path, "new", b"old", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            mode,
            "{mode:o}"
        );
        assert_eq!(saved.meta.mode & 0o7777, mode);
    }
    assert!(dir.leftovers().is_empty(), "{:?}", dir.leftovers());
    // A file recreated by force gets the documented default.
    let gone = dir.0.join("gone.txt");
    save_with(&RealFs, &gone, "fresh", b"", true).unwrap();
    assert_eq!(
        std::fs::metadata(&gone).unwrap().permissions().mode() & 0o7777,
        NEW_FILE_MODE
    );
}

#[test]
fn save_conflicts_are_content_based_and_force_overrides() {
    let dir = Dir::new();
    let path = dir.file("c.txt", b"base");
    // Identical rewrite and touch are not conflicts.
    std::fs::write(&path, b"base").unwrap();
    let touched = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    touched
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
        .unwrap();
    save_with(&RealFs, &path, "mine", b"base", false).unwrap();

    std::fs::write(&path, b"theirs").unwrap();
    let failure = save_with(&RealFs, &path, "mine2", b"mine", false).unwrap_err();
    assert_eq!(failure.error.code, "conflict");
    let conflict = failure.conflict.unwrap();
    assert_eq!(conflict.kind, "modified");
    assert_eq!(conflict.disk_sha256, Some(sha256_hex(b"theirs")));
    assert_eq!(conflict.disk_size, Some(6));
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    assert!(dir.leftovers().is_empty());
    save_with(&RealFs, &path, "forced", b"mine", true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"forced");

    std::fs::remove_file(&path).unwrap();
    let failure = save_with(&RealFs, &path, "x", b"forced", false).unwrap_err();
    assert_eq!(failure.conflict.unwrap().kind, "deleted");
    assert!(!path.exists());
    save_with(&RealFs, &path, "recreated", b"forced", true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"recreated");

    // A file that grew past the cap cannot be proven equal.
    std::fs::write(&path, vec![b'a'; MAX_TEXT_BYTES + 5]).unwrap();
    let failure = save_with(&RealFs, &path, "x", b"recreated", false).unwrap_err();
    assert_eq!(failure.conflict.unwrap().disk_sha256, None);
}

#[test]
fn a_change_between_the_check_and_the_rename_is_a_conflict() {
    let dir = Dir::new();
    let path = dir.file("race.txt", b"base");
    let expected = sha256_hex(b"base");
    let during = path.clone();
    let failure = save(
        &RealFs,
        &path,
        b"mine",
        false,
        &expected,
        false,
        // The fence runs after the temporary file exists and before the final re-check.
        &|| {
            std::fs::write(&during, b"racing writer").unwrap();
            true
        },
    )
    .unwrap_err();
    assert_eq!(failure.conflict.unwrap().kind, "modified");
    assert_eq!(std::fs::read(&path).unwrap(), b"racing writer");
    assert!(dir.leftovers().is_empty());
}

#[test]
fn a_refused_fence_writes_nothing_and_removes_the_temporary_file() {
    let dir = Dir::new();
    let path = dir.file("fenced.txt", b"base");
    let failure = save(
        &RealFs,
        &path,
        b"mine",
        false,
        &sha256_hex(b"base"),
        false,
        &|| false,
    )
    .unwrap_err();
    assert_eq!(failure.error.code, "stale_owner");
    assert_eq!(std::fs::read(&path).unwrap(), b"base");
    assert!(dir.leftovers().is_empty());
}

#[test]
fn injected_failures_never_tear_the_file_or_leak_temporaries() {
    let dir = Dir::new();
    let path = dir.file("atomic.txt", b"original content");
    for step in [
        "create_temp",
        "write_all",
        "torn_write",
        "set_mode",
        "sync_file",
        "rename",
    ] {
        let backend = Faulty::new(step);
        let failure = save_with(
            &backend,
            &path,
            "replacement text",
            b"original content",
            false,
        )
        .unwrap_err();
        assert_eq!(failure.error.code, "io_error", "{step}");
        assert!(
            !failure.outcome_unknown,
            "{step}: a pre-rename failure is known not written"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original content", "{step}");
        assert!(dir.leftovers().is_empty(), "{step}: {:?}", dir.leftovers());
    }
    // After the rename the new content is visible: the failure must say so.
    let backend = Faulty::new("sync_dir");
    let failure = save_with(
        &backend,
        &path,
        "replacement text",
        b"original content",
        false,
    )
    .unwrap_err();
    assert_eq!(failure.error.code, "sync_failed");
    assert!(failure.outcome_unknown);
    let committed = failure.committed.expect("visible content is reported");
    assert_eq!(committed.sha256, sha256_hex(b"replacement text"));
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement text");
}

#[test]
fn read_only_directory_reports_directory_not_writable_without_in_place_fallback() {
    if rustix::process::geteuid().is_root() {
        // Root bypasses permission bits; the injected mapping below still covers the code path.
        return;
    }
    let dir = Dir::new();
    let path = dir.file("ro.txt", b"base");
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    let failure = save_with(&RealFs, &path, "mine", b"base", false).unwrap_err();
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(failure.error.code, "directory_not_writable");
    assert_eq!(std::fs::read(&path).unwrap(), b"base");
}

#[test]
fn hard_linked_files_read_but_do_not_save() {
    let dir = Dir::new();
    let path = dir.file("one.txt", b"shared");
    std::fs::hard_link(&path, dir.0.join("two.txt")).unwrap();
    assert!(load(&RealFs, &path, FileMode::Text).is_ok());
    let failure = save_with(&RealFs, &path, "x", b"shared", true).unwrap_err();
    assert_eq!(failure.error.code, "hard_linked");
    assert_eq!(std::fs::read(&path).unwrap(), b"shared");
    assert!(dir.leftovers().is_empty());
}

#[test]
fn save_validates_the_body() {
    let dir = Dir::new();
    let path = dir.file("body.txt", b"base");
    let hash = sha256_hex(b"base");
    let invalid = save(&RealFs, &path, &[0xFF, 0xFE], false, &hash, false, &|| true).unwrap_err();
    assert_eq!(invalid.error.code, "invalid_utf8");
    let nul = save(&RealFs, &path, b"a\0b", false, &hash, false, &|| true).unwrap_err();
    assert_eq!(nul.error.code, "binary_file");
    let big = vec![b'a'; MAX_TEXT_BYTES + 1];
    let large = save(&RealFs, &path, &big, false, &hash, false, &|| true).unwrap_err();
    assert_eq!(large.error.code, "too_large");
    assert_eq!(std::fs::read(&path).unwrap(), b"base");
    let exact = vec![b'a'; MAX_TEXT_BYTES];
    assert!(save(&RealFs, &path, &exact, false, &hash, false, &|| true).is_ok());
}

#[test]
fn fingerprints_ignore_nothing_cheap_but_content_hash_ignores_touch() {
    let dir = Dir::new();
    let path = dir.file("fp.txt", b"same");
    let before = load(&RealFs, &path, FileMode::Text).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(120))
        .unwrap();
    let after = load(&RealFs, &path, FileMode::Text).unwrap();
    assert_ne!(before.meta.fingerprint, after.meta.fingerprint);
    assert_eq!(before.sha256, after.sha256);
    assert_eq!(before.meta.fingerprint.ino, after.meta.fingerprint.ino);
    assert_eq!(observe(&RealFs, &path), Observation::Present(after.meta));
    assert_eq!(
        observe(&RealFs, &dir.0.join("missing")),
        Observation::Missing
    );
    // Missing parent directory also reads as missing.
    assert_eq!(
        observe(&RealFs, &dir.0.join("no/dir/file")),
        Observation::Missing
    );
}

#[test]
fn sha256_hex_matches_a_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn backend_trait_objects_can_be_shared() {
    let backend: Arc<dyn FileBackend> = Arc::new(RealFs);
    let dir = Dir::new();
    let path = dir.file("shared.txt", b"x");
    assert!(load(backend.as_ref(), &path, FileMode::Text).is_ok());
}
