//! `zmux open` and `zmux markdown open`: grammar, exact wire shape, per-item reporting and exit
//! status, then the same commands against a real Desktop behind a real ControlServer.
//!
//! Nothing here sleeps to wait for an outcome. Wire tests synchronise on the connection itself
//! (the CLI cannot finish without the reply the fixture server owes it) and "never connected" is
//! proved after the child has exited, so no timing assumption is involved. The end-to-end tests
//! start the production file service with generous deadlines because they assert behaviour, not
//! overload handling (that belongs to `crates/desktop-runtime/tests/files.rs`).
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{
    Desktop, DesktopOptions, Shell, control::ControlServer, files::FilesConfig,
};

static NEXT: AtomicU64 = AtomicU64::new(1);
/// Patience for conditions that must eventually hold; it bounds a hang, never a result.
const PATIENCE: Duration = Duration::from_secs(60);

fn private_dir(prefix: &str) -> PathBuf {
    let path = PathBuf::from("/tmp").join(format!(
        "{prefix}{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path.canonicalize().unwrap()
}

/// The zmux binary with every ambient target and credential variable removed.
fn zmux(socket: &Path, cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
    command
        .arg("--socket")
        .arg(socket)
        .args(args)
        .current_dir(cwd)
        .env("HOME", cwd)
        .stdin(Stdio::null());
    for key in [
        "ZMUX_WINDOW_ID",
        "ZMUX_WORKSPACE_ID",
        "ZMUX_SURFACE_ID",
        "ZMUX_TAB_ID",
        "ZMUX_LOCAL_TMUX",
        "ZMUX_SOCKET",
        "ZMUX_SOCKET_PATH",
        "ZMUX_SOCKET_PASSWORD",
        "ZMUX_SOCKET_PASSWORD_FILE",
    ] {
        command.env_remove(key);
    }
    command
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// One fake app: answers exactly one request with `reply` and records it. It owns the listener,
/// so a CLI that never connects is observable (`request` stays `None`) without waiting.
struct Wire {
    dir: PathBuf,
    socket: PathBuf,
    done: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Option<Value>>>,
}
impl Wire {
    fn new(reply: Value) -> Self {
        let dir = private_dir("zo");
        let socket = dir.join("c");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let worker = thread::spawn(move || {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop.load(Ordering::SeqCst) {
                            // The client has exited: one last look catches a connection that was
                            // queued before it did.
                            match listener.accept() {
                                Ok((stream, _)) => break stream,
                                Err(_) => return None,
                            }
                        }
                        thread::yield_now();
                    }
                    Err(error) => panic!("wire accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(PATIENCE)).unwrap();
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut body = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let mut response = reply;
            response["version"] = request["version"].clone();
            response["request_id"] = request["request_id"].clone();
            let response = serde_json::to_vec(&response).unwrap();
            stream
                .write_all(&(response.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&response).unwrap();
            Some(request)
        });
        Self {
            dir,
            socket,
            done,
            worker: Some(worker),
        }
    }
    fn ok(result: Value) -> Self {
        Self::new(json!({"ok": true, "result": result}))
    }
    /// The recorded request, or `None` when the client never connected.
    fn request(mut self) -> Option<Value> {
        self.done.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn operation(request: &Value) -> &Value {
    &request["command"]["request"]["operation"]
}

#[test]
fn open_sends_the_designed_wire_request_with_cwd_absolute_paths() {
    let cwd = private_dir("zc");
    let wire = Wire::ok(json!({"ok": true, "items": []}));
    let output = zmux(
        &wire.socket,
        &cwd,
        &[
            "open",
            "README.md",
            "sub/../notes.txt",
            "~/kept-literally.txt",
            "/abs/main.rs",
            "HTTPS://example.org/docs",
            "http://example.org/",
            "about:blank",
        ],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let request = wire.request().expect("the CLI connected");
    assert_eq!(request["version"], 2);
    assert_eq!(request["command"]["method"], "targeted");
    let base = cwd.to_str().unwrap();
    assert_eq!(
        *operation(&request),
        json!({
            "operation": "open_items",
            "focus": true,
            "force_mode": null,
            "items": [
                {"kind": "path", "value": format!("{base}/README.md")},
                {"kind": "path", "value": format!("{base}/sub/../notes.txt")},
                {"kind": "path", "value": format!("{base}/~/kept-literally.txt")},
                {"kind": "path", "value": "/abs/main.rs"},
                {"kind": "url", "value": "HTTPS://example.org/docs"},
                {"kind": "url", "value": "http://example.org/"},
                {"kind": "path", "value": format!("{base}/about:blank")},
            ]
        })
    );
    let target = &request["command"]["request"];
    assert_eq!(
        target["scope"],
        json!({"window": null, "workspace": null, "pane": null, "surface": null})
    );
    assert!(target["expected_model_revision"].is_null());
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn open_options_select_scope_focus_and_literal_operands() {
    let cwd = private_dir("zc");
    for (args, focus) in [
        (
            vec![
                "open",
                "--window",
                "window:1",
                "--workspace",
                "workspace:2",
                "--pane",
                "pane:3",
                "--surface",
                "surface:4",
                "--expect-revision",
                "9",
                "--no-focus",
                "--",
                "-dash.txt",
                "--also-literal",
            ],
            false,
        ),
        (
            vec!["open", "--tab", "tab:4", "--focus", "false", "a.txt"],
            false,
        ),
        (vec!["open", "a.txt", "--focus", "true"], true),
        (vec!["open", "--focus", "true", "--", "--focus"], true),
    ] {
        let wire = Wire::ok(json!({"ok": true, "items": []}));
        let output = zmux(&wire.socket, &cwd, &args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
        let request = wire.request().unwrap();
        assert_eq!(operation(&request)["focus"], focus, "{args:?}");
        let target = &request["command"]["request"];
        if args.contains(&"--window") {
            assert_eq!(
                target["scope"],
                json!({"window": "window:1", "workspace": "workspace:2", "pane": "pane:3", "surface": "surface:4"})
            );
            assert_eq!(target["expected_model_revision"], 9);
            let base = cwd.to_str().unwrap();
            assert_eq!(
                operation(&request)["items"],
                json!([
                    {"kind": "path", "value": format!("{base}/-dash.txt")},
                    {"kind": "path", "value": format!("{base}/--also-literal")},
                ])
            );
        }
        if args.contains(&"--tab") {
            assert_eq!(target["scope"]["surface"], "tab:4");
        }
    }
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn open_defaults_follow_the_existing_target_environment_rules() {
    let cwd = private_dir("zc");
    let wire = Wire::ok(json!({"ok": true, "items": []}));
    let output = zmux(&wire.socket, &cwd, &["open", "a.txt"])
        .env("ZMUX_WORKSPACE_ID", "workspace:5")
        .env("ZMUX_SURFACE_ID", "surface:7")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let caller = &wire.request().unwrap()["command"]["request"]["caller"];
    assert_eq!(caller["workspace"], "workspace:5");
    assert_eq!(caller["surface"], "surface:7");
    // An explicit selector replaces the ambient caller, exactly as for every other targeted command.
    let wire = Wire::ok(json!({"ok": true, "items": []}));
    let output = zmux(
        &wire.socket,
        &cwd,
        &["open", "--workspace", "workspace:2", "a.txt"],
    )
    .env("ZMUX_WORKSPACE_ID", "workspace:5")
    .env("ZMUX_SURFACE_ID", "surface:7")
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let request = wire.request().unwrap();
    let caller = &request["command"]["request"]["caller"];
    assert!(caller["workspace"].is_null() && caller["surface"].is_null());
    assert_eq!(
        request["command"]["request"]["scope"]["workspace"],
        "workspace:2"
    );
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn markdown_open_forces_markdown_and_accepts_paths_only() {
    let cwd = private_dir("zc");
    let wire = Wire::ok(json!({"ok": true, "items": []}));
    let output = zmux(
        &wire.socket,
        &cwd,
        &["markdown", "open", "--no-focus", "README", "docs/guide.md"],
    )
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let request = wire.request().unwrap();
    let base = cwd.to_str().unwrap();
    assert_eq!(
        *operation(&request),
        json!({
            "operation": "open_items", "focus": false, "force_mode": "markdown",
            "items": [
                {"kind": "path", "value": format!("{base}/README")},
                {"kind": "path", "value": format!("{base}/docs/guide.md")},
            ]
        })
    );
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn lexical_errors_fail_the_whole_invocation_before_connecting() {
    let cwd = private_dir("zc");
    let long = "x".repeat(4097);
    let seventeen: Vec<String> = (0..17).map(|n| format!("f{n}.txt")).collect();
    let oversize: Vec<String> = (0..8)
        .map(|n| format!("/{n}{}", "y".repeat(3000)))
        .collect();
    let mut cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["open"], "at least one"),
        (vec!["open", "--no-focus"], "at least one"),
        (vec!["open", ""], "invalid_operand"),
        (vec!["open", "tab\there"], "invalid_operand"),
        (vec!["open", &long], "invalid_operand"),
        (vec!["open", "file:///etc/hosts"], "unsupported_scheme"),
        (
            vec!["open", "a.txt", "ftp://example.org/x"],
            "unsupported_scheme",
        ),
        (vec!["open", "--bogus", "a.txt"], "unknown open option"),
        (vec!["open", "-x"], "unknown open option"),
        (
            vec!["open", "a.txt", "--pane", "pane:1"],
            "unknown open option",
        ),
        (
            vec!["open", "--focus", "maybe", "a"],
            "--focus requires true or false",
        ),
        (
            vec!["open", "--no-focus", "--focus", "true", "a"],
            "conflicting",
        ),
        (
            vec!["open", "--focus", "true", "--focus", "false", "a"],
            "conflicting",
        ),
        (
            vec!["open", "--pane", "pane:1", "--pane", "pane:2", "a"],
            "repeated --pane",
        ),
        (vec!["open", "--pane", "pane:0", "a"], "positive ID"),
        (vec!["markdown"], "markdown requires open"),
        (vec!["markdown", "bogus", "a.md"], "markdown requires open"),
        (vec!["markdown", "open"], "at least one"),
        (
            vec!["markdown", "open", "https://example.org/"],
            "invalid_operand",
        ),
        (vec!["markdown", "open", "file:///a.md"], "invalid_operand"),
    ];
    let seventeen_refs: Vec<&str> = std::iter::once("open")
        .chain(seventeen.iter().map(String::as_str))
        .collect();
    cases.push((seventeen_refs, "at most 16"));
    let oversize_refs: Vec<&str> = std::iter::once("open")
        .chain(oversize.iter().map(String::as_str))
        .collect();
    cases.push((oversize_refs, "split the operands across invocations"));
    for (args, expected) in cases {
        let wire = Wire::ok(json!({"ok": true, "items": []}));
        let output = zmux(&wire.socket, &cwd, &args).output().unwrap();
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(
            stderr(&output).contains(expected),
            "{:?}: expected `{expected}` in `{}`",
            &args[..args.len().min(4)],
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
        assert!(
            wire.request().is_none(),
            "{:?} connected before being rejected",
            &args[..args.len().min(4)]
        );
    }
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn item_failures_print_the_full_result_and_exit_one_without_retrying() {
    let cwd = private_dir("zc");
    let result = json!({"ok": false, "items": [
        {"index": 0, "kind": "file", "ok": true, "path": "/a.md", "mode": "markdown", "deduplicated": false},
        {"index": 1, "kind": "file", "ok": false, "path": "/dir",
         "error": {"code": "is_directory", "message": "directories cannot be opened"}},
        {"index": 2, "kind": "url", "ok": false, "url": "http://u:p@x/",
         "error": {"code": "invalid_url", "message": "credentials"}},
    ]});
    let wire = Wire::ok(result.clone());
    let output = zmux(
        &wire.socket,
        &cwd,
        &["open", "a.md", "dir", "http://u:p@x/"],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        result
    );
    assert!(
        stderr(&output).contains("zmux: 2 of 3 items failed"),
        "{}",
        stderr(&output)
    );
    assert!(wire.request().is_some());

    let wire = Wire::ok(json!({"ok": true, "items": [
        {"index": 0, "kind": "file", "ok": true, "path": "/a.md", "mode": "markdown", "deduplicated": true}]}));
    let output = zmux(&wire.socket, &cwd, &["open", "a.md"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stderr(&output).is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["items"][0]["deduplicated"],
        true
    );
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn whole_request_failures_exit_one_with_the_server_error_and_no_items() {
    let cwd = private_dir("zc");
    let wire = Wire::new(json!({"ok": false, "error": {
        "code": "stale_revision", "message": "expected 3 but the model is at 4"}}));
    let output = zmux(
        &wire.socket,
        &cwd,
        &["open", "--expect-revision", "3", "a.txt"],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).contains("zmux: stale_revision: expected 3 but the model is at 4"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("items failed"));
    assert_eq!(
        operation(&wire.request().unwrap())["items"][0]["kind"],
        "path"
    );
    let _ = fs::remove_dir_all(cwd);
}

#[test]
fn help_describes_both_commands_offline() {
    let cwd = private_dir("zc");
    for args in [
        vec!["open", "--help"],
        vec!["help", "open"],
        vec!["markdown", "--help"],
        vec!["markdown", "open", "--help"],
        vec!["help", "markdown"],
    ] {
        let output = zmux(Path::new("relative-must-not-be-read"), &cwd, &args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Usage:"), "{args:?}");
        assert!(text.contains("PATH"), "{args:?}");
        assert!(
            text.contains("2 MiB") && text.contains("7 MiB"),
            "{args:?}: {text}"
        );
        assert!(text.contains("--no-focus"), "{args:?}");
    }
    let top = zmux(Path::new("relative-must-not-be-read"), &cwd, &["--help"])
        .output()
        .unwrap();
    let top = String::from_utf8(top.stdout).unwrap();
    assert!(top.contains("Files:") && top.contains("open [TARGET_OPTIONS]"));
    let _ = fs::remove_dir_all(cwd);
}

/// A real Desktop and ControlServer with the production file service and generous deadlines.
struct Fixture {
    root: PathBuf,
    dir: PathBuf,
    socket: PathBuf,
    desktop: Arc<Desktop>,
    server: Option<ControlServer>,
}
impl Fixture {
    fn new() -> Self {
        let root = private_dir("zf");
        let dir = root.join("files");
        fs::create_dir(&dir).unwrap();
        let socket = root.join("c");
        let desktop = Arc::new(
            Desktop::with_options(
                // A terminal that never writes, so no revision appears behind a test's back.
                Shell {
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), "exec cat".into()],
                    cwd: Some(root.clone()),
                },
                DesktopOptions {
                    files: Some(FilesConfig {
                        probe_timeout: PATIENCE,
                        read_timeout: PATIENCE,
                        save_timeout: PATIENCE,
                        command_timeout: PATIENCE,
                        ..FilesConfig::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let deadline = Instant::now() + PATIENCE;
        loop {
            let snapshot = desktop_snapshot::read_until(&desktop, deadline);
            if !snapshot.model.workspaces.is_empty() && snapshot.pending_launches == 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the starter workspace never came up"
            );
            thread::sleep(Duration::from_millis(2));
        }
        Self {
            root,
            dir,
            socket,
            desktop,
            server: Some(server),
        }
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, bytes).unwrap();
        path
    }
    /// Every call states a generous global deadline: the assertions are about behaviour.
    fn run(&self, args: &[&str]) -> Output {
        zmux(&self.socket, &self.dir, &["--timeout", "30000"])
            .args(args)
            .output()
            .unwrap()
    }
    #[track_caller]
    fn value(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(output.status.success(), "{args:?}: {}", stderr(&output));
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn pane_surfaces(&self, pane: &str) -> Vec<Value> {
        self.value(&["list-pane-surfaces", "--pane", pane])["surfaces"]
            .as_array()
            .unwrap()
            .clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.shutdown(Duration::from_secs(30));
        }
        let _ = self.desktop.shutdown(Duration::from_secs(30));
        let _ = fs::remove_dir_all(&self.root);
    }
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

#[test]
fn a_mixed_batch_creates_ordered_tabs_and_reports_each_item() {
    let fx = Fixture::new();
    fx.write("README.md", b"# title\n");
    fx.write("src/main.rs", b"fn main() {}\n");
    fx.write("pic.png", PNG);
    fx.write("binary.dat", &[1, 2, 0, 3]);
    let panes = fx.value(&["list-panes"]);
    let pane = panes["panes"][0]["pane_id"].to_string();
    let before = fx.pane_surfaces(&pane);
    assert_eq!(before.len(), 1);

    let output = fx.run(&[
        "open",
        "README.md",
        "src/main.rs",
        "pic.png",
        "src",
        "missing.txt",
        "binary.dat",
        "https://example.org/docs",
    ]);
    // Items 3, 4 and 5 fail; the rest still opened, in order.
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("zmux: 3 of 7 items failed"),
        "{}",
        stderr(&output)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], false);
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 7);
    let base = fx.dir.to_str().unwrap();
    for (index, mode) in [(0, "markdown"), (1, "text"), (2, "image")] {
        assert_eq!(items[index]["ok"], true, "{}", items[index]);
        assert_eq!(items[index]["kind"], "file");
        assert_eq!(items[index]["mode"], mode);
        assert_eq!(items[index]["deduplicated"], false);
        assert_eq!(items[index]["index"], index);
        assert!(items[index]["surface"]["surface_id"].is_u64());
    }
    assert_eq!(items[0]["path"], format!("{base}/README.md"));
    assert_eq!(items[1]["path"], format!("{base}/src/main.rs"));
    for (index, code) in [(3, "is_directory"), (4, "not_found"), (5, "binary_file")] {
        assert_eq!(items[index]["ok"], false, "{}", items[index]);
        assert_eq!(items[index]["error"]["code"], code, "{}", items[index]);
        assert!(items[index].get("surface").is_none());
    }
    assert_eq!(items[6]["kind"], "url");
    assert_eq!(items[6]["ok"], true, "{}", items[6]);
    assert_eq!(items[6]["url"], "https://example.org/docs");

    // Tabs were appended to the one pane in operand order, only for the successes.
    let after = fx.pane_surfaces(&pane);
    let added: Vec<_> = after[before.len()..].iter().collect();
    assert_eq!(added.len(), 4, "{after:?}");
    assert_eq!(added[0]["kind"], "file");
    assert_eq!(added[0]["file"]["mode"], "markdown");
    assert_eq!(added[1]["file"]["path"], format!("{base}/src/main.rs"));
    assert_eq!(added[2]["file"]["mode"], "image");
    assert_eq!(added[3]["kind"], "browser");
    // Focus defaults to true and goes to the first success only.
    let selected: Vec<_> = after.iter().filter(|s| s["selected"] == true).collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0]["surface_uuid"],
        items[0]["surface"]["surface_uuid"]
    );

    // All-success exits 0 with the full JSON and a quiet stderr.
    fx.write("again.txt", b"hello\n");
    let ok = fx.run(&["open", "again.txt"]);
    assert_eq!(ok.status.code(), Some(0), "{}", stderr(&ok));
    assert!(stderr(&ok).is_empty());
    let ok: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(ok["ok"], true);
    assert_eq!(ok["items"][0]["mode"], "text");
    assert_eq!(ok["created_surfaces"].as_array().unwrap().len(), 1);
}

#[test]
fn an_upper_case_scheme_opens_a_browser_tab_with_the_canonical_scheme() {
    let fx = Fixture::new();
    let panes = fx.value(&["list-panes"]);
    let pane = panes["panes"][0]["pane_id"].to_string();
    let before = fx.pane_surfaces(&pane).len();
    let result = fx.value(&[
        "open",
        "HTTPS://Example.org/Docs?Q=1",
        "HtTp://example.net/",
    ]);
    assert_eq!(result["ok"], true, "{result}");
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["kind"], "url");
        assert_eq!(item["ok"], true, "{item}");
        assert!(item["surface"]["surface_id"].is_u64());
    }
    assert_eq!(items[0]["url"], "https://Example.org/Docs?Q=1");
    assert_eq!(items[1]["url"], "http://example.net/");
    let after = fx.pane_surfaces(&pane);
    assert_eq!(after.len(), before + 2);
    assert!(
        after[before..]
            .iter()
            .all(|surface| surface["kind"] == "browser")
    );
}

#[test]
fn the_command_catalog_does_not_depend_on_the_working_directory() {
    let first = private_dir("zc");
    let second = private_dir("zc");
    let socket = Path::new("relative-must-not-be-read");
    let run = |cwd: &Path, args: &[&str]| zmux(socket, cwd, args).output().unwrap();
    let catalog_a = run(&first, &["commands"]);
    let catalog_b = run(&second, &["commands"]);
    assert!(catalog_a.status.success(), "{}", stderr(&catalog_a));
    assert!(catalog_b.status.success(), "{}", stderr(&catalog_b));
    // Each probe request carries a fresh random id; everything else must be identical.
    fn without_request_ids(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("request_id");
                map.values_mut().for_each(without_request_ids);
            }
            Value::Array(items) => items.iter_mut().for_each(without_request_ids),
            _ => {}
        }
    }
    let mut parsed_a: Value = serde_json::from_slice(&catalog_a.stdout).unwrap();
    let mut parsed_b: Value = serde_json::from_slice(&catalog_b.stdout).unwrap();
    without_request_ids(&mut parsed_a);
    without_request_ids(&mut parsed_b);
    assert_eq!(parsed_a, parsed_b);
    let text = String::from_utf8(catalog_a.stdout).unwrap();
    assert!(
        !text.contains(first.to_str().unwrap()) && !text.contains(second.to_str().unwrap()),
        "the catalog embeds a working directory"
    );

    // A working directory that was removed under the process must not matter either.
    for args in [&["commands"][..], &["commands", "--check"][..]] {
        let gone = private_dir("zg");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg("cd \"$1\" && rmdir \"$1\" && shift && exec \"$@\"")
            .arg("sh")
            .arg(&gone)
            .arg(env!("CARGO_BIN_EXE_zmux"))
            .args(args)
            .env("HOME", &first)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} from a removed directory: {}",
            stderr(&output)
        );
        assert!(!gone.exists());
    }
    let _ = fs::remove_dir_all(first);
    let _ = fs::remove_dir_all(second);
}

#[test]
fn dedupe_no_focus_and_the_first_success_focus_rule() {
    let fx = Fixture::new();
    fx.write("a.txt", b"a\n");
    fx.write("b.txt", b"b\n");
    fx.write("c.txt", b"c\n");
    let panes = fx.value(&["list-panes"]);
    let pane = panes["panes"][0]["pane_id"].to_string();
    let starter = fx.pane_surfaces(&pane)[0]["surface_uuid"].clone();

    // --no-focus keeps the starter terminal selected.
    let first = fx.value(&["open", "--no-focus", "a.txt", "b.txt"]);
    assert_eq!(first["ok"], true);
    let surfaces = fx.pane_surfaces(&pane);
    assert_eq!(surfaces.len(), 3);
    let selected: Vec<_> = surfaces.iter().filter(|s| s["selected"] == true).collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["surface_uuid"], starter);

    // Re-opening is a success that creates nothing and reports the existing tab.
    let again = fx.value(&["open", "--focus", "false", "b.txt", "./a.txt"]);
    assert_eq!(again["ok"], true);
    assert_eq!(again["items"][0]["deduplicated"], true);
    assert_eq!(again["items"][1]["deduplicated"], true);
    assert_eq!(
        again["items"][0]["surface"]["surface_uuid"],
        first["items"][1]["surface"]["surface_uuid"]
    );
    assert!(again["created_surfaces"].as_array().unwrap().is_empty());
    assert_eq!(fx.pane_surfaces(&pane).len(), 3);

    // With focus, an existing tab is selected (first success) and a new later one is not.
    let focused = fx.value(&["open", "b.txt", "c.txt"]);
    assert_eq!(focused["items"][0]["deduplicated"], true);
    assert_eq!(focused["items"][1]["deduplicated"], false);
    let surfaces = fx.pane_surfaces(&pane);
    assert_eq!(surfaces.len(), 4);
    let selected: Vec<_> = surfaces.iter().filter(|s| s["selected"] == true).collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0]["surface_uuid"],
        first["items"][1]["surface"]["surface_uuid"]
    );
}

#[test]
fn explicit_targets_environment_defaults_markdown_force_and_stale_revisions() {
    let fx = Fixture::new();
    fx.write("README", b"# no extension\n");
    fx.write("notes.txt", b"notes\n");
    fx.write("pic.png", PNG);
    let first = fx.value(&["list-workspaces"]);
    let first_workspace = first["workspaces"][0]["id"].to_string();
    let first_pane = fx.value(&["list-panes"])["panes"][0]["pane_id"].to_string();
    // A second workspace becomes the focused one.
    let second = fx.value(&["new-workspace", "Second"]);
    let second_pane = second["focused_pane"].to_string();
    assert_ne!(second_pane, first_pane);

    // The caller's workspace (the ZMUX_WORKSPACE_ID rule) wins over the focused workspace.
    let output = zmux(
        &fx.socket,
        &fx.dir,
        &["--timeout", "30000", "open", "notes.txt"],
    )
    .env("ZMUX_WORKSPACE_ID", format!("workspace:{first_workspace}"))
    .output()
    .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let opened: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(opened["ok"], true);
    assert_eq!(
        opened["items"][0]["surface"]["workspace_id"].to_string(),
        first_workspace
    );
    assert!(
        fx.pane_surfaces(&first_pane)
            .iter()
            .any(|s| s["file"]["mode"] == "text")
    );
    assert!(
        !fx.pane_surfaces(&second_pane)
            .iter()
            .any(|s| s["kind"] == "file")
    );

    // markdown open forces Markdown on an extensionless file and refuses an image per item.
    let forced = fx.run(&[
        "markdown",
        "open",
        "--pane",
        &format!("pane:{second_pane}"),
        "README",
        "pic.png",
    ]);
    assert_eq!(forced.status.code(), Some(1), "{}", stderr(&forced));
    let forced: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(forced["items"][0]["ok"], true, "{forced}");
    assert_eq!(forced["items"][0]["mode"], "markdown");
    assert_eq!(forced["items"][1]["ok"], false);
    assert_eq!(forced["items"][1]["error"]["code"], "content_mismatch");
    assert!(
        fx.pane_surfaces(&second_pane)
            .iter()
            .any(|s| s["file"]["mode"] == "markdown")
    );

    // A stale revision is a whole-request failure: no items, nothing created.
    let revision = desktop_snapshot::read(&fx.desktop).model.revision;
    let count = fx.pane_surfaces(&second_pane).len();
    let stale = fx.run(&[
        "open",
        "--pane",
        &format!("pane:{second_pane}"),
        "--expect-revision",
        &(revision - 1).to_string(),
        "notes.txt",
    ]);
    assert_eq!(stale.status.code(), Some(1));
    assert!(stale.stdout.is_empty());
    assert!(
        stderr(&stale).contains("stale_revision"),
        "{}",
        stderr(&stale)
    );
    assert_eq!(fx.pane_surfaces(&second_pane).len(), count);

    // A target that does not exist is also a whole-request failure.
    let missing = fx.run(&["open", "--pane", "pane:999999", "notes.txt"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert!(
        !stderr(&missing).contains("items failed"),
        "{}",
        stderr(&missing)
    );
}

#[test]
fn a_read_only_socket_is_refused_before_any_file_is_probed() {
    // The production listener with a read-only policy: the refusal is the server's, relayed by
    // the CLI as a whole-request failure with no per-item data.
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let root = private_dir("zr");
    let socket = root.join("c");
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exec cat".into()],
            cwd: Some(root.clone()),
        })
        .unwrap(),
    );
    let server = ControlServer::start_with_policy(
        &socket,
        Arc::clone(&desktop),
        ControlPolicy::same_user(ControlAccess::ReadOnly),
    )
    .unwrap();
    let output = zmux(
        &socket,
        &root,
        &["--timeout", "30000", "open", "/etc/hostname"],
    )
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        stderr(&output).contains("policy_denied"),
        "{}",
        stderr(&output)
    );
    server.shutdown(Duration::from_secs(30)).unwrap();
    desktop.shutdown(Duration::from_secs(30)).unwrap();
    let _ = fs::remove_dir_all(root);
}
