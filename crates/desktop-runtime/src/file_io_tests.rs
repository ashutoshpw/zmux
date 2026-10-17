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
        self.check("unlink")?;
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
    // rustix::fs::mknodat does not exist on Apple targets, so use mkfifo(1), which both
    // supported platforms ship.
    let made = std::process::Command::new("mkfifo")
        .arg("-m")
        .arg("600")
        .arg(&fifo)
        .status()
        .expect("mkfifo is available");
    assert!(made.success(), "mkfifo failed: {made}");
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
fn a_preserved_bom_counts_against_the_save_cap_so_the_file_stays_readable() {
    let dir = Dir::new();
    let mut original = vec![0xEF, 0xBB, 0xBF];
    original.extend(vec![b'a'; MAX_TEXT_BYTES - 3]);
    let path = dir.file("bom.txt", &original);
    let loaded = load(&RealFs, &path, FileMode::Text).unwrap();
    assert!(loaded.bom);
    // The body alone fits, but BOM + body would be three bytes over what `load` accepts.
    let body = vec![b'b'; MAX_TEXT_BYTES];
    let refused = save(&RealFs, &path, &body, true, &loaded.sha256, false, &|| true).unwrap_err();
    assert_eq!(refused.error.code, "too_large");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "nothing was written"
    );
    // The largest body that keeps the whole file within the cap saves and reads back.
    let fits = vec![b'b'; MAX_TEXT_BYTES - 3];
    assert!(save(&RealFs, &path, &fits, true, &loaded.sha256, false, &|| true).is_ok());
    let reread = load(&RealFs, &path, FileMode::Text).unwrap();
    assert!(reread.bom);
    assert_eq!(reread.bytes.len(), MAX_TEXT_BYTES);
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

#[test]
fn a_conflict_found_at_the_last_moment_still_reports_a_temporary_it_could_not_remove() {
    let dir = Dir::new();
    let path = dir.file("leak.txt", b"base");
    let expected = sha256_hex(b"base");
    let during = path.clone();
    let failure = save(
        &Faulty::new("unlink"),
        &path,
        b"mine",
        false,
        &expected,
        false,
        &|| {
            std::fs::write(&during, b"racing writer").unwrap();
            true
        },
    )
    .unwrap_err();
    // The conflict detail survives, and the leaked temporary is named, never swallowed.
    assert_eq!(failure.error.code, "conflict");
    assert_eq!(failure.conflict.as_ref().unwrap().kind, "modified");
    let leftovers = dir.leftovers();
    assert_eq!(
        leftovers.len(),
        1,
        "the failing unlink leaves the temporary"
    );
    assert!(
        failure.error.message.contains(&leftovers[0])
            && failure.error.message.contains("could not be removed"),
        "message must report the leak: {}",
        failure.error.message
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"racing writer");

    // The deleted-at-the-last-moment variant takes the same branch.
    let dir = Dir::new();
    let path = dir.file("gone.txt", b"base");
    let during = path.clone();
    let failure = save(
        &Faulty::new("unlink"),
        &path,
        b"mine",
        false,
        &expected,
        false,
        &|| {
            std::fs::remove_file(&during).unwrap();
            true
        },
    )
    .unwrap_err();
    assert_eq!(failure.conflict.as_ref().unwrap().kind, "deleted");
    assert!(failure.error.message.contains("could not be removed"));
}

#[test]
fn the_leaf_open_flags_forbid_a_controlling_terminal_symlinks_and_blocking() {
    for flag in [
        OFlags::NOCTTY,
        OFlags::NOFOLLOW,
        OFlags::NONBLOCK,
        OFlags::CLOEXEC,
        OFlags::RDONLY,
    ] {
        assert!(LEAF_OPEN_FLAGS.contains(flag), "missing {flag:?}");
    }
    assert!(!LEAF_OPEN_FLAGS.intersects(OFlags::WRONLY | OFlags::RDWR | OFlags::CREATE));
}

const CTTY_SLAVE_ENV: &str = "ZMUX_FILE_IO_CTTY_SLAVE";

/// Child half of `opening_a_terminal_never_acquires_a_controlling_terminal`. It does nothing
/// unless the parent test launched it (new session, no controlling terminal) with a slave path.
#[test]
fn ctty_child_probe() {
    let Ok(slave) = std::env::var(CTTY_SLAVE_ENV) else {
        return;
    };
    let controlling_terminal = || std::fs::OpenOptions::new().read(true).open("/dev/tty");
    assert!(
        controlling_terminal().is_err(),
        "the child must start without a controlling terminal"
    );
    let slave = PathBuf::from(slave);
    let directory = RealFs.open_dir(slave.parent().unwrap()).unwrap();
    for _ in 0..3 {
        // `observe`, `load` and `save` all share this open, and the poller repeats it.
        let error = RealFs
            .open_leaf(&directory, slave.file_name().unwrap())
            .expect_err("a terminal is never opened");
        assert_eq!(error.code, "not_regular_file");
    }
    assert!(
        controlling_terminal().is_err(),
        "opening a pty slave must not hand the process a controlling terminal"
    );
    println!("ctty-child-verified");
}

#[test]
#[cfg(target_os = "linux")]
fn opening_a_terminal_never_acquires_a_controlling_terminal() {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let slave = ptsname(&master, Vec::new()).unwrap();
    let slave = slave.to_str().unwrap().to_owned();
    // `setsid -w` runs the child as a session leader with no controlling terminal, the state
    // in which an unguarded open of a terminal would adopt it.
    let output = std::process::Command::new("setsid")
        .arg("-w")
        .arg(std::env::current_exe().unwrap())
        .args(["ctty_child_probe", "--nocapture", "--test-threads=1"])
        .env(CTTY_SLAVE_ENV, &slave)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("util-linux `setsid` is required for this test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("ctty-child-verified"),
        "child failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(master);
}

/// Restores permissive bits on drop so a failing test still lets the temporary tree be removed.
struct Restore(PathBuf);
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn search_only_ancestors_do_not_block_probe_load_observe_save_or_the_walk() {
    if rustix::process::geteuid().is_root() {
        // Root bypasses permission bits, so the scenario cannot be created. Recorded, not hidden.
        eprintln!("SKIPPED search_only_ancestors_*: running as root");
        return;
    }
    let dir = Dir::new();
    // `search` is mode 0311 (owner -wx): the user can traverse it and create entries but cannot
    // list it, so an `O_RDONLY` open of the directory is `EACCES` while `open(2)` of a file below
    // it succeeds. `inner` shows a search-only grandparent above an ordinary directory.
    let search = dir.0.join("search");
    std::fs::create_dir(&search).unwrap();
    std::fs::create_dir(search.join("inner")).unwrap();
    let direct = dir.file("search/a.txt", b"direct");
    let nested = dir.file("search/inner/b.md", b"nested");
    std::fs::set_permissions(&search, std::fs::Permissions::from_mode(0o311)).unwrap();
    let _restore = Restore(search.clone());
    // The premise: an ordinary directory open is refused, a plain file open is not.
    assert!(std::fs::read_dir(&search).is_err());
    assert_eq!(std::fs::read(&direct).unwrap(), b"direct");

    // probe (canonicalize + whole-file validation), through the file as a direct child and
    // through the search-only directory as an ancestor.
    let probed = text(&direct).unwrap();
    assert_eq!((probed.mode, probed.size), (FileMode::Text, 6));
    assert_eq!(text(&nested).unwrap().mode, FileMode::Markdown);
    // load and observe.
    let loaded = load(&RealFs, &direct, FileMode::Text).unwrap();
    assert_eq!(loaded.body(), b"direct");
    assert!(loaded.writable, "owner has w and x on the directory");
    assert!(matches!(
        observe(&RealFs, &nested),
        Observation::Present(meta) if meta.regular
    ));
    // The leaf below a search-only directory that does not exist is still `not_found`.
    assert_eq!(
        observe(&RealFs, &search.join("missing.txt")),
        Observation::Missing
    );
    // Symlink refusal still holds inside a search-only directory.
    symlink(&nested, search.join("link.txt")).unwrap();
    assert_eq!(
        code(load(&RealFs, &search.join("link.txt"), FileMode::Text)),
        "path_changed"
    );

    // save: the directory is the search-only one (temp create, fchmod, fsync, renameat, and a
    // directory fsync that cannot happen on a traversal-only descriptor but is not an error).
    let saved = save_with(&RealFs, &direct, "direct v2", b"direct", false).unwrap();
    assert_eq!(saved.sha256, sha256_hex(b"direct v2"));
    assert_eq!(std::fs::read(&direct).unwrap(), b"direct v2");
    // save below the search-only ancestor, in an ordinary directory.
    save_with(&RealFs, &nested, "nested v2", b"nested", false).unwrap();
    assert_eq!(std::fs::read(&nested).unwrap(), b"nested v2");
    // A conflict is still found through the search-only walk.
    std::fs::write(&direct, b"external").unwrap();
    let failure = save_with(&RealFs, &direct, "mine", b"direct v2", false).unwrap_err();
    assert_eq!(failure.conflict.unwrap().kind, "modified");
    std::fs::set_permissions(&search, std::fs::Permissions::from_mode(0o755)).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(&search)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".zmux-save-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_directory_with_no_search_permission_is_still_permission_denied() {
    if rustix::process::geteuid().is_root() {
        eprintln!("SKIPPED a_directory_with_no_search_*: running as root");
        return;
    }
    let dir = Dir::new();
    let sealed = dir.0.join("sealed");
    std::fs::create_dir(&sealed).unwrap();
    let path = dir.file("sealed/a.txt", b"x");
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _restore = Restore(sealed);
    // The fallback is traversal-only: it cannot widen access past a missing `x` bit.
    assert_eq!(
        code(load(&RealFs, &path, FileMode::Text)),
        "permission_denied"
    );
}

/// Counts the bytes handed back by `read`, to prove how much of a file an operation consumed.
struct ReadSpy {
    bytes: AtomicUsize,
    calls: AtomicUsize,
}
impl ReadSpy {
    fn new() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        }
    }
}
impl FileBackend for ReadSpy {
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
        let bytes = RealFs.read(file, max_bytes)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.bytes.fetch_add(bytes.len(), Ordering::SeqCst);
        Ok(bytes)
    }
    fn create_temp(&self, dir: &File, name: &str) -> Result<File, FileError> {
        RealFs.create_temp(dir, name)
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError> {
        RealFs.write_all(file, bytes)
    }
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError> {
        RealFs.set_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> Result<(), FileError> {
        RealFs.sync_file(file)
    }
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError> {
        RealFs.rename(dir, from, to)
    }
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError> {
        RealFs.unlink(dir, name)
    }
    fn sync_dir(&self, dir: &File) -> Result<(), FileError> {
        RealFs.sync_dir(dir)
    }
}

#[test]
fn image_and_pdf_probes_use_fstat_size_and_signature_without_reading_or_hashing_the_file() {
    let dir = Dir::new();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.resize(MAX_PREVIEW_BYTES, 7);
    let mut pdf = b"%PDF-1.7\n".to_vec();
    pdf.resize(MAX_PREVIEW_BYTES, b' ');
    for (name, bytes, mode) in [
        ("big.png", png, FileMode::Image),
        ("big.pdf", pdf, FileMode::Pdf),
    ] {
        let path = dir.file(name, &bytes);
        let spy = ReadSpy::new();
        let probed = probe(&spy, path.to_str().unwrap(), None).unwrap();
        assert_eq!(probed.mode, mode);
        assert_eq!(probed.size as usize, MAX_PREVIEW_BYTES);
        assert_eq!(probed.sha256, None, "{name}: a preview probe never hashes");
        let read = spy.bytes.load(Ordering::SeqCst);
        assert!(
            read <= SIGNATURE_BYTES && spy.calls.load(Ordering::SeqCst) == 1,
            "{name}: probe read {read} bytes in {} reads",
            spy.calls.load(Ordering::SeqCst)
        );
    }
    // Size is still judged from fstat, before anything is read.
    let mut over = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    over.resize(MAX_PREVIEW_BYTES + 1, 0);
    let spy = ReadSpy::new();
    let path = dir.file("over.png", &over);
    assert_eq!(code(probe(&spy, path.to_str().unwrap(), None)), "too_large");
    assert_eq!(spy.calls.load(Ordering::SeqCst), 0);
    // Text and Markdown still read and hash the whole file.
    let path = dir.file("t.txt", &vec![b'a'; 100_000]);
    let spy = ReadSpy::new();
    let probed = probe(&spy, path.to_str().unwrap(), None).unwrap();
    assert_eq!(probed.sha256, Some(sha256_hex(&vec![b'a'; 100_000])));
    assert_eq!(spy.bytes.load(Ordering::SeqCst), 100_000);
}

/// Replaces the saved path with a different file right after the real `renameat`, the window
/// in which an external writer can overwrite what the save just committed. Records the inode
/// of the temporary file, which becomes the committed file.
struct OverwriteAfterRename {
    target: PathBuf,
    temp_inode: std::sync::Mutex<Option<u64>>,
}
impl FileBackend for OverwriteAfterRename {
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
        let file = RealFs.create_temp(dir, name)?;
        *self.temp_inode.lock().unwrap() = Some(meta_of(&file)?.fingerprint.ino);
        Ok(file)
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError> {
        RealFs.write_all(file, bytes)
    }
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError> {
        RealFs.set_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> Result<(), FileError> {
        RealFs.sync_file(file)
    }
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError> {
        RealFs.rename(dir, from, to)?;
        // An external editor atomically replaces the file we just committed.
        let elsewhere = self.target.with_file_name("external-writer.tmp");
        std::fs::write(&elsewhere, b"external overwrite").unwrap();
        std::fs::rename(&elsewhere, &self.target).unwrap();
        Ok(())
    }
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError> {
        RealFs.unlink(dir, name)
    }
    fn sync_dir(&self, dir: &File) -> Result<(), FileError> {
        RealFs.sync_dir(dir)
    }
}

#[test]
fn the_committed_fingerprint_is_taken_from_the_written_descriptor_not_the_path() {
    let dir = Dir::new();
    let path = dir.file("window.txt", b"base");
    let backend = OverwriteAfterRename {
        target: path.clone(),
        temp_inode: std::sync::Mutex::new(None),
    };
    let saved = save_with(&backend, &path, "mine", b"base", false).unwrap();
    let temp_inode = backend.temp_inode.lock().unwrap().expect("temp created");
    // The baseline describes the inode that was written and fsynced, not the replacement.
    assert_eq!(saved.meta.fingerprint.ino, temp_inode);
    assert_eq!(saved.sha256, sha256_hex(b"mine"));
    assert_eq!(saved.meta.fingerprint.size, 4);
    // So the next observation of the path is a different file and is reported as a change.
    match observe(&RealFs, &path) {
        Observation::Present(now) => {
            assert_ne!(now.fingerprint, saved.meta.fingerprint);
            assert_ne!(now.fingerprint.ino, temp_inode);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"external overwrite");
}

#[test]
fn an_undisturbed_save_reports_the_fingerprint_the_next_poll_sees() {
    let dir = Dir::new();
    let path = dir.file("steady.txt", b"base");
    let saved = save_with(&RealFs, &path, "mine", b"base", false).unwrap();
    match observe(&RealFs, &path) {
        Observation::Present(now) => assert_eq!(now.fingerprint, saved.meta.fingerprint),
        other => panic!("{other:?}"),
    }
}
