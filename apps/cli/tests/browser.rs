//! Grammar and transport contracts; successful DOM execution belongs to native WebKit tests.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Parallel tests may observe the same wall-clock tick on macOS.
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            PathBuf::from("/tmp").join(format!("zb{}-{nonce:x}-{sequence:x}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn desktop(&self) -> (PathBuf, Arc<Desktop>, ControlServer) {
        let socket = self.0.join("control.sock");
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: Some(self.0.clone()),
            })
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = desktop.snapshot().unwrap();
            if !snapshot.model.workspaces.is_empty() && snapshot.pending_launches == 0 {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        (socket, desktop, server)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("browser fixture cleanup: {error}");
        }
    }
}
fn client(socket: &Path, root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
    command
        .arg("--socket")
        .arg(socket)
        .env("HOME", root)
        .env_remove("ZMUX_SOCKET_PASSWORD")
        .env_remove("ZMUX_SOCKET_PASSWORD_FILE")
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID");
    command
}
fn invoke(socket: &Path, root: &Path, args: &[&str]) -> Output {
    client(socket, root).args(args).output().unwrap()
}
fn value(socket: &Path, root: &Path, args: &[&str]) -> Value {
    let output = invoke(socket, root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Forward the public wire protocol to the real runtime. Delay admission, or
/// withhold a real status response, without manufacturing execution receipts.
struct BrowserSocketDelay {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl BrowserSocketDelay {
    fn new(root: &Path, upstream: PathBuf, admission_delay: Duration, stall_status: bool) -> Self {
        let path = root.join("delayed.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let (mut client, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("delayed socket accept: {error}"),
                };
                client
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut length = [0; 4];
                client.read_exact(&mut length).unwrap();
                let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
                client.read_exact(&mut bytes).unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                if request["command"]["method"] == "browser" {
                    thread::sleep(admission_delay);
                }
                let mut upstream = UnixStream::connect(&upstream).unwrap();
                upstream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                upstream.write_all(&length).unwrap();
                upstream.write_all(&bytes).unwrap();
                upstream.read_exact(&mut length).unwrap();
                let mut response = vec![0; u32::from_be_bytes(length) as usize];
                upstream.read_exact(&mut response).unwrap();
                if stall_status && request["command"]["method"] == "browser_status" {
                    while !worker_stop.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(2));
                    }
                } else {
                    // The pre-fix CLI may hit its deadline and close first.
                    let _ = client
                        .write_all(&length)
                        .and_then(|()| client.write_all(&response));
                }
            }
        });
        Self {
            path,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for BrowserSocketDelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
        fs::remove_file(&self.path).unwrap();
    }
}

#[test]
fn automation_help_is_offline_and_invalid_options_fail_before_connecting() {
    let fixture = Fixture::new();
    let socket = Path::new("relative-unused-socket");
    for args in [
        vec!["browser", "--help"],
        vec!["browser", "get", "--help"],
        vec!["browser", "get", "text", "--help"],
        vec!["browser", "snapshot", "--help"],
        vec!["browser", "eval", "--help"],
        vec!["browser", "back", "--help"],
    ] {
        let output = invoke(socket, &fixture.0, &args);
        assert!(output.status.success(), "offline help failed: {args:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("--snapshot-after"));
    }
    for (args, reason) in [
        (
            vec!["browser", "open", "--focus"],
            "--focus requires true or false",
        ),
        (
            vec!["browser", "open", "--focus", "yes", "about:blank"],
            "--focus requires true or false",
        ),
        (
            vec![
                "browser",
                "open",
                "--no-focus",
                "about:blank",
                "--focus",
                "true",
            ],
            "conflicting browser focus",
        ),
        (
            vec!["browser", "open", "--no-focus", "--no-focus", "about:blank"],
            "conflicting browser focus",
        ),
        (
            vec!["browser", "open", "about:blank", "--bogus"],
            "unknown browser open option",
        ),
        (vec!["browser", "get"], "requires a kind"),
        (
            vec!["browser", "get", "unknown"],
            "unknown browser get kind",
        ),
        (
            vec!["browser", "get", "title", "#one"],
            "accepts no selector",
        ),
        (
            vec!["browser", "get", "text"],
            "requires a nonempty selector",
        ),
        (
            vec!["browser", "get", "text", "#one", "#two"],
            "extra arguments",
        ),
        (
            vec!["browser", "get", "text", "--selector"],
            "requires a nonempty value",
        ),
        (vec!["browser", "get", "attr", "#one"], "requires --attr"),
        (
            vec!["browser", "get", "attr", "#one", "--attr", "id", "title"],
            "extra arguments",
        ),
        (
            vec!["browser", "get", "text", "#one", "--property", "color"],
            "unknown browser get option",
        ),
        (vec!["browser", "eval"], "requires a nonempty script"),
        (
            vec!["browser", "eval", "--script", "  "],
            "requires a nonempty script",
        ),
        (
            vec!["browser", "eval", "--script"],
            "requires a nonempty value",
        ),
        (
            vec!["browser", "eval", "--script", "one", "two"],
            "conflicts",
        ),
        (
            vec!["browser", "eval", "--script", "one", "--script", "two"],
            "repeated --script",
        ),
        (
            vec!["browser", "eval", "--snapshot-after", "one"],
            "unknown browser eval option",
        ),
        (
            vec!["browser", "snapshot", "--max-depth", "-1"],
            "nonnegative integer",
        ),
        (
            vec!["browser", "snapshot", "--max-depth", "4294967296"],
            "nonnegative integer",
        ),
        (
            vec!["browser", "snapshot", "--interactive", "-i"],
            "repeated --interactive",
        ),
        (
            vec![
                "browser",
                "snapshot",
                "--selector",
                "one",
                "--selector",
                "two",
            ],
            "repeated --selector",
        ),
        (
            vec!["browser", "snapshot", "--cursor", "--cursor"],
            "repeated --cursor",
        ),
        (
            vec!["browser", "snapshot", "--compact", "--compact"],
            "repeated --compact",
        ),
        (
            vec!["browser", "snapshot", "trailing"],
            "accepts options only",
        ),
        (
            vec!["browser", "back", "--snapshot-after", "--snapshot-after"],
            "repeated --snapshot-after",
        ),
        (vec!["browser", "forward", "1"], "accepts options only"),
        (
            vec!["browser", "reload", "--hard"],
            "unknown browser reload option",
        ),
        (
            vec!["browser", "snapshot", "--surface", "1", "--tab", "2"],
            "repeated --tab",
        ),
        (
            vec!["browser", "snapshot", "--surface"],
            "requires a nonempty value",
        ),
        (vec!["browser", "snapshot", "--surface", "0"], "positive ID"),
        (
            vec!["browser", "snapshot", "--expect-revision", "x"],
            "nonnegative integer",
        ),
        (
            vec![
                "browser",
                "snapshot",
                "--expect-revision",
                "1",
                "--expect-revision",
                "2",
            ],
            "repeated --expect-revision",
        ),
    ] {
        let output = invoke(socket, &fixture.0, &args);
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (args, reason) in [
        (
            vec!["browser".to_owned(), "eval".into(), "x".repeat(8193)],
            "script",
        ),
        (
            vec![
                "browser".to_owned(),
                "get".into(),
                "text".into(),
                "x".repeat(2049),
            ],
            "selector",
        ),
        (
            vec![
                "browser".to_owned(),
                "snapshot".into(),
                "--max-depth".into(),
                "33".into(),
            ],
            "depth",
        ),
    ] {
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        let output = invoke(socket, &fixture.0, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("control socket path"));
    }
}

/// This peer only records bytes and rejects them. It cannot supply fake DOM success.
fn capture_rejection(listener: UnixListener) -> thread::JoinHandle<Value> {
    listener.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "CLI never sent its request");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("capture accept: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        let bytes = serde_json::to_vec(&json!({
            "version": request["version"], "request_id":request["request_id"], "ok":false,
            "error":{"code":"capture_only", "message":"transport fixture does not execute WebKit"}
        }))
        .unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
        request
    })
}

#[test]
fn browser_wire_preserves_scripts_options_and_existing_target_precedence() {
    let fixture = Fixture::new();
    let script = " \n({text: 'quoted \\\" text', value: -1})\n ";
    for (index, (args, expected)) in [
        (vec!["eval", "--script", script], json!({"action":"eval","script":script})),
        (vec!["eval", script], json!({"action":"eval","script":script})),
        (vec!["eval", "--", "-1"], json!({"action":"eval","script":"-1"})),
        (vec!["get", "text", "--selector", "#entry"], json!({"action":"get","kind":"text","selector":"#entry","attribute":null,"property":null})),
        (vec!["get", "html", "#entry"], json!({"action":"get","kind":"html","selector":"#entry","attribute":null,"property":null})),
        (vec!["get", "value", "#entry"], json!({"action":"get","kind":"value","selector":"#entry","attribute":null,"property":null})),
        (vec!["get", "count", "#entry"], json!({"action":"get","kind":"count","selector":"#entry","attribute":null,"property":null})),
        (vec!["get", "box", "#entry"], json!({"action":"get","kind":"box","selector":"#entry","attribute":null,"property":null})),
        (vec!["get", "attr", "#entry", "--attr", "data-id"], json!({"action":"get","kind":"attr","selector":"#entry","attribute":"data-id","property":null})),
        (vec!["get", "styles", "#entry", "--property", "color"], json!({"action":"get","kind":"styles","selector":"#entry","attribute":null,"property":"color"})),
        (vec!["get", "url"], json!({"action":"get","kind":"url","selector":null,"attribute":null,"property":null})),
        (vec!["get", "title"], json!({"action":"get","kind":"title","selector":null,"attribute":null,"property":null})),
        (vec!["snapshot", "--interactive", "--cursor", "--compact", "--max-depth", "0", "--selector", "main"], json!({"action":"snapshot","options":{"interactive":true,"cursor":true,"compact":true,"max_depth":0,"selector":"main"}})),
        (vec!["back", "--snapshot-after"], json!({"action":"back","snapshot_after":true})),
        (vec!["forward"], json!({"action":"forward","snapshot_after":false})),
        (vec!["reload"], json!({"action":"reload","snapshot_after":false})),
    ].into_iter().enumerate() {
        let socket = fixture.0.join(format!("c{index}"));
        let capture = capture_rejection(UnixListener::bind(&socket).unwrap());
        let prefix = if args[0] == "get" { 2 } else { 1 };
        let output = client(&socket, &fixture.0).arg("browser").args(&args[..prefix])
            .args(["--surface", "surface:7", "--expect-revision", "12"]).args(&args[prefix..])
            .env("ZMUX_WORKSPACE_ID", "invalid-caller-workspace")
            .env("ZMUX_SURFACE_ID", "invalid-caller-surface")
            .output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("capture_only"), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        let request = capture.join().unwrap();
        let request = &request["command"]["request"];
        assert_eq!(request["action"], expected, "{args:?}");
        assert_eq!(request["scope"]["surface"], "surface:7");
        assert!(request["caller"]["surface"].is_null());
        assert!(request["caller"]["workspace"].is_null());
        assert_eq!(request["expected_model_revision"], 12);
        assert!((1..=3000).contains(&request["timeout_ms"].as_u64().unwrap()));
    }
}

#[test]
fn portable_browser_requests_cannot_claim_native_dom_success_or_bypass_read_only() {
    let fixture = Fixture::new();
    let (socket, desktop, server) = fixture.desktop();
    let browser = value(&socket, &fixture.0, &["browser", "open", "about:blank"]);
    let surface = browser["focused_surface_uuid"].as_str().unwrap();
    for args in [
        vec!["browser", "get", "title", "--surface", surface],
        vec![
            "browser",
            "eval",
            "--surface",
            surface,
            "--script",
            "document.title",
        ],
        vec!["browser", "snapshot", "--surface", surface],
    ] {
        let output = invoke(&socket, &fixture.0, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("browser_unavailable"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    for args in [
        vec![
            "browser",
            "eval",
            "--surface",
            surface,
            "--script",
            "document.body.textContent='changed'",
        ],
        vec!["browser", "back", "--surface", surface],
        vec!["browser", "forward", "--surface", surface],
        vec!["browser", "reload", "--surface", surface],
    ] {
        let output = invoke(&socket, &fixture.0, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("read-only"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let read = invoke(
        &socket,
        &fixture.0,
        &["browser", "get", "title", "--surface", surface],
    );
    assert!(String::from_utf8_lossy(&read.stderr).contains("browser_unavailable"));
    assert!(value(&socket, &fixture.0, &["status"])["workspace_count"].is_u64());
    // Attach a real runtime lease, but provide no native consumer. The production
    // owner must expire the queued operation; the fixture never claims DOM execution.
    server
        .set_policy(ControlPolicy::same_user(ControlAccess::Full))
        .unwrap();
    let snapshot = desktop.snapshot().unwrap();
    let surface_id = browser["focused_surface"].as_u64().unwrap();
    let model_surface = snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .find(|s| s.id.0 == surface_id)
        .unwrap();
    desktop
        .browser_host_event(
            serde_json::from_value(json!({
                "event":"attach", "epoch":snapshot.browser_epoch,
                "surface_id":surface_id,"host_id":"00000000-0000-4000-8000-000000000001",
                "document_generation":1,"model_navigation":model_surface.navigation
            }))
            .unwrap(),
        )
        .unwrap();
    for stall_status in [false, true] {
        // Admission latency must not consume all of the receipt observation
        // budget. The negative control withholds status replies: the CLI must
        // then stop at its original deadline without inventing a host timeout.
        // Allow the real timeout receipt a 700 ms scheduling margin on macOS:
        // 4000 ms total - 3000 ms execution - 300 ms admission. The former
        // fixed 100 ms reserve still expires before execution can time out.
        let timeout_ms = if stall_status { 2000 } else { 4000 };
        let cli_timeout = timeout_ms.to_string();
        let proxy = BrowserSocketDelay::new(
            &fixture.0,
            socket.clone(),
            Duration::from_millis(300),
            stall_status,
        );
        let count = desktop.snapshot().unwrap().browser_operations.len();
        let cli_socket = proxy.path.clone();
        let cli_root = fixture.0.clone();
        let cli_surface = surface.to_owned();
        let started = Instant::now();
        let cli = thread::spawn(move || {
            invoke(
                &cli_socket,
                &cli_root,
                &[
                    "--timeout",
                    &cli_timeout,
                    "browser",
                    "eval",
                    "--surface",
                    &cli_surface,
                    "document.body.textContent = 'never executed'",
                ],
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if desktop.snapshot().unwrap().browser_operations.len() > count {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "browser operation was not admitted"
            );
            thread::sleep(Duration::from_millis(5));
        }
        // Receipt polling must leave the listener free for unrelated control requests.
        assert!(value(&socket, &fixture.0, &["status"])["workspace_count"].is_u64());
        let output = cli.join().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        if stall_status {
            assert!(!error.contains("browser_timeout"), "{error}");
            assert!(error.contains("waiting for zmux"), "{error}");
            assert!(error.contains("not retried"), "{error}");
            assert!(
                started.elapsed() < Duration::from_millis(2500),
                "CLI extended its total deadline"
            );
        } else {
            assert!(
                error.contains("browser_timeout"),
                "real runtime timeout receipt was not observed within {timeout_ms} ms (elapsed {:?}): {error}",
                started.elapsed()
            );
        }
        let after = desktop.snapshot().unwrap();
        assert_eq!(
            after.browser_operations.len(),
            count + 1,
            "CLI retried admission"
        );
        assert!(
            desktop
                .browser_drain_requests(Duration::from_secs(1))
                .unwrap()
                .requests
                .is_empty(),
            "expired evaluation remained executable"
        );
        drop(proxy);
    }
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn browser_open_returns_unselected_creation_receipts_and_defaults_to_focus() {
    let fixture = Fixture::new();
    let (socket, desktop, server) = fixture.desktop();
    let first = value(&socket, &fixture.0, &["list-workspaces"]);
    let first_workspace = first["focused_workspace"].as_u64().unwrap().to_string();
    let first_pane = first["workspaces"][0]["focused_pane"]
        .as_u64()
        .unwrap()
        .to_string();
    let first_surface = first["workspaces"][0]["root"]["pane"]["selected_surface"].clone();
    let other = value(&socket, &fixture.0, &["new-workspace", "Hidden target"]);
    let other_workspace = other["focused_workspace"].clone();
    let other_pane = other["focused_pane_uuid"].as_str().unwrap();
    let other_surface = other["focused_surface"].clone();
    value(&socket, &fixture.0, &["select-workspace", &first_workspace]);
    for args in [
        vec![
            "browser",
            "open",
            "--pane",
            other_pane,
            "--focus",
            "false",
            "about:blank",
        ],
        vec![
            "browser",
            "open",
            "--pane",
            other_pane,
            "about:blank",
            "--no-focus",
        ],
    ] {
        let opened = value(&socket, &fixture.0, &args);
        assert_eq!(opened["focused_workspace"], first["focused_workspace"]);
        assert_eq!(
            opened["focused_pane"].as_u64().unwrap().to_string(),
            first_pane
        );
        assert_eq!(opened["focused_surface"], first_surface);
        assert!(opened["created_workspaces"].as_array().unwrap().is_empty());
        assert!(opened["created_panes"].as_array().unwrap().is_empty());
        let created = opened["created_surfaces"].as_array().unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0]["workspace_id"], other_workspace);
        assert_ne!(created[0]["surface_id"], opened["focused_surface"]);
        let pane = value(
            &socket,
            &fixture.0,
            &["list-pane-surfaces", "--pane", other_pane],
        );
        let selected = pane["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["selected"] == true)
            .unwrap();
        assert_eq!(selected["surface_id"], other_surface);
        let created_surface = pane["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["surface_uuid"] == created[0]["surface_uuid"])
            .unwrap();
        assert_eq!(created_surface["kind"], "browser");
        assert!(created_surface["session_id"].is_null());
    }
    let focused = value(
        &socket,
        &fixture.0,
        &[
            "browser",
            "open",
            "--pane",
            other_pane,
            "--focus",
            "true",
            "about:blank",
        ],
    );
    assert_eq!(focused["focused_workspace"], other_workspace);
    assert_eq!(
        focused["focused_surface"],
        focused["created_surfaces"][0]["surface_id"]
    );
    let legacy = value(
        &socket,
        &fixture.0,
        &["browser", "open", &first_pane, "about:blank"],
    );
    assert_eq!(legacy["focused_workspace"], first["focused_workspace"]);
    assert_eq!(
        legacy["focused_surface"],
        legacy["created_surfaces"][0]["surface_id"]
    );
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
