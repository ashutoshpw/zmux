//! File service behaviour against real temporary files and a real Desktop. The file worker is
//! observed only through the public Desktop methods, the published snapshot and the disk.
#[path = "support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{
    Command, Desktop, DesktopOptions, MAX_FILE_SURFACES, Request, Shell, Snapshot,
    attention::AttentionCommand,
    control::{CONTROL_VERSION, ControlServer},
    control_policy::{ControlAccess, ControlPolicy},
    file_io::{FileBackend, FileError, RealFs, sha256_hex},
    files::{FileFailure, FileOwner, FilesConfig},
};
use zmux_workspace_core::{Action, FileMode, SurfaceId};

/// Wraps the real backend. Any operation can be gated (blocked until released) and every call
/// is counted, so tests can prove what did and did not reach the disk.
#[derive(Default)]
struct Control {
    gated: Mutex<Option<&'static str>>,
    changed: Condvar,
    blocked: AtomicUsize,
    counts: Mutex<HashMap<&'static str, usize>>,
}
impl Control {
    fn arm(&self, op: &'static str) {
        *self.gated.lock().unwrap() = Some(op);
    }
    fn release(&self) {
        *self.gated.lock().unwrap() = None;
        self.changed.notify_all();
    }
    fn count(&self, op: &'static str) -> usize {
        self.counts.lock().unwrap().get(op).copied().unwrap_or(0)
    }
    #[track_caller]
    fn wait_blocked(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.blocked.load(Ordering::SeqCst) < expected {
            assert!(Instant::now() < deadline, "no operation reached the gate");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn pass(&self, op: &'static str) {
        *self.counts.lock().unwrap().entry(op).or_default() += 1;
        let mut gated = self.gated.lock().unwrap();
        if *gated == Some(op) {
            self.blocked.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(30);
            while *gated == Some(op) {
                let (guard, timeout) = self
                    .changed
                    .wait_timeout(gated, deadline.saturating_duration_since(Instant::now()))
                    .unwrap();
                gated = guard;
                assert!(!timeout.timed_out(), "gate was never released");
            }
            self.blocked.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
/// Releases a gate when a failing test unwinds so the worker can still shut down.
struct Release(Arc<Control>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Gated(Arc<Control>);
impl FileBackend for Gated {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, FileError> {
        self.0.pass("canonicalize");
        RealFs.canonicalize(path)
    }
    fn open_dir(&self, path: &Path) -> Result<File, FileError> {
        self.0.pass("open_dir");
        RealFs.open_dir(path)
    }
    fn open_leaf(&self, dir: &File, name: &OsStr) -> Result<File, FileError> {
        self.0.pass("open_leaf");
        RealFs.open_leaf(dir, name)
    }
    fn writable(&self, dir: &File, name: &OsStr) -> bool {
        RealFs.writable(dir, name)
    }
    fn read(&self, file: &mut File, max_bytes: usize) -> Result<Vec<u8>, FileError> {
        self.0.pass("read");
        RealFs.read(file, max_bytes)
    }
    fn create_temp(&self, dir: &File, name: &str) -> Result<File, FileError> {
        self.0.pass("create_temp");
        RealFs.create_temp(dir, name)
    }
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> Result<(), FileError> {
        self.0.pass("write_all");
        RealFs.write_all(file, bytes)
    }
    fn set_mode(&self, file: &File, mode: u32) -> Result<(), FileError> {
        RealFs.set_mode(file, mode)
    }
    fn sync_file(&self, file: &File) -> Result<(), FileError> {
        self.0.pass("sync_file");
        RealFs.sync_file(file)
    }
    fn rename(&self, dir: &File, from: &str, to: &OsStr) -> Result<(), FileError> {
        self.0.pass("rename");
        RealFs.rename(dir, from, to)
    }
    fn unlink(&self, dir: &File, name: &str) -> Result<(), FileError> {
        self.0.pass("unlink");
        RealFs.unlink(dir, name)
    }
    fn sync_dir(&self, dir: &File) -> Result<(), FileError> {
        RealFs.sync_dir(dir)
    }
}

struct Fixture {
    desktop: Arc<Desktop>,
    dir: PathBuf,
    /// Present when the desktop was started with session persistence.
    state: Option<PathBuf>,
    control: Arc<Control>,
    _release: Release,
}
impl Fixture {
    fn new() -> Self {
        Self::with(|_| {})
    }
    fn with(configure: impl FnOnce(&mut FilesConfig)) -> Self {
        Self::build(false, configure)
    }
    /// A desktop with session persistence, so export and restore are available.
    fn persistent() -> Self {
        Self::build(true, |_| {})
    }
    fn build(persistent: bool, configure: impl FnOnce(&mut FilesConfig)) -> Self {
        let state = persistent.then(|| {
            use std::os::unix::fs::PermissionsExt;
            let path = std::env::temp_dir().join(format!("zmux-files-state-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::canonicalize(path).unwrap()
        });
        let control = Arc::new(Control::default());
        let mut config = FilesConfig {
            backend: Arc::new(Gated(control.clone())),
            // Polling is opt-in per test so unrelated assertions never race the poller.
            poll_interval: Duration::from_secs(3600),
            poll_tick: Duration::from_millis(5),
            stall_after: Duration::from_secs(60),
            probe_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(5),
            save_timeout: Duration::from_secs(5),
            ..FilesConfig::default()
        };
        configure(&mut config);
        let desktop = Arc::new(
            Desktop::with_options(
                Shell {
                    program: "/bin/sh".into(),
                    args: vec!["-i".into()],
                    cwd: None,
                },
                DesktopOptions {
                    files: Some(config),
                    state_directory: state.as_ref().map(|path| path.join("state")),
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        let dir = std::env::temp_dir().join(format!("zmux-files-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::fs::canonicalize(dir).unwrap();
        // The starter terminal must be quiet before tests measure revisions.
        wait(&desktop, |s| {
            !s.frames.is_empty()
                && s.frames.iter().all(|f| {
                    f.snapshot
                        .as_ref()
                        .is_some_and(|s| s.state == zmux_session_runtime::State::Running)
                })
        });
        thread::sleep(Duration::from_millis(150));
        Self {
            desktop,
            dir,
            state,
            _release: Release(control.clone()),
            control,
        }
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
    fn open_item(&self, path: &Path) -> Value {
        let result = self
            .desktop
            .file_open(vec![path.to_str().unwrap().into()], None, true)
            .unwrap();
        result["items"][0].clone()
    }
    #[track_caller]
    fn open(&self, name: &str, bytes: &[u8]) -> SurfaceId {
        let path = self.write(name, bytes);
        let item = self.open_item(&path);
        assert_eq!(item["ok"], true, "{item}");
        SurfaceId(item["surface"]["surface_id"].as_u64().unwrap())
    }
    fn epoch(&self) -> Uuid {
        self.snapshot().files.epoch
    }
    fn snapshot(&self) -> Arc<Snapshot> {
        desktop_snapshot::read(&self.desktop)
    }
    fn attach_as(
        &self,
        surface: SurfaceId,
        host: Uuid,
    ) -> Result<desktop_runtime::files::FileContent, FileFailure> {
        self.desktop.file_attach(self.epoch(), surface, host)
    }
    #[track_caller]
    fn attach(&self, surface: SurfaceId) -> (FileOwner, desktop_runtime::files::FileContent) {
        let content = self.attach_as(surface, Uuid::new_v4()).unwrap();
        (content.owner.clone(), content)
    }
    fn entry(&self, surface: SurfaceId) -> Option<Value> {
        let files = serde_json::to_value(&self.snapshot().files).unwrap();
        files["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["surface_id"] == surface.0)
            .cloned()
    }
    #[track_caller]
    fn wait_entry(&self, surface: SurfaceId, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(entry) = self.entry(surface)
                && predicate(&entry)
            {
                return entry;
            }
            assert!(
                Instant::now() < deadline,
                "entry never reached the expected state: {:?}",
                self.entry(surface)
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn close(&self, surface: SurfaceId) {
        self.desktop
            .submit(Request {
                version: 1,
                request_id: 900,
                payload: Command::Action {
                    action: Action::CloseSurface {
                        surface_id: surface,
                    },
                },
            })
            .unwrap();
        wait(&self.desktop, |s| s.last_request_id == 900);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.control.release();
        let _ = self.desktop.shutdown(Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&self.dir);
        if let Some(state) = &self.state {
            let _ = std::fs::remove_dir_all(state);
        }
    }
}
#[track_caller]
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = desktop_snapshot::read(desktop);
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "desktop never reached the expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn file_surface(
    snapshot: &Snapshot,
    surface: SurfaceId,
) -> Option<zmux_workspace_core::FileSurface> {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .find(|s| s.id == surface)
        .and_then(|s| s.file.clone())
}
fn selected_surface(snapshot: &Snapshot) -> SurfaceId {
    let workspace = &snapshot.model.workspaces[0];
    workspace
        .root
        .panes()
        .into_iter()
        .find(|p| p.id == workspace.focused_pane)
        .unwrap()
        .selected_surface
}
#[track_caller]
fn failure<T: std::fmt::Debug>(result: Result<T, FileFailure>) -> FileFailure {
    result.expect_err("expected a file failure")
}

#[test]
fn attach_reload_and_save_increment_generations_and_hash_the_disk_bytes() {
    let fx = Fixture::new();
    let surface = fx.open("note.txt", b"hello\r\nworld\n");
    let (owner, content) = fx.attach(surface);
    assert_eq!(
        content.body, b"hello\r\nworld\n",
        "bytes in, bytes out: no newline conversion"
    );
    assert_eq!(content.mode, FileMode::Text);
    assert_eq!(content.generation, 1);
    assert_eq!(content.sha256, sha256_hex(b"hello\r\nworld\n"));
    assert!(content.writable && !content.bom);
    let entry = fx.wait_entry(surface, |e| e["attached"] == true);
    assert_eq!(entry["state"], "in_sync");
    assert_eq!(entry["last_op"]["kind"], "attach");
    assert_eq!(entry["last_op"]["state"], "completed");

    let saved = fx
        .desktop
        .file_save(&owner, content.generation, false, b"changed\n".to_vec())
        .unwrap();
    assert_eq!(saved.generation, 2);
    assert_eq!(saved.sha256, sha256_hex(b"changed\n"));
    assert_eq!(
        std::fs::read(fx.dir.join("note.txt")).unwrap(),
        b"changed\n"
    );

    let reloaded = fx.desktop.file_reload(&owner, false).unwrap();
    assert_eq!(reloaded.generation, 3);
    assert_eq!(reloaded.body, b"changed\n");
    let entry = fx.wait_entry(surface, |e| e["generation"] == 3);
    assert_eq!(entry["last_op"]["kind"], "reload");
    assert_eq!(entry["disk_generation"], 3);
}

#[test]
fn binary_oversize_and_missing_files_never_create_tabs() {
    let fx = Fixture::new();
    let mut late_nul = vec![b'a'; 8193];
    late_nul.push(0);
    let mut late_bad = vec![b'a'; 20_000];
    late_bad.push(0xFF);
    let before = fx.snapshot().model.clone();
    let mut paths = Vec::new();
    for (name, bytes) in [
        ("late-nul.txt", late_nul),
        ("late-bad.txt", late_bad),
        ("over.txt", vec![b'a'; 2 * 1024 * 1024 + 1]),
        ("fake.png", b"not a png".to_vec()),
    ] {
        paths.push(fx.write(name, &bytes).to_str().unwrap().to_owned());
    }
    paths.push(fx.dir.join("missing.txt").to_str().unwrap().to_owned());
    paths.push(fx.dir.to_str().unwrap().to_owned());
    paths.push("relative/path.txt".into());
    let result = fx.desktop.file_open(paths, None, true).unwrap();
    let codes: Vec<_> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["ok"].as_bool().unwrap(),
                i["error"]["code"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            (false, "binary_file".into()),
            (false, "invalid_utf8".into()),
            (false, "too_large".into()),
            (false, "content_mismatch".into()),
            (false, "not_found".into()),
            (false, "is_directory".into()),
            (false, "invalid_operand".into()),
        ]
    );
    assert_eq!(result["ok"], false);
    assert_eq!(
        fx.snapshot().model.workspaces,
        before.workspaces,
        "no tab was created"
    );
}

#[test]
fn mixed_batch_applies_items_in_order_dedupes_and_focuses_only_the_first_success() {
    let fx = Fixture::new();
    let terminal = selected_surface(&fx.snapshot());
    let png = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"x"].concat();
    let md = fx.write("README.md", b"# title\n");
    let rs = fx.write("main.rs", b"fn main() {}\n");
    let image = fx.write("pic.png", &png);
    let pdf = fx.write("doc.pdf", b"%PDF-1.7\n");
    let missing = fx.dir.join("missing.txt");
    let target = fx.write("target.txt", b"t");
    let link = fx.dir.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let operands: Vec<String> = [&md, &missing, &rs, &image, &pdf, &link]
        .iter()
        .map(|p| p.to_str().unwrap().to_owned())
        .collect();
    let result = fx.desktop.file_open(operands, None, true).unwrap();
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    assert_eq!(
        items
            .iter()
            .map(|i| i["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(items[0]["mode"], "markdown");
    assert_eq!(items[1]["error"]["code"], "not_found");
    assert_eq!(items[2]["mode"], "text");
    assert_eq!(items[3]["mode"], "image");
    assert_eq!(items[4]["mode"], "pdf");
    assert_eq!(
        items[5]["path"],
        target.to_str().unwrap(),
        "symlink opens its canonical target"
    );
    assert_eq!(result["ok"], false);
    let snapshot = fx.snapshot();
    let first = SurfaceId(items[0]["surface"]["surface_id"].as_u64().unwrap());
    assert_eq!(
        selected_surface(&snapshot),
        first,
        "only the first success takes focus"
    );
    // Tabs were appended in operand order after the terminal.
    let pane = snapshot.model.workspaces[0].root.panes()[0];
    let order: Vec<_> = pane
        .surfaces
        .iter()
        .filter_map(|s| s.file.as_ref())
        .map(|f| f.path.clone())
        .collect();
    assert_eq!(
        order,
        [
            md.to_str().unwrap(),
            rs.to_str().unwrap(),
            image.to_str().unwrap(),
            pdf.to_str().unwrap(),
            target.to_str().unwrap()
        ]
    );
    assert!(pane.surfaces.iter().any(|s| s.id == terminal));

    // Reopening dedupes and re-focuses; focus=false keeps the current selection.
    let again = fx
        .desktop
        .file_open(vec![rs.to_str().unwrap().into()], None, true)
        .unwrap();
    assert_eq!(again["items"][0]["deduplicated"], true);
    assert_eq!(again["ok"], true);
    assert_eq!(
        selected_surface(&fx.snapshot()).0,
        again["items"][0]["surface"]["surface_id"].as_u64().unwrap()
    );
    let kept = selected_surface(&fx.snapshot());
    let unfocused = fx
        .desktop
        .file_open(vec![md.to_str().unwrap().into()], None, false)
        .unwrap();
    assert_eq!(unfocused["items"][0]["deduplicated"], true);
    assert_eq!(selected_surface(&fx.snapshot()), kept);
}

#[test]
fn forced_markdown_mode_rejects_images_and_accepts_extensionless_text() {
    let fx = Fixture::new();
    let readme = fx.write("README", b"plain\n");
    let image = fx.write(
        "a.png",
        &[&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"x"].concat(),
    );
    let result = fx
        .desktop
        .file_open(
            vec![
                readme.to_str().unwrap().into(),
                image.to_str().unwrap().into(),
            ],
            Some(FileMode::Markdown),
            true,
        )
        .unwrap();
    assert_eq!(result["items"][0]["mode"], "markdown");
    assert_eq!(result["items"][1]["error"]["code"], "content_mismatch");
}

#[test]
fn seventeenth_file_is_a_file_capacity_item_error() {
    let fx = Fixture::new();
    for index in 0..MAX_FILE_SURFACES {
        fx.open(&format!("f{index}.txt"), b"x");
    }
    let path = fx.write("one-too-many.txt", b"x");
    let item = fx.open_item(&path);
    assert_eq!(item["ok"], false);
    assert_eq!(item["error"]["code"], "file_capacity");
    let files = fx
        .snapshot()
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .filter(|s| s.file.is_some())
        .count();
    assert_eq!(files, MAX_FILE_SURFACES);
}

#[test]
fn conflict_modified_and_deleted_require_explicit_force() {
    let fx = Fixture::new();
    let surface = fx.open("c.txt", b"base");
    let path = fx.dir.join("c.txt");
    let (owner, content) = fx.attach(surface);
    std::fs::write(&path, b"theirs").unwrap();
    let conflict =
        failure(
            fx.desktop
                .file_save(&owner, content.generation, false, b"mine".to_vec()),
        );
    assert_eq!(conflict.code, "conflict");
    assert!(!conflict.outcome_unknown);
    let detail = conflict.conflict.unwrap();
    assert_eq!(detail.kind, "modified");
    assert_eq!(
        detail.disk_sha256.as_deref(),
        Some(sha256_hex(b"theirs").as_str())
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"theirs");
    fx.wait_entry(surface, |e| {
        e["last_op"]["state"] == "failed" && e["last_op"]["code"] == "conflict"
    });

    let forced = fx
        .desktop
        .file_save(&owner, content.generation, true, b"mine".to_vec())
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"mine");

    std::fs::remove_file(&path).unwrap();
    let deleted =
        failure(
            fx.desktop
                .file_save(&owner, forced.generation, false, b"again".to_vec()),
        );
    assert_eq!(deleted.conflict.unwrap().kind, "deleted");
    assert!(!path.exists());
    fx.desktop
        .file_save(&owner, forced.generation, true, b"again".to_vec())
        .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"again",
        "force recreates a deleted file"
    );
}

#[test]
fn touch_and_identical_rewrite_are_not_conflicts() {
    let fx = Fixture::new();
    let surface = fx.open("t.txt", b"same");
    let path = fx.dir.join("t.txt");
    let (owner, content) = fx.attach(surface);
    std::fs::write(&path, b"same").unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(90))
        .unwrap();
    let saved = fx
        .desktop
        .file_save(&owner, content.generation, false, b"updated".to_vec())
        .unwrap();
    assert_eq!(saved.generation, 2);
    assert_eq!(std::fs::read(&path).unwrap(), b"updated");
}

#[test]
fn stale_view_save_writes_nothing() {
    let fx = Fixture::new();
    let surface = fx.open("s.txt", b"one");
    let (owner, first) = fx.attach(surface);
    std::fs::write(fx.dir.join("s.txt"), b"two").unwrap();
    let reloaded = fx.desktop.file_reload(&owner, false).unwrap();
    assert_eq!(reloaded.generation, first.generation + 1);
    // The view is still showing generation 1 text.
    let stale =
        failure(
            fx.desktop
                .file_save(&owner, first.generation, false, b"overwrite".to_vec()),
        );
    assert_eq!(stale.code, "stale_view");
    assert_eq!(std::fs::read(fx.dir.join("s.txt")).unwrap(), b"two");
    assert_eq!(fx.control.count("rename"), 0);
}

#[test]
fn owners_are_fenced_by_host_surface_lifetime_and_epoch() {
    let fx = Fixture::new();
    let surface = fx.open("o.txt", b"owned");
    let host = Uuid::new_v4();
    let content = fx.attach_as(surface, host).unwrap();
    let owner = content.owner.clone();
    // A second host cannot take over a bound surface; the same host may re-attach.
    assert_eq!(
        failure(fx.attach_as(surface, Uuid::new_v4())).code,
        "host_conflict"
    );
    assert!(fx.attach_as(surface, host).is_ok());
    // Forged owners are rejected before any I/O.
    let reads = fx.control.count("read");
    for forged in [
        FileOwner {
            host_id: Uuid::new_v4(),
            ..owner.clone()
        },
        FileOwner {
            epoch: Uuid::new_v4(),
            ..owner.clone()
        },
        FileOwner {
            identity_namespace: Uuid::new_v4(),
            ..owner.clone()
        },
    ] {
        assert_eq!(
            failure(fx.desktop.file_reload(&forged, false)).code,
            "stale_owner"
        );
        assert_eq!(
            failure(fx.desktop.file_save(&forged, 1, true, b"x".to_vec())).code,
            "stale_owner"
        );
    }
    assert_eq!(
        failure(fx.desktop.file_reload(
            &FileOwner {
                surface_id: SurfaceId(99_999),
                ..owner.clone()
            },
            false
        ))
        .code,
        "unknown_surface"
    );
    assert_eq!(fx.control.count("read"), reads);

    // Detach releases the binding and fences the old owner.
    fx.desktop.file_detach(&owner).unwrap();
    assert_eq!(
        failure(fx.desktop.file_save(&owner, 2, true, b"late".to_vec())).code,
        "stale_owner"
    );
    assert_eq!(std::fs::read(fx.dir.join("o.txt")).unwrap(), b"owned");
    let fresh = fx.attach_as(surface, Uuid::new_v4()).unwrap();
    assert!(
        fresh.generation > content.generation,
        "generations never move backwards"
    );

    // Closing the surface retires the owner: late calls are stale, not unknown.
    fx.close(surface);
    fx.wait_entry_gone(surface);
    assert_eq!(
        failure(
            fx.desktop
                .file_save(&fresh.owner, fresh.generation, true, b"late".to_vec())
        )
        .code,
        "stale_owner"
    );
    assert_eq!(
        failure(fx.desktop.file_reload(&fresh.owner, false)).code,
        "stale_owner"
    );
    assert_eq!(std::fs::read(fx.dir.join("o.txt")).unwrap(), b"owned");
    assert_eq!(
        failure(fx.desktop.file_detach(&fresh.owner)).code,
        "stale_owner"
    );
}
impl Fixture {
    #[track_caller]
    fn wait_entry_gone(&self, surface: SurfaceId) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.entry(surface).is_some() {
            assert!(Instant::now() < deadline, "entry was never retired");
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn a_save_queued_behind_a_closed_surface_never_writes() {
    let fx = Fixture::new();
    let surface = fx.open("q.txt", b"original");
    let (owner, content) = fx.attach(surface);
    fx.control.arm("sync_file");
    let desktop = &fx.desktop;
    thread::scope(|scope| {
        let save = scope
            .spawn(|| desktop.file_save(&owner, content.generation, false, b"late write".to_vec()));
        fx.control.wait_blocked(1);
        // The save holds a durable temporary file; now its view goes away.
        fx.close(surface);
        fx.wait_entry_gone(surface);
        fx.control.release();
        let result = save.join().unwrap();
        assert_eq!(failure(result).code, "stale_owner");
    });
    assert_eq!(std::fs::read(fx.dir.join("q.txt")).unwrap(), b"original");
    assert_eq!(fx.control.count("rename"), 0);
    let leftovers: Vec<_> = std::fs::read_dir(&fx.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".zmux-save-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_reload_that_outlives_a_detach_never_rebinds_the_old_view() {
    let fx = Fixture::new();
    let surface = fx.open("r.txt", b"reload");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        let reload = scope.spawn(|| fx.desktop.file_reload(&owner, false));
        fx.control.wait_blocked(1);
        fx.desktop.file_detach(&owner).unwrap();
        fx.control.release();
        assert_eq!(failure(reload.join().unwrap()).code, "stale_owner");
    });
    let entry = fx.wait_entry(surface, |e| e["last_op"]["state"] != "pending");
    assert_eq!(entry["attached"], false, "{entry}");
    assert_eq!(entry["last_op"]["state"], "failed");
    assert_eq!(entry["last_op"]["code"], "stale_owner");
    // A new view can attach and the old owner stays fenced.
    assert!(fx.attach_as(surface, Uuid::new_v4()).is_ok());
    assert_eq!(
        failure(fx.desktop.file_reload(&owner, false)).code,
        "stale_owner"
    );
}

#[test]
fn a_timed_out_reload_finishing_after_detach_does_not_rebind_the_old_view() {
    let fx = Fixture::with(|c| c.read_timeout = Duration::from_millis(100));
    let surface = fx.open("t.txt", b"timeout");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    assert_eq!(
        failure(fx.desktop.file_reload(&owner, false)).code,
        "timeout"
    );
    fx.desktop.file_detach(&owner).unwrap();
    fx.control.release();
    let entry = fx.wait_entry(surface, |e| e["last_op"]["state"] != "pending");
    assert_eq!(entry["attached"], false, "{entry}");
    assert!(fx.attach_as(surface, Uuid::new_v4()).is_ok());
}

#[test]
fn a_save_queued_behind_work_then_detached_releases_the_tab_and_never_writes() {
    let fx = Fixture::new();
    let busy = fx.open("busy.txt", b"busy");
    let (busy_owner, _) = fx.attach(busy);
    let surface = fx.open("s.txt", b"original");
    let (owner, content) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        // The worker is parked inside another tab's reload while the save waits its turn.
        let blocker = scope.spawn(|| fx.desktop.file_reload(&busy_owner, false));
        fx.control.wait_blocked(1);
        let save = scope.spawn(|| {
            fx.desktop
                .file_save(&owner, content.generation, false, b"late".to_vec())
        });
        fx.wait_entry(surface, |e| e["last_op"]["state"] == "pending");
        fx.desktop.file_detach(&owner).unwrap();
        fx.control.release();
        assert_eq!(failure(save.join().unwrap()).code, "stale_owner");
        assert!(blocker.join().unwrap().is_ok());
    });
    assert_eq!(std::fs::read(fx.dir.join("s.txt")).unwrap(), b"original");
    assert_eq!(fx.control.count("rename"), 0);
    let entry = fx.wait_entry(surface, |e| e["last_op"]["state"] != "pending");
    assert_eq!(entry["last_op"]["code"], "stale_owner");
    // The tab is not wedged: a fresh attach is not `busy`.
    assert!(fx.attach_as(surface, Uuid::new_v4()).is_ok());
}

#[test]
fn a_preserved_bom_counts_against_the_save_cap_end_to_end() {
    use zmux_desktop_runtime::file_io::MAX_TEXT_BYTES;
    let fx = Fixture::new();
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(vec![b'a'; MAX_TEXT_BYTES - 3]);
    let surface = fx.open("bom.txt", &bytes);
    let (owner, content) = fx.attach(surface);
    assert!(content.bom);
    let refused = failure(fx.desktop.file_save(
        &owner,
        content.generation,
        false,
        vec![b'b'; MAX_TEXT_BYTES],
    ));
    assert_eq!(refused.code, "too_large");
    assert_eq!(std::fs::read(fx.dir.join("bom.txt")).unwrap(), bytes);
    // The largest fitting save succeeds, and the tab reads and saves it again without a conflict.
    let receipt = fx
        .desktop
        .file_save(
            &owner,
            content.generation,
            false,
            vec![b'b'; MAX_TEXT_BYTES - 3],
        )
        .unwrap();
    let reloaded = fx.desktop.file_reload(&owner, false).unwrap();
    assert_eq!(reloaded.size as usize, MAX_TEXT_BYTES);
    assert_eq!(reloaded.sha256, receipt.sha256);
    assert!(
        fx.desktop
            .file_save(&owner, reloaded.generation, false, b"small".to_vec())
            .is_ok()
    );
}

#[test]
fn a_second_operation_on_the_same_surface_is_busy() {
    let fx = Fixture::new();
    let surface = fx.open("b.txt", b"busy");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        let first = scope.spawn(|| fx.desktop.file_reload(&owner, false));
        fx.control.wait_blocked(1);
        assert_eq!(failure(fx.desktop.file_reload(&owner, false)).code, "busy");
        assert_eq!(
            failure(fx.desktop.file_save(&owner, 1, true, b"x".to_vec())).code,
            "busy"
        );
        fx.control.release();
        assert!(first.join().unwrap().is_ok());
    });
}

#[test]
fn the_request_queue_is_bounded_and_overload_is_explicit() {
    let fx = Fixture::new();
    let mut owners = Vec::new();
    for index in 0..10 {
        let surface = fx.open(&format!("n{index}.txt"), b"queued");
        owners.push(fx.attach(surface).0);
    }
    fx.control.arm("read");
    thread::scope(|scope| {
        // The first reload must be dequeued and blocked in the gate before anything else is
        // submitted; otherwise it still occupies a queue slot and the ninth submission can be
        // refused as `busy`. After that one job runs (and blocks) and eight more fill the queue.
        let first = scope.spawn(|| fx.desktop.file_reload(&owners[0], false));
        fx.control.wait_blocked(1);
        let mut handles = vec![first];
        handles.extend(
            owners[1..9]
                .iter()
                .map(|owner| scope.spawn(|| fx.desktop.file_reload(owner, false))),
        );
        for owner in &owners[..9] {
            fx.wait_entry(owner.surface_id, |e| e["last_op"]["state"] == "pending");
        }
        let overload = failure(fx.desktop.file_reload(&owners[9], false));
        assert_eq!(overload.code, "busy");
        assert!(!overload.outcome_unknown, "nothing ran");
        // The refused surface is free again; nothing is left pending for it.
        let entry = fx.entry(owners[9].surface_id).unwrap();
        assert_ne!(entry["last_op"]["state"], "pending");
        fx.control.release();
        for handle in handles {
            assert!(handle.join().unwrap().is_ok());
        }
    });
    assert!(fx.desktop.file_reload(&owners[9], false).is_ok());
}

#[test]
fn a_stalled_worker_fails_fast_then_recovers() {
    let fx = Fixture::with(|c| c.stall_after = Duration::from_millis(100));
    let surface = fx.open("stall.txt", b"slow volume");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        let hung = scope.spawn(|| fx.desktop.file_reload(&owner, false));
        fx.control.wait_blocked(1);
        let snapshot = wait(&fx.desktop, |s| {
            serde_json::to_value(&s.files).unwrap()["worker"]["state"] == "stalled"
        });
        assert!(
            serde_json::to_value(&snapshot.files).unwrap()["worker"]["stalled_ms"]
                .as_u64()
                .unwrap()
                <= 5000
        );
        let other = fx.open_item(&fx.write("other.txt", b"o"));
        assert_eq!(other["error"]["code"], "file_worker_unavailable");
        assert_eq!(
            failure(fx.desktop.file_reload(&owner, false)).code,
            "file_worker_unavailable"
        );
        fx.control.release();
        assert!(hung.join().unwrap().is_ok());
    });
    wait(&fx.desktop, |s| {
        serde_json::to_value(&s.files).unwrap()["worker"]["state"] == "ok"
    });
    assert_eq!(fx.open_item(&fx.write("after.txt", b"ok"))["ok"], true);
}

#[test]
fn a_save_that_times_out_is_outcome_unknown_and_is_never_replayed() {
    let fx = Fixture::with(|c| c.save_timeout = Duration::from_millis(150));
    let surface = fx.open("u.txt", b"before");
    let (owner, content) = fx.attach(surface);
    fx.control.arm("rename");
    let timed_out =
        failure(
            fx.desktop
                .file_save(&owner, content.generation, false, b"after".to_vec()),
        );
    assert_eq!(timed_out.code, "timeout");
    assert!(timed_out.outcome_unknown);
    let op_id = timed_out
        .op_id
        .expect("the caller can correlate the unknown outcome");
    let entry = fx.wait_entry(surface, |e| e["last_op"]["state"] == "outcome_unknown");
    assert_eq!(entry["last_op"]["op_id"], op_id);
    assert_eq!(entry["last_op"]["code"], "timeout");
    assert_eq!(std::fs::read(fx.dir.join("u.txt")).unwrap(), b"before");

    // The worker still finishes the one save it owns; nobody submits it again.
    fx.control.release();
    let entry = fx.wait_entry(surface, |e| e["last_op"]["state"] == "completed");
    assert_eq!(entry["last_op"]["op_id"], op_id);
    assert_eq!(entry["generation"], content.generation + 1);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        fx.control.count("rename"),
        1,
        "exactly one rename ever happened"
    );
    assert_eq!(std::fs::read(fx.dir.join("u.txt")).unwrap(), b"after");
    // The view learned the outcome from the snapshot, so its next save is stale until it reloads.
    let stale = failure(
        fx.desktop
            .file_save(&owner, content.generation, false, b"again".to_vec()),
    );
    assert_eq!(stale.code, "stale_view");
}

#[test]
fn reload_is_refused_while_the_model_says_dirty_unless_discarding() {
    let fx = Fixture::new();
    let surface = fx.open("d.txt", b"disk");
    let (owner, _) = fx.attach(surface);
    fx.desktop.file_observe(&owner, true).unwrap();
    assert_eq!(failure(fx.desktop.file_reload(&owner, false)).code, "dirty");
    let reloaded = fx.desktop.file_reload(&owner, true).unwrap();
    assert_eq!(reloaded.body, b"disk");
}

#[test]
fn attaching_again_as_the_bound_host_obeys_the_dirty_rule_and_never_loses_an_update() {
    let fx = Fixture::new();
    let surface = fx.open("again.txt", b"disk");
    let host = Uuid::new_v4();
    let content = fx.attach_as(surface, host).unwrap();
    let owner = content.owner.clone();
    fx.desktop.file_observe(&owner, true).unwrap();
    std::fs::write(fx.dir.join("again.txt"), b"external edit").unwrap();
    let before = fx.entry(surface).unwrap();
    let reads = fx.control.count("read");

    // A re-attach is the same operation as a reload without `discard`: refused while dirty,
    // before any read, and nothing is re-baselined.
    assert_eq!(failure(fx.attach_as(surface, host)).code, "dirty");
    assert_eq!(
        fx.control.count("read"),
        reads,
        "a refused attach reads nothing"
    );
    assert_eq!(
        fx.entry(surface).unwrap()["generation"],
        before["generation"]
    );
    assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);

    // The old generation still fences the save: the external edit is reported, not overwritten.
    let conflict =
        failure(
            fx.desktop
                .file_save(&owner, content.generation, false, b"mine".to_vec()),
        );
    assert_eq!(conflict.code, "conflict");
    assert_eq!(
        std::fs::read(fx.dir.join("again.txt")).unwrap(),
        b"external edit"
    );

    // Dropping the unsaved text is explicit, and then a re-attach is allowed again.
    fx.desktop.file_reload(&owner, true).unwrap();
    fx.desktop.file_observe(&owner, false).unwrap();
    let again = fx.attach_as(surface, host).unwrap();
    assert_eq!(again.body, b"external edit");
}

#[test]
fn a_session_restore_rotates_the_epoch_and_fences_every_earlier_view() {
    use desktop_runtime::sessions::{OperationState, SessionCommand};
    let fx = Fixture::persistent();
    let surface = fx.open("restored.txt", b"on disk");
    let host = Uuid::new_v4();
    let content = fx.attach_as(surface, host).unwrap();
    let old = content.owner.clone();
    let old_epoch = fx.epoch();
    assert_eq!(old.epoch, old_epoch);

    let run = |id: u64, command: SessionCommand| {
        fx.desktop
            .submit(Request {
                version: 1,
                request_id: id,
                payload: Command::Session { action: command },
            })
            .unwrap();
        let accepted = wait(&fx.desktop, |s| s.last_request_id == id);
        assert!(accepted.last_error.is_none(), "{:?}", accepted.last_error);
        let operation = accepted.persistence.operations.last().unwrap().operation_id;
        let finished = wait(&fx.desktop, |s| {
            s.persistence.operations.iter().any(|op| {
                op.operation_id == operation && !matches!(op.state, OperationState::Pending)
            })
        });
        let receipt = finished
            .persistence
            .operations
            .iter()
            .find(|op| op.operation_id == operation)
            .unwrap();
        assert!(
            matches!(receipt.state, OperationState::Completed),
            "{:?}",
            receipt.error
        );
        finished
    };
    let export = fx.state.as_ref().unwrap().join("checkpoint.json");
    let exported = run(
        1,
        SessionCommand::Export {
            path: export.clone(),
            force: false,
        },
    );
    for (index, workspace) in exported.model.workspaces.iter().enumerate() {
        fx.desktop
            .submit(Request {
                version: 1,
                request_id: 10 + index as u64,
                payload: Command::Action {
                    action: Action::CloseWorkspace {
                        workspace_id: workspace.id,
                    },
                },
            })
            .unwrap();
        wait(&fx.desktop, |s| s.last_request_id == 10 + index as u64);
    }
    let restored = run(2, SessionCommand::Restore { path: Some(export) });
    assert!(
        file_surface(&restored, surface).is_some(),
        "the file tab is part of the restored session"
    );
    assert_ne!(
        restored.files.epoch, old_epoch,
        "restore must rotate the file epoch"
    );

    // Every handle from before the restore is stale, whatever its host or surface looks like.
    assert_eq!(
        failure(fx.desktop.file_attach(old_epoch, surface, host)).code,
        "stale_owner"
    );
    assert_eq!(
        failure(fx.desktop.file_reload(&old, true)).code,
        "stale_owner"
    );
    assert_eq!(
        failure(
            fx.desktop
                .file_save(&old, content.generation, true, b"late".to_vec())
        )
        .code,
        "stale_owner"
    );
    assert_eq!(
        failure(fx.desktop.file_observe(&old, true)).code,
        "stale_owner"
    );
    assert_eq!(failure(fx.desktop.file_detach(&old)).code, "stale_owner");
    assert_eq!(
        std::fs::read(fx.dir.join("restored.txt")).unwrap(),
        b"on disk"
    );
    assert!(!file_surface(&fx.snapshot(), surface).unwrap().dirty);

    // Views re-attach under the new epoch and work normally.
    let fresh = fx
        .desktop
        .file_attach(restored.files.epoch, surface, Uuid::new_v4())
        .unwrap();
    assert_eq!(fresh.body, b"on disk");
    assert_eq!(fresh.owner.epoch, restored.files.epoch);
}

#[test]
fn the_registry_is_never_older_than_a_published_snapshot() {
    let fx = Fixture::new();
    for round in 0..30 {
        let name = format!("loop-{round}.txt");
        let path = fx.write(&name, b"body");
        let item = fx.open_item(&path);
        assert_eq!(item["ok"], true, "{item}");
        let surface = SurfaceId(item["surface"]["surface_id"].as_u64().unwrap());
        // The open acknowledgement says the tab exists, so the registry must know it already.
        let content = fx
            .attach_as(surface, Uuid::new_v4())
            .unwrap_or_else(|failure| panic!("round {round}: attach right after open: {failure}"));
        fx.close(surface);
        // The close is published, so the owner is stale at once: never `unknown_surface`, never
        // an operation against a surface the published model no longer has.
        assert!(file_surface(&fx.snapshot(), surface).is_none());
        for code in [
            failure(fx.desktop.file_reload(&content.owner, true)).code,
            failure(fx.desktop.file_observe(&content.owner, true)).code,
            failure(
                fx.desktop
                    .file_save(&content.owner, content.generation, true, b"x".to_vec()),
            )
            .code,
        ] {
            assert_eq!(code, "stale_owner", "round {round}");
        }
        assert!(fx.entry(surface).is_none(), "round {round}");
        assert_eq!(std::fs::read(&path).unwrap(), b"body");
    }
}

#[test]
fn dirty_is_set_only_by_the_trusted_host_event_and_cleared_on_detach() {
    let fx = Fixture::new();
    let surface = fx.open("host.txt", b"x");
    let (owner, _) = fx.attach(surface);
    assert!(!file_surface(&fx.snapshot(), surface).unwrap().dirty);
    fx.desktop.file_observe(&owner, true).unwrap();
    assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);
    // The public action path never accepts a dirty observation.
    fx.desktop
        .submit(Request {
            version: 1,
            request_id: 901,
            payload: Command::Action {
                action: Action::ObserveFile {
                    surface_id: surface,
                    dirty: false,
                },
            },
        })
        .unwrap();
    let rejected = wait(&fx.desktop, |s| s.last_request_id == 901);
    assert!(
        rejected
            .last_error
            .as_deref()
            .unwrap()
            .contains("trusted file opener")
    );
    assert!(file_surface(&rejected, surface).unwrap().dirty);
    // Detach clears the bit: the view that held the unsaved text is gone.
    fx.desktop.file_detach(&owner).unwrap();
    assert!(!file_surface(&fx.snapshot(), surface).unwrap().dirty);
    // Image and PDF surfaces never hold edits.
    let image = fx.open(
        "i.png",
        &[&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"x"].concat(),
    );
    let (image_owner, content) = fx.attach(image);
    assert!(!content.writable);
    assert!(fx.desktop.file_observe(&image_owner, true).is_err());
    assert_eq!(
        failure(fx.desktop.file_save(&image_owner, 1, true, b"x".to_vec())).code,
        "invalid_request"
    );
}

#[test]
fn retried_observe_after_stale_owner_is_not_applied() {
    let fx = Fixture::new();
    let surface = fx.open("stale.txt", b"x");
    let (old, _) = fx.attach(surface);
    fx.desktop.file_observe(&old, true).unwrap();
    assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);

    // Detach clears the bit; the old view's retried observe must not set it again.
    fx.desktop.file_detach(&old).unwrap();
    assert!(!file_surface(&fx.snapshot(), surface).unwrap().dirty);
    let revision = fx.snapshot().revision;
    for _ in 0..3 {
        assert_eq!(
            failure(fx.desktop.file_observe(&old, true)).code,
            "stale_owner"
        );
    }
    assert!(!file_surface(&fx.snapshot(), surface).unwrap().dirty);
    assert_eq!(
        fx.snapshot().revision,
        revision,
        "refused retries change nothing"
    );

    // A different view's bit is neither set nor cleared by the old owner's retries.
    let (fresh, _) = fx.attach(surface);
    fx.desktop.file_observe(&fresh, true).unwrap();
    assert_eq!(
        failure(fx.desktop.file_observe(&old, false)).code,
        "stale_owner"
    );
    assert!(
        file_surface(&fx.snapshot(), surface).unwrap().dirty,
        "a stale observe(false) must not clear another view's dirty bit"
    );

    // Epoch and namespace forgeries are refused the same way.
    for forged in [
        FileOwner {
            epoch: Uuid::new_v4(),
            ..fresh.clone()
        },
        FileOwner {
            identity_namespace: Uuid::new_v4(),
            ..fresh.clone()
        },
    ] {
        assert_eq!(
            failure(fx.desktop.file_observe(&forged, false)).code,
            "stale_owner"
        );
    }
    assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);

    // After the tab is closed every retry is stale, not unknown, and nothing reappears.
    fx.desktop.file_observe(&fresh, false).unwrap();
    fx.close(surface);
    fx.wait_entry_gone(surface);
    assert_eq!(
        failure(fx.desktop.file_observe(&fresh, true)).code,
        "stale_owner"
    );
    assert!(file_surface(&fx.snapshot(), surface).is_none());
}

#[test]
fn observe_is_idempotent_and_does_not_advance_the_model_revision_when_unchanged() {
    let fx = Fixture::new();
    let surface = fx.open("idem.txt", b"x");
    let (owner, _) = fx.attach(surface);
    fx.desktop.file_observe(&owner, true).unwrap();
    let revision = fx.snapshot().model.revision;
    fx.desktop.file_observe(&owner, true).unwrap();
    fx.desktop.file_observe(&owner, true).unwrap();
    assert_eq!(fx.snapshot().model.revision, revision);
    fx.desktop.file_observe(&owner, false).unwrap();
    assert_eq!(fx.snapshot().model.revision, revision + 1);
}

#[test]
fn a_refused_observe_converges_once_the_queue_drains_and_the_close_guard_then_holds() {
    let hold = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let hook_hold = hold.clone();
    let fx = Fixture::with(|c| {
        c.executor_tick_hook = Some(Arc::new(move || {
            let (flag, changed) = &*hook_hold;
            let mut state = flag.lock().unwrap();
            while state.0 {
                state.1 = true;
                state = changed.wait(state).unwrap();
            }
            state.1 = false;
        }));
    });
    // Always release the executor, even if an assertion below fails.
    struct Unhold(Arc<(Mutex<(bool, bool)>, Condvar)>);
    impl Drop for Unhold {
        fn drop(&mut self) {
            self.0.0.lock().unwrap().0 = false;
            self.0.1.notify_all();
        }
    }
    let _unhold = Unhold(hold.clone());
    let surface = fx.open("conv.txt", b"x");
    let (owner, _) = fx.attach(surface);
    let model_before = fx.snapshot().model.clone();

    hold.0.lock().unwrap().0 = true;
    // Do not flood until the executor is provably parked inside the hook.
    let parked = Instant::now() + Duration::from_secs(5);
    while !hold.0.lock().unwrap().1 {
        assert!(
            Instant::now() < parked,
            "the executor never reached the hook"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let mut sent = 0;
    let mut id = 2000;
    loop {
        id += 1;
        let request = Request {
            version: 1,
            request_id: id,
            payload: Command::Attention {
                action: AttentionCommand::AppActive { active: true },
            },
        };
        match fx.desktop.submit(request) {
            Ok(()) => sent += 1,
            Err(desktop_runtime::DesktopError::QueueFull) => break,
            Err(error) => panic!("{error}"),
        }
        assert!(sent < 200, "the command queue is not bounded");
    }
    let refused = failure(fx.desktop.file_observe(&owner, true));
    assert_eq!(refused.code, "busy");
    assert_eq!(
        fx.snapshot().model,
        model_before,
        "a refused observe changes nothing"
    );

    hold.0.lock().unwrap().0 = false;
    hold.1.notify_all();
    wait(&fx.desktop, |s| s.last_request_id == id - 1);
    fx.desktop.file_observe(&owner, true).unwrap();
    assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);

    fx.desktop
        .submit(Request {
            version: 1,
            request_id: 950,
            payload: Command::Action {
                action: Action::CloseSurface {
                    surface_id: surface,
                },
            },
        })
        .unwrap();
    let refused_close = wait(&fx.desktop, |s| s.last_request_id == 950);
    assert!(
        refused_close
            .last_error
            .as_deref()
            .unwrap()
            .contains("unsaved changes"),
        "{:?}",
        refused_close.last_error
    );
    assert!(file_surface(&refused_close, surface).is_some());
    fx.desktop.file_observe(&owner, false).unwrap();
    fx.close(surface);
    assert!(file_surface(&fx.snapshot(), surface).is_none());
}

#[test]
fn polling_reports_changes_deletes_recreation_and_ignores_touch() {
    let fx = Fixture::with(|c| c.poll_interval = Duration::from_millis(20));
    let surface = fx.open("p.txt", b"v1");
    let path = fx.dir.join("p.txt");
    let (owner, _) = fx.attach(surface);
    let mut entry = fx.wait_entry(surface, |e| e["state"] == "in_sync");
    assert_eq!(entry["disk_generation"], 1);

    // A touch changes only the timestamp: still in sync after many polls.
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(30))
        .unwrap();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(fx.entry(surface).unwrap()["state"], "in_sync");

    // An in-place edit is reported once its metadata is stable.
    std::fs::write(&path, b"v2 edited").unwrap();
    entry = fx.wait_entry(surface, |e| e["state"] == "changed");
    assert_eq!(entry["disk_generation"], 2);
    assert_eq!(
        entry["generation"], 1,
        "the baseline the view shows is unchanged"
    );
    // A second external write while changed is visible as a new disk generation.
    std::fs::write(&path, b"v3 edited again").unwrap();
    entry = fx.wait_entry(surface, |e| e["disk_generation"] == 3);
    assert_eq!(entry["state"], "changed");

    // An atomic replace by another editor is also a change; restoring the baseline bytes heals it.
    let replacement = fx.write("replacement.tmp", b"v1");
    std::fs::rename(&replacement, &path).unwrap();
    fx.wait_entry(surface, |e| e["state"] == "in_sync");

    // Deleting needs two consecutive misses; recreating is reported as a change.
    std::fs::remove_file(&path).unwrap();
    fx.wait_entry(surface, |e| e["state"] == "deleted");
    std::fs::write(&path, b"back with new content").unwrap();
    fx.wait_entry(surface, |e| e["state"] == "changed");

    // A directory in place of the file is unreadable, with a code.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let entry = fx.wait_entry(surface, |e| e["state"] == "unreadable");
    assert_eq!(entry["error_code"], "is_directory");
    // A reload while the file is gone reports the error and publishes it.
    assert_eq!(
        failure(fx.desktop.file_reload(&owner, false)).code,
        "is_directory"
    );
}

#[test]
fn file_worker_changes_advance_the_revision_and_an_idle_service_does_not() {
    let fx = Fixture::with(|c| c.poll_interval = Duration::from_millis(20));
    let surface = fx.open("rev.txt", b"v1");
    let path = fx.dir.join("rev.txt");
    let (owner, content) = fx.attach(surface);
    fx.wait_entry(surface, |e| e["state"] == "in_sync");
    thread::sleep(Duration::from_millis(100));
    // Idle: nothing is changing, so nothing is published.
    let idle = fx.snapshot().revision;
    thread::sleep(Duration::from_millis(400));
    assert_eq!(
        fx.snapshot().revision,
        idle,
        "an idle file service must not churn revisions"
    );

    // With no other activity, each worker-side change reaches a new revision.
    let mut last = idle;
    std::fs::write(&path, b"external write").unwrap();
    fx.wait_entry(surface, |e| e["state"] == "changed");
    let after_change = fx.snapshot().revision;
    assert!(after_change > last);
    last = after_change;
    std::fs::remove_file(&path).unwrap();
    fx.wait_entry(surface, |e| e["state"] == "deleted");
    assert!(fx.snapshot().revision > last);
    last = fx.snapshot().revision;
    // A completed save is visible through last_op without any other command.
    fx.desktop
        .file_save(&owner, content.generation, true, b"saved".to_vec())
        .unwrap();
    wait(&fx.desktop, |s| s.revision > last);
    fx.wait_entry(surface, |e| {
        e["last_op"]["kind"] == "save" && e["last_op"]["state"] == "completed"
    });
}

#[test]
fn the_executor_is_not_blocked_while_the_backend_is_gated() {
    let fx = Fixture::new();
    let surface = fx.open("gate.txt", b"x");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        let blocked = scope.spawn(|| fx.desktop.file_reload(&owner, false));
        fx.control.wait_blocked(1);
        // Commands, snapshots and trusted host events still make progress.
        fx.desktop
            .submit(Request {
                version: 1,
                request_id: 777,
                payload: Command::Action {
                    action: Action::CreateWorkspace {
                        name: "while blocked".into(),
                    },
                },
            })
            .unwrap();
        let snapshot = wait(&fx.desktop, |s| s.last_request_id == 777);
        assert!(
            snapshot
                .model
                .workspaces
                .iter()
                .any(|w| w.name == "while blocked")
        );
        fx.desktop.file_observe(&owner, true).unwrap();
        assert!(file_surface(&fx.snapshot(), surface).unwrap().dirty);
        fx.control.release();
        // Dirty now, so the in-flight reload (admitted earlier) still completes.
        assert!(blocked.join().unwrap().is_ok());
    });
}

#[test]
fn probe_timeout_is_an_explicit_item_error_and_other_items_complete() {
    let fx = Fixture::with(|c| c.probe_timeout = Duration::from_millis(150));
    let slow = fx.write("slow.txt", b"slow");
    let fast = fx.write("fast.txt", b"fast");
    fx.control.arm("canonicalize");
    let desktop = &fx.desktop;
    let result = thread::scope(|scope| {
        let call = scope.spawn(|| {
            desktop.file_open(
                vec![slow.to_str().unwrap().into(), fast.to_str().unwrap().into()],
                None,
                true,
            )
        });
        fx.control.wait_blocked(1);
        let result = call.join().unwrap().unwrap();
        fx.control.release();
        result
    });
    assert_eq!(result["items"][0]["error"]["code"], "probe_timeout");
    // The stuck worker cannot serve later items within the same deadline: explicit, not silent.
    assert_eq!(result["items"][1]["error"]["code"], "probe_timeout");
    assert_eq!(result["ok"], false);
    let after = fx.open_item(&fast);
    assert_eq!(after["ok"], true, "the worker recovers once released");
}

#[test]
fn shutdown_waits_for_a_busy_worker_and_joins_it_once_released() {
    let fx = Fixture::new();
    let surface = fx.open("sd.txt", b"x");
    let (owner, _) = fx.attach(surface);
    fx.control.arm("read");
    thread::scope(|scope| {
        let blocked = scope.spawn(|| fx.desktop.file_reload(&owner, false));
        fx.control.wait_blocked(1);
        let report = fx
            .desktop
            .begin_shutdown(Duration::from_millis(300), false)
            .unwrap();
        let pending = fx
            .desktop
            .wait_shutdown(report.ticket, Duration::from_millis(500))
            .unwrap();
        assert!(!pending.physical_cleanup_complete, "{pending:?}");
        assert_eq!(pending.pending.files, 1);
        assert!(pending.deadline_expired);
        fx.control.release();
        let result = blocked.join().unwrap();
        // The reload may complete or be dropped as closed; it is never left hanging.
        assert!(result.is_ok() || failure(result).code == "closed");
        // The root deadline already expired; the ticket keeps observing until the join.
        let give_up = Instant::now() + Duration::from_secs(5);
        let done = loop {
            let status = fx.desktop.shutdown_status(report.ticket).unwrap();
            if status.physical_cleanup_complete || Instant::now() >= give_up {
                break status;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(done.physical_cleanup_complete, "{done:?}");
        assert_eq!(done.pending.files, 0);
    });
    assert_eq!(
        failure(fx.desktop.file_reload(&owner, false)).code,
        "closed"
    );
}

#[test]
fn idle_shutdown_joins_the_file_worker_within_the_deadline() {
    let fx = Fixture::new();
    fx.open("idle.txt", b"x");
    let started = Instant::now();
    fx.desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn file_content_never_enters_the_snapshot_or_published_json() {
    let fx = Fixture::new();
    let sentinel = "SENTINEL-8f3a9c1e-never-published";
    let surface = fx.open("secret.txt", format!("{sentinel}\n").as_bytes());
    let (owner, content) = fx.attach(surface);
    fx.desktop
        .file_save(
            &owner,
            content.generation,
            false,
            format!("{sentinel} edited").into_bytes(),
        )
        .unwrap();
    fx.desktop.file_observe(&owner, true).unwrap();
    let published = serde_json::to_string(&*fx.snapshot()).unwrap();
    assert!(!published.contains(sentinel));
    let item = fx.open_item(&fx.dir.join("secret.txt"));
    assert!(!item.to_string().contains(sentinel));
    // Only the absolute path appears, and only in the model.
    assert!(published.contains(fx.dir.join("secret.txt").to_str().unwrap()));
    assert_eq!(
        json!(published.contains("sha256")),
        json!(false),
        "hashes stay inside the service"
    );
}

// ---------------------------------------------------------------------------------------------
// The control-socket route (design D16): every test here talks to a real `ControlServer`.

/// A real control socket for one fixture. The directory is private and short, as sockets need.
struct Socket {
    _server: ControlServer,
    path: PathBuf,
    dir: PathBuf,
}
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Socket {
    /// One framed request; returns the whole response.
    fn call(&self, command: Value) -> Value {
        use std::io::{Read, Write};
        let mut stream = std::os::unix::net::UnixStream::connect(&self.path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let bytes = serde_json::to_vec(
            &json!({"version": CONTROL_VERSION, "request_id": 7, "command": command}),
        )
        .unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut response = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut response).unwrap();
        serde_json::from_slice(&response).unwrap()
    }
    /// An `open_items` request. `items` are `(kind, value)` pairs.
    fn open(&self, items: &[(&str, &str)], options: Value) -> Value {
        let mut operation = json!({
            "operation": "open_items",
            "items": items.iter().map(|(kind, value)| json!({"kind": kind, "value": value})).collect::<Vec<_>>(),
        });
        for (key, value) in options.as_object().into_iter().flatten() {
            if key != "scope" && key != "expected_model_revision" {
                operation[key] = value.clone();
            }
        }
        self.call(json!({"method": "targeted", "request": {
            "scope": options.get("scope").cloned().unwrap_or(json!({})),
            "caller": {},
            "expected_model_revision": options.get("expected_model_revision").cloned().unwrap_or(Value::Null),
            "operation": operation,
        }}))
    }
}
impl Fixture {
    fn serve(&self, access: ControlAccess) -> Socket {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("zc-{}", &Uuid::new_v4().simple().to_string()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("c.sock");
        let server = ControlServer::start_with_policy(
            &path,
            Arc::clone(&self.desktop),
            ControlPolicy::same_user(access),
        )
        .unwrap();
        Socket {
            _server: server,
            path,
            dir,
        }
    }
    /// Backend calls that touch a user path.
    fn probe_calls(&self) -> usize {
        ["canonicalize", "open_dir", "open_leaf", "read"]
            .iter()
            .map(|op| self.control.count(op))
            .sum()
    }
    fn surfaces_of_kind(&self, files: bool) -> usize {
        self.snapshot()
            .model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .filter(|s| s.file.is_some() == files && (files || s.url.is_some()))
            .count()
    }
}
#[track_caller]
fn result_of(response: &Value) -> &Value {
    assert_eq!(response["ok"], true, "{response}");
    &response["result"]
}
#[track_caller]
fn error_code(response: &Value) -> &str {
    assert_eq!(response["ok"], false, "{response}");
    response["error"]["code"].as_str().unwrap()
}

#[test]
fn a_mixed_batch_over_the_socket_returns_exact_ordered_item_results() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let png = [&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"x"].concat();
    let mut late_nul = vec![b'a'; 8193];
    late_nul.push(0);
    let mut late_bad = vec![b'a'; 20_000];
    late_bad.push(0xFF);
    let paths: Vec<String> = [
        ("README.md", b"# title\n".to_vec()),
        ("main.rs", b"fn main() {}\n".to_vec()),
        ("pic.png", png),
        ("doc.pdf", b"%PDF-1.7\n".to_vec()),
        ("nul.txt", late_nul),
        ("bad.txt", late_bad),
        ("over.txt", vec![b'a'; 2 * 1024 * 1024 + 1]),
    ]
    .into_iter()
    .map(|(name, bytes)| fx.write(name, &bytes).to_str().unwrap().to_owned())
    .collect();
    let missing = fx.dir.join("missing.txt").to_str().unwrap().to_owned();
    let directory = fx.dir.to_str().unwrap().to_owned();
    let before = fx.snapshot().model.revision;
    let items: Vec<(&str, &str)> = vec![
        ("path", &paths[0]),
        ("url", "https://example.org/docs"),
        ("path", &directory),
        ("path", &missing),
        ("path", &paths[1]),
        ("path", &paths[4]),
        ("path", &paths[5]),
        ("path", &paths[6]),
        ("url", "file:///etc/hostname"),
        ("url", "https://user:secret@example.org/"),
        ("path", "relative/path.txt"),
        ("path", &paths[2]),
        ("path", &paths[3]),
        ("url", "ftp://example.org/x"),
    ];
    let response = socket.open(&items, json!({}));
    let result = result_of(&response);
    let reported: Vec<(bool, String)> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            assert_eq!(item["index"], index);
            (
                item["ok"].as_bool().unwrap(),
                item["error"]["code"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let expected: Vec<(bool, &str)> = vec![
        (true, ""),
        (true, ""),
        (false, "is_directory"),
        (false, "not_found"),
        (true, ""),
        (false, "binary_file"),
        (false, "invalid_utf8"),
        (false, "too_large"),
        (false, "unsupported_scheme"),
        (false, "invalid_url"),
        (false, "invalid_operand"),
        (true, ""),
        (true, ""),
        (false, "unsupported_scheme"),
    ];
    assert_eq!(
        reported,
        expected
            .into_iter()
            .map(|(ok, code)| (ok, code.to_owned()))
            .collect::<Vec<_>>()
    );
    assert_eq!(result["ok"], false);
    let items = result["items"].as_array().unwrap();
    assert_eq!(items[0]["kind"], "file");
    assert_eq!(items[0]["mode"], "markdown");
    assert_eq!(items[0]["deduplicated"], false);
    assert_eq!(items[0]["path"], paths[0]);
    assert_eq!(items[1]["kind"], "url");
    assert_eq!(items[1]["url"], "https://example.org/docs");
    assert_eq!(items[4]["mode"], "text");
    assert_eq!(items[11]["mode"], "image");
    assert_eq!(items[12]["mode"], "pdf");
    for index in [0, 1, 4, 11, 12] {
        assert!(items[index]["surface"]["surface_id"].is_u64(), "{index}");
        assert!(items[index]["surface"]["window_uuid"].is_string());
    }
    // New tabs are appended in operand order, one receipt each, and only the first success
    // took the selection.
    let created: Vec<_> = result["created_surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["surface_id"].clone())
        .collect();
    let expected_ids: Vec<_> = [0, 1, 4, 11, 12]
        .iter()
        .map(|i| items[*i]["surface"]["surface_id"].clone())
        .collect();
    assert_eq!(created, expected_ids);
    assert_eq!(result["focused_surface"], items[0]["surface"]["surface_id"]);
    assert_eq!(fx.surfaces_of_kind(true), 4);
    assert_eq!(fx.surfaces_of_kind(false), 1);
    assert!(fx.snapshot().model.revision > before);
    // No file content crosses the socket.
    assert!(!response.to_string().contains("# title"));
}

#[test]
fn reopening_dedupes_and_focus_false_keeps_the_selection() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let first = fx.write("one.txt", b"1");
    let second = fx.write("two.txt", b"2");
    let (first, second) = (first.to_str().unwrap(), second.to_str().unwrap());
    let terminal = selected_surface(&fx.snapshot());

    let response = socket.open(&[("path", first)], json!({"focus": false}));
    let item = result_of(&response)["items"][0].clone();
    assert_eq!(item["ok"], true);
    assert_eq!(
        selected_surface(&fx.snapshot()),
        terminal,
        "focus false keeps the selection"
    );

    let response = socket.open(&[("path", second), ("path", first)], json!({}));
    let items = &result_of(&response)["items"];
    assert_eq!(items[0]["deduplicated"], false);
    assert_eq!(items[1]["deduplicated"], true);
    assert_eq!(
        items[1]["surface"]["surface_id"],
        item["surface"]["surface_id"]
    );
    assert_eq!(
        selected_surface(&fx.snapshot()).0,
        items[0]["surface"]["surface_id"].as_u64().unwrap(),
        "only the first success is focused, a later duplicate does not steal it"
    );
    assert_eq!(fx.surfaces_of_kind(true), 2, "a duplicate opens no new tab");

    // Deduplicating as the first success focuses the existing tab.
    let response = socket.open(&[("path", first)], json!({}));
    let again = &result_of(&response)["items"][0];
    assert_eq!(again["deduplicated"], true);
    assert_eq!(
        selected_surface(&fx.snapshot()).0,
        item["surface"]["surface_id"].as_u64().unwrap()
    );
    assert_eq!(result_of(&response)["created_surfaces"], json!([]));
}

#[test]
fn the_seventeenth_file_and_a_forced_mode_are_item_results_over_the_socket() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let names: Vec<String> = (0..MAX_FILE_SURFACES)
        .map(|i| {
            fx.write(&format!("c{i}.txt"), b"x")
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let items: Vec<(&str, &str)> = names.iter().map(|n| ("path", n.as_str())).collect();
    let response = socket.open(&items, json!({"focus": false}));
    assert_eq!(result_of(&response)["ok"], true);
    let extra = fx.write("extra.txt", b"x");
    let response = socket.open(&[("path", extra.to_str().unwrap())], json!({}));
    let item = &result_of(&response)["items"][0];
    assert_eq!(item["error"]["code"], "file_capacity");
    assert_eq!(result_of(&response)["ok"], false);
    assert_eq!(fx.surfaces_of_kind(true), MAX_FILE_SURFACES);

    // `markdown open`: a text file becomes markdown, media and URLs are refused per item.
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let notes = fx.write("notes.txt", b"text");
    let png = fx.write(
        "p.png",
        &[&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], b"x"].concat(),
    );
    let response = socket.open(
        &[
            ("path", notes.to_str().unwrap()),
            ("path", png.to_str().unwrap()),
            ("url", "https://example.org/"),
        ],
        json!({"force_mode": "markdown"}),
    );
    let items = &result_of(&response)["items"];
    assert_eq!(items[0]["ok"], true);
    assert_eq!(items[0]["mode"], "markdown");
    assert_eq!(items[1]["error"]["code"], "content_mismatch");
    assert_eq!(items[2]["error"]["code"], "invalid_operand");
}

#[test]
fn a_read_only_socket_is_denied_before_any_file_is_touched() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::ReadOnly);
    let path = fx.write("secret.txt", b"x");
    let before = fx.snapshot().model.revision;
    let calls = fx.probe_calls();
    let response = socket.open(
        &[
            ("path", path.to_str().unwrap()),
            ("path", "/definitely/missing"),
        ],
        json!({}),
    );
    assert_eq!(error_code(&response), "policy_denied", "{response}");
    assert!(response.get("result").is_none_or(Value::is_null));
    assert_eq!(
        fx.probe_calls(),
        calls,
        "no existence oracle: the backend was not called at all"
    );
    assert_eq!(fx.snapshot().model.revision, before);
    assert_eq!(fx.surfaces_of_kind(true), 0);
    // Reads stay available.
    let capabilities = socket.call(json!({"method": "capabilities"}));
    assert_eq!(result_of(&capabilities)["access"], "read_only");
}

#[test]
fn a_stale_revision_is_a_whole_request_error_that_changes_nothing() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let path = fx.write("rev.txt", b"x");
    let model = fx.snapshot().model.clone();
    let calls = fx.probe_calls();
    let response = socket.open(
        &[("path", path.to_str().unwrap())],
        json!({"expected_model_revision": model.revision + 5}),
    );
    assert_eq!(error_code(&response), "stale_revision");
    assert!(response.get("result").is_none_or(Value::is_null));
    assert_eq!(fx.snapshot().model.revision, model.revision);
    assert_eq!(fx.surfaces_of_kind(true), 0);
    assert_eq!(
        fx.probe_calls(),
        calls,
        "refused before any file is examined"
    );

    // The current revision is accepted.
    let response = socket.open(
        &[("path", path.to_str().unwrap())],
        json!({"expected_model_revision": model.revision}),
    );
    assert_eq!(result_of(&response)["items"][0]["ok"], true);
}

#[test]
fn the_executor_rechecks_the_revision_and_target_before_any_item_runs() {
    use desktop_runtime::files::{OpenBatch, OpenPlan};
    let fx = Fixture::new();
    let path = fx.write("exec.txt", b"x");
    let probe = fx
        .desktop
        .file_open(vec![path.to_str().unwrap().into()], None, false)
        .unwrap();
    assert_eq!(probe["ok"], true);
    // A batch whose revision went stale between the probe and the executor (the window the
    // early socket check cannot close) applies nothing.
    let before = fx.snapshot();
    let batch = |revision, scope: Value| OpenBatch {
        focus: true,
        scope: serde_json::from_value(scope).unwrap(),
        caller: Default::default(),
        expected_model_revision: revision,
        items: vec![OpenPlan::Url {
            url: "https://example.org/".into(),
        }],
    };
    for (id, stale) in [
        (990, batch(Some(before.model.revision + 9), json!({}))),
        (991, batch(None, json!({"pane": "pane:9999"}))),
    ] {
        fx.desktop
            .submit(Request {
                version: 1,
                request_id: id,
                payload: Command::OpenFiles { batch: stale },
            })
            .unwrap();
        let after = wait(&fx.desktop, |s| s.last_request_id == id);
        assert!(after.last_error.is_some(), "request {id} must be refused");
        assert_eq!(after.model.revision, before.model.revision);
        assert_eq!(fx.surfaces_of_kind(false), 0, "no browser tab was created");
    }
}

#[test]
fn a_gated_probe_times_out_per_item_over_the_socket_while_urls_still_open() {
    let fx = Fixture::with(|c| c.probe_timeout = Duration::from_millis(150));
    let socket = fx.serve(ControlAccess::Full);
    let slow = fx.write("slow.txt", b"slow");
    fx.control.arm("canonicalize");
    let response = thread::scope(|scope| {
        let call = scope.spawn(|| {
            socket.open(
                &[
                    ("path", slow.to_str().unwrap()),
                    ("url", "https://example.org/"),
                ],
                json!({}),
            )
        });
        fx.control.wait_blocked(1);
        let response = call.join().unwrap();
        fx.control.release();
        response
    });
    let result = result_of(&response);
    assert_eq!(result["items"][0]["error"]["code"], "probe_timeout");
    assert_eq!(result["items"][1]["ok"], true);
    assert_eq!(result["ok"], false);
    assert_eq!(fx.surfaces_of_kind(true), 0);
    assert_eq!(fx.surfaces_of_kind(false), 1);
}

#[test]
fn open_items_validates_its_request_and_the_executor_path_refuses_it_raw() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let calls = fx.probe_calls();
    let none = socket.open(&[], json!({}));
    assert_eq!(error_code(&none), "invalid_request");
    let many: Vec<(&str, &str)> = vec![("url", "https://example.org/"); 17];
    assert_eq!(
        error_code(&socket.open(&many, json!({}))),
        "invalid_request"
    );
    let unknown = socket.call(json!({"method": "targeted", "request": {
        "scope": {}, "caller": {}, "expected_model_revision": null,
        "operation": {"operation": "open_items", "items": [], "surprise": true}}}));
    assert_eq!(unknown["ok"], false);
    let bad_kind = socket.call(json!({"method": "targeted", "request": {
        "scope": {}, "caller": {}, "expected_model_revision": null,
        "operation": {"operation": "open_items", "items": [{"kind": "stdin", "value": "x"}]}}}));
    assert_eq!(bad_kind["ok"], false);
    assert_eq!(fx.probe_calls(), calls);
    assert_eq!(fx.surfaces_of_kind(true) + fx.surfaces_of_kind(false), 0);

    // The generic executor path never takes unprobed paths.
    let operation: desktop_runtime::targets::TargetOperation = serde_json::from_value(json!({
        "operation": "open_items", "items": [{"kind": "path", "value": "/etc/hostname"}]
    }))
    .unwrap();
    fx.desktop
        .submit(Request {
            version: 1,
            request_id: 992,
            payload: Command::Targeted {
                request: desktop_runtime::targets::TargetRequest {
                    operation,
                    ..Default::default()
                },
            },
        })
        .unwrap();
    let after = wait(&fx.desktop, |s| s.last_request_id == 992);
    assert!(
        after
            .last_error
            .as_deref()
            .unwrap()
            .contains("control-socket probe path"),
        "{:?}",
        after.last_error
    );
    assert_eq!(fx.surfaces_of_kind(true), 0);
    // And `open_files` is not a wire command at all.
    assert!(
        serde_json::from_value::<Request>(json!({
            "version": 1, "request_id": 1,
            "payload": {"command": "open_files", "batch": {}}
        }))
        .is_err()
    );
}

#[test]
fn new_tabs_go_to_the_resolved_pane_and_inspection_reports_file_surfaces() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let workspace = fx.snapshot().model.workspaces[0].clone();
    let first_pane = workspace.focused_pane;
    fx.desktop
        .submit(Request {
            version: 1,
            request_id: 993,
            payload: Command::Action {
                action: Action::SplitPane {
                    pane_id: first_pane,
                    axis: zmux_workspace_core::Axis::Horizontal,
                },
            },
        })
        .unwrap();
    let split = wait(&fx.desktop, |s| {
        s.last_request_id == 993 && s.pending_launches == 0
    });
    assert_ne!(
        split.model.workspaces[0].focused_pane, first_pane,
        "the split focused its new pane, so the default target is not the first pane"
    );
    let path = fx.write("scoped.txt", b"scoped");
    let response = socket.open(
        &[("path", path.to_str().unwrap())],
        json!({"scope": {"pane": format!("pane:{}", first_pane.0)}}),
    );
    let item = &result_of(&response)["items"][0];
    assert_eq!(item["surface"]["pane_id"], first_pane.0);

    let listed = socket.call(json!({"method": "list_pane_surfaces", "pane_id": first_pane.0}));
    let entries = result_of(&listed)["surfaces"].as_array().unwrap().clone();
    let entry = entries
        .iter()
        .find(|e| e["kind"] == "file")
        .unwrap_or_else(|| panic!("no file entry in {listed}"));
    assert_eq!(entry["file"]["path"], path.to_str().unwrap());
    assert_eq!(entry["file"]["mode"], "text");
    assert_eq!(entry["file"]["dirty"], false);
    assert!(entry["session_id"].is_null());

    let current = socket.call(json!({"method": "current", "limit": 50}));
    let current_item = result_of(&current)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "file")
        .cloned()
        .unwrap_or_else(|| panic!("no file item in {current}"));
    assert!(
        current_item["session"].is_null(),
        "a file has no terminal session"
    );
    assert_eq!(current_item["pane"]["id"], first_pane.0);
}

#[test]
fn capabilities_advertise_the_file_panel_limits_actually_in_force() {
    let fx = Fixture::with(|c| c.poll_interval = Duration::from_millis(750));
    let socket = fx.serve(ControlAccess::Full);
    let capabilities = socket.call(json!({"method": "capabilities"}));
    assert_eq!(
        result_of(&capabilities)["file_panel"],
        json!({
            "max_file_surfaces": 16, "max_open_items": 16, "max_text_bytes": 2_097_152,
            "max_preview_bytes": 7_340_032, "poll_interval_ms": 750,
            "open_probe_timeout_ms": 5000,
        })
    );
}

#[test]
fn a_closed_desktop_is_reported_as_closed_not_as_an_invalid_open_request() {
    let fx = Fixture::new();
    let socket = fx.serve(ControlAccess::Full);
    let path = fx.write("closing.txt", b"x");
    let operand = path.to_str().unwrap();
    // A malformed request is still `invalid_request`: the code reflects the real cause.
    assert_eq!(error_code(&socket.open(&[], json!({}))), "invalid_request");
    let calls = fx.probe_calls();
    fx.desktop.shutdown(Duration::from_secs(5)).unwrap();
    let response = socket.open(&[("path", operand)], json!({}));
    assert_eq!(error_code(&response), "desktop_closed", "{response}");
    assert_eq!(
        fx.probe_calls(),
        calls,
        "no file was touched after shutdown"
    );
}
