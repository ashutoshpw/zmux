//! Public CLI against the real window owner; trusted native acknowledgements are
//! driven at the production boundary. This does not execute or claim AppKit UI.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{
    Desktop, Shell,
    control::ControlServer,
    windows::{NativeWindowEvent, NativeWindowRequest, WindowHostToken, WindowKind},
};
static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn workspace_move_destination_is_distinct_from_source_and_focus_failure_keeps_topology() {
    let fixture = Fixture::new();
    let before = desktop_snapshot::read(&fixture.desktop);
    let source = before.model.windows[0].id;
    let workspace = before.model.workspaces[0].id;
    let surface = before.model.workspaces[0].root.panes()[0].selected_surface;
    let child = fixture.spawn(&["new-window", "--no-focus"]);
    let open = fixture.pending(WindowKind::Open);
    fixture.acknowledge(&open);
    let created = value(child.wait_with_output().unwrap());
    let destination = format!("window:{}", created["window_id"]);
    let moved = fixture.value(&[
        "move-workspace-to-window",
        "--window",
        &destination,
        "--workspace",
        &format!("workspace:{}", workspace.0),
    ]);
    assert_eq!(moved["state"], "topology_committed");
    assert_eq!(moved["source_window_id"], source.0);
    assert_eq!(moved["destination_window_id"], open.window_id.0);
    assert_eq!(moved["native_focus"], Value::Null);
    let snapshot = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(snapshot.model.focused_window, Some(source));
    assert_eq!(snapshot.model.focused_workspace, None);
    let current = value(
        fixture
            .command(&["current-window"])
            .env("ZMUX_WINDOW_ID", format!("window:{}", source.0))
            .env("ZMUX_SURFACE_ID", format!("surface:{}", surface.0))
            .output()
            .unwrap(),
    );
    assert_eq!(
        current["window_id"], open.window_id.0,
        "surviving caller must override its inherited old window"
    );
    let moving = fixture.spawn(&[
        "move-workspace-to-window",
        "--workspace",
        &workspace.0.to_string(),
        "--window",
        &format!("window:{}", source.0),
        "--focus",
        "true",
    ]);
    let focus = fixture.pending(WindowKind::Focus);
    fixture
        .desktop
        .native_window_event(NativeWindowEvent::Failed {
            epoch: focus.epoch,
            operation_id: focus.operation_id,
            message: "native focus deliberately unavailable".into(),
        })
        .unwrap();
    let failed = moving.wait_with_output().unwrap();
    assert!(!failed.status.success());
    let message = String::from_utf8_lossy(&failed.stderr);
    assert!(message.contains("topology already committed"), "{message}");
    assert!(message.contains("not replayed or rolled back"), "{message}");
    let committed = desktop_snapshot::read(&fixture.desktop);
    assert!(
        committed.model.windows[0]
            .workspace_ids
            .contains(&workspace)
    );
    assert!(
        !committed.model.windows[1]
            .workspace_ids
            .contains(&workspace)
    );
    let revision = committed.model.revision;
    let stale = fixture
        .command(&[
            "move-workspace-to-window",
            "--workspace",
            &workspace.0.to_string(),
            "--window",
            &destination,
            "--expect-revision",
            &(revision - 1).to_string(),
            "--no-focus",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).model,
        committed.model
    );
    let success = fixture.spawn(&[
        "move-workspace-to-window",
        "--focus",
        "true",
        "--workspace",
        &workspace.0.to_string(),
        "--window",
        &destination,
    ]);
    let native = fixture.pending(WindowKind::Focus);
    fixture.acknowledge(&native);
    let completed = value(success.wait_with_output().unwrap());
    assert_eq!(completed["state"], "topology_committed");
    assert_eq!(completed["native_focus"]["state"], "completed");
    assert_eq!(
        completed["native_focus"]["operation_id"],
        native.operation_id.to_string()
    );
    let selected = desktop_snapshot::read(&fixture.desktop).model.clone();
    let same = fixture.value(&[
        "move-workspace-to-window",
        "--workspace",
        &workspace.0.to_string(),
        "--window",
        &destination,
    ]);
    assert_eq!(same["changed_window"], false);
    assert_eq!(same["focus_requested"], false);
    assert_eq!(same["native_focus"], Value::Null);
    let unchanged = desktop_snapshot::read(&fixture.desktop).model.clone();
    assert_eq!(unchanged.windows, selected.windows);
    assert_eq!(unchanged.focused_window, selected.focused_window);
    for args in [
        vec!["move-workspace-to-window"],
        vec!["move-workspace-to-window", "--workspace", "1"],
        vec!["move-workspace-to-window", "--window", "1"],
        vec![
            "move-workspace-to-window",
            "--workspace",
            "1",
            "--window",
            "2",
            "--focus",
            "true",
            "--no-focus",
        ],
        vec![
            "move-workspace-to-window",
            "--workspace",
            "1",
            "--window",
            "2",
            "--focus",
            "maybe",
        ],
        vec![
            "move-workspace-to-window",
            "--workspace",
            "1",
            "--window",
            "2",
            "--no-focus",
            "--no-focus",
        ],
    ] {
        assert!(!fixture.command(&args).output().unwrap().status.success());
    }
    assert!(
        fixture
            .command(&["help", "move-workspace-to-window"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    desktop: Arc<Desktop>,
    server: ControlServer,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "zw{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: Some(root.clone()),
            })
            .unwrap(),
        );
        let socket = root.join("c");
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let fixture = Self {
            root,
            socket,
            desktop,
            server,
        };
        let open = fixture.pending(WindowKind::Open);
        fixture.acknowledge(&open);
        fixture
            .desktop
            .native_window_event(NativeWindowEvent::Key {
                token: fixture.token(&open),
                key: true,
            })
            .unwrap();
        fixture
    }
    fn command(&self, args: &[&str]) -> Command {
        self.command_at(&self.socket, args)
    }
    fn command_at(&self, socket: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command
            .arg("--socket")
            .arg(socket)
            .args(args)
            .stdin(Stdio::null());
        for key in [
            "ZMUX_WINDOW_ID",
            "ZMUX_WORKSPACE_ID",
            "ZMUX_SURFACE_ID",
            "ZMUX_TAB_ID",
            "ZMUX_LOCAL_TMUX",
            "ZMUX_SOCKET_PASSWORD",
            "ZMUX_SOCKET_PASSWORD_FILE",
        ] {
            command.env_remove(key);
        }
        command
    }
    fn spawn(&self, args: &[&str]) -> Child {
        self.command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
    fn value(&self, args: &[&str]) -> Value {
        value(self.command(args).output().unwrap())
    }
    fn pending(&self, kind: WindowKind) -> NativeWindowRequest {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(request) = desktop_snapshot::read_until(&self.desktop, deadline)
                .window_requests
                .iter()
                .find(|r| r.kind == kind)
                .cloned()
            {
                return request;
            }
            assert!(
                Instant::now() < deadline,
                "Window request was never admitted"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn token(&self, request: &NativeWindowRequest) -> WindowHostToken {
        WindowHostToken {
            epoch: request.epoch,
            identity_namespace: request.identity_namespace,
            window_id: request.window_id,
            host_id: request.host_id.unwrap_or(request.operation_id),
        }
    }
    fn acknowledge(&self, request: &NativeWindowRequest) {
        self.desktop
            .native_window_event(NativeWindowEvent::Complete {
                token: self.token(request),
                operation_id: request.operation_id,
            })
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.shutdown(Duration::from_secs(2)).unwrap();
        self.desktop.shutdown(Duration::from_secs(2)).unwrap();
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("window fixture cleanup: {error}");
        }
    }
}
fn value(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn finish(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "Window CLI did not finish within its expected deadline: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn window_cli_waits_for_native_receipts_and_preserves_no_focus_and_exact_scope() {
    let fixture = Fixture::new();
    let initial = desktop_snapshot::read(&fixture.desktop);
    let original = initial.model.focused_window.unwrap();
    let root_surface = initial.model.workspaces[0].root.panes()[0].selected_surface;
    let mut new = fixture.spawn(&["new-window", "--no-focus"]);
    let open = fixture.pending(WindowKind::Open);
    assert_ne!(open.window_id, original);
    assert!(!open.focus);
    assert!(
        new.try_wait().unwrap().is_none(),
        "Logical creation was reported as native completion"
    );
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop)
            .model
            .focused_window,
        Some(original)
    );
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).observed_key_window,
        Some(original)
    );
    fixture.acknowledge(&open);
    let created = value(finish(new));
    assert_eq!(created["state"], "completed");
    assert_eq!(created["kind"], "open");
    assert_eq!(created["operation_id"], open.operation_id.to_string());
    assert_eq!(created["window_uuid"], open.window_uuid.to_string());
    let handle = open.window_uuid.to_string();
    assert_eq!(
        fixture.value(&["list-windows"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let current = value(
        fixture
            .command(&["current-window", "--window", &handle])
            .env("ZMUX_SURFACE_ID", "surface:0")
            .output()
            .unwrap(),
    );
    assert_eq!(current["window_uuid"], handle);
    let mismatch = fixture
        .command(&["read-screen", "--window", &handle])
        .env("ZMUX_SURFACE_ID", root_surface.0.to_string())
        .env("ZMUX_WINDOW_ID", handle.clone())
        .output()
        .unwrap();
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("scope_mismatch"));
    let hinted = value(
        fixture
            .command(&["current-window"])
            .env("ZMUX_WINDOW_ID", &handle)
            .output()
            .unwrap(),
    );
    assert_eq!(hinted["window_uuid"], handle);
    let hinted = value(
        fixture
            .command(&["current-window"])
            .env("ZMUX_SURFACE_ID", root_surface.0.to_string())
            .env("ZMUX_WINDOW_ID", &handle)
            .output()
            .unwrap(),
    );
    assert_eq!(
        hinted["window_id"], original.0,
        "A stale window hint overrode the surviving caller object"
    );
    let browser = fixture.value(&["browser", "open", "--no-focus", "about:blank"]);
    let browser_id = browser["created_surfaces"][0]["surface_id"]
        .as_u64()
        .unwrap()
        .to_string();
    let original_workspace = initial.model.workspaces[0].id.0.to_string();
    let original_surface = root_surface.0.to_string();
    for args in [
        vec![
            "browser",
            "--window",
            handle.as_str(),
            "get",
            "title",
            "--surface",
            browser_id.as_str(),
        ],
        vec![
            "browser",
            "download",
            "list",
            "--window",
            handle.as_str(),
            "--surface",
            browser_id.as_str(),
        ],
        vec![
            "set-status",
            "key",
            "value",
            "--window",
            handle.as_str(),
            "--workspace",
            original_workspace.as_str(),
        ],
        vec![
            "notify",
            "--window",
            handle.as_str(),
            "--surface",
            original_surface.as_str(),
            "--title",
            "Wrong window",
        ],
    ] {
        let output = fixture.command(&args).output().unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("scope_mismatch"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let hinted = value(
        fixture
            .command(&["current-window"])
            .env("ZMUX_SURFACE_ID", root_surface.0.to_string())
            .env("ZMUX_WINDOW_ID", "window:999999")
            .output()
            .unwrap(),
    );
    assert_eq!(hinted["window_id"], original.0);
    let hinted = value(
        fixture
            .command(&["current-window"])
            .env("ZMUX_WINDOW_ID", "not-a-window-handle")
            .output()
            .unwrap(),
    );
    assert_eq!(hinted["window_id"], original.0);
    let focus = fixture.spawn(&["focus-window", "--window", &handle]);
    let request = fixture.pending(WindowKind::Focus);
    fixture
        .desktop
        .native_window_event(NativeWindowEvent::Key {
            token: fixture.token(&request),
            key: true,
        })
        .unwrap();
    fixture.acknowledge(&request);
    assert_eq!(value(finish(focus))["window_uuid"], handle);
    assert_eq!(fixture.value(&["current-window"])["window_uuid"], handle);
    let close = fixture.spawn(&["close-window", "--window", &handle]);
    let request = fixture.pending(WindowKind::Close);
    fixture.acknowledge(&request);
    assert_eq!(value(finish(close))["kind"], "close");
    assert_eq!(
        fixture.value(&["list-windows"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let close_last = fixture.spawn(&["close-window", "--window", &original.0.to_string()]);
    let terminate = fixture.pending(WindowKind::Terminate);
    fixture.acknowledge(&terminate);
    assert_eq!(value(finish(close_last))["kind"], "terminate");
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).model.windows.len(),
        1,
        "A terminate acknowledgement is not proof of saved shutdown"
    );
    fixture
        .server
        .set_policy(
            zmux_desktop_runtime::control_policy::ControlPolicy::same_user(
                zmux_desktop_runtime::control_policy::ControlAccess::ReadOnly,
            ),
        )
        .unwrap();
    assert_eq!(
        fixture.value(&["list-windows"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fixture.value(&["current-window"])["window_id"], original.0);
    let denied = fixture.command(&["new-window"]).output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).model.windows.len(),
        1
    );
}

#[test]
fn accepted_window_mutation_times_out_without_replay_or_premature_success() {
    let fixture = Fixture::new();
    let proxy_path = fixture.root.join("p");
    let listener = UnixListener::bind(&proxy_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let upstream = fixture.socket.clone();
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    let proxy = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let accept = || {
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "CLI never reached proxy phase");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("window proxy accept: {error}"),
                }
            };
            // Darwin inherits the listener's nonblocking mode.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
        };
        let frame = |stream: &mut UnixStream| {
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let length = u32::from_be_bytes(length) as usize;
            assert!(length > 0 && length <= desktop_runtime::control::MAX_CONTROL_RESPONSE_BYTES);
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).unwrap();
            bytes
        };
        let mut client = accept();
        let request = frame(&mut client);
        let mut server = UnixStream::connect(upstream).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        server
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        server
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        server.write_all(&request).unwrap();
        let response = frame(&mut server);
        let admission: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(admission["ok"], true);
        client
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        client.write_all(&response).unwrap();
        drop(client);
        let mut client = accept();
        let status: Value = serde_json::from_slice(&frame(&mut client)).unwrap();
        assert_eq!(status["command"]["method"], "window_status");
        assert_eq!(status["command"]["epoch"], admission["result"]["epoch"]);
        assert_eq!(
            status["command"]["operation_id"],
            admission["result"]["operation_id"]
        );
        // This request proves the CLI decoded the real admission receipt. Hold
        // its response until the client's own deadline; never manufacture one.
        observed_tx
            .send(status["command"]["operation_id"].clone())
            .unwrap();
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "CLI replayed a window mutation or receipt request"
        );
    });
    // The small transport deadline is covered independently in cli.rs. Here an
    // observed status request establishes the receipt phase before its timeout.
    let create = fixture
        .command_at(&proxy_path, &["--timeout", "3000", "new-window"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let observed = match observed_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(observed) => observed,
        Err(error) => {
            let output = finish(create);
            panic!(
                "CLI did not observe window admission: {error}; {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    };
    let request = fixture.pending(WindowKind::Open);
    assert_eq!(observed, request.operation_id.to_string());
    assert!(request.focus, "new-window must request focus by default");
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop)
            .model
            .focused_window,
        Some(request.window_id)
    );
    let output = finish(create);
    proxy.join().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&request.operation_id.to_string()), "{error}");
    assert!(error.contains("not replayed"), "{error}");
    let snapshot = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(snapshot.model.windows.len(), 2);
    assert_eq!(
        snapshot
            .window_requests
            .iter()
            .filter(|r| r.kind == WindowKind::Open)
            .count(),
        1
    );
    fixture.acknowledge(&request);
    assert_eq!(
        fixture.value(&["list-windows"])["windows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn failed_native_open_is_reported_and_explicit_close_allows_a_new_window() {
    let fixture = Fixture::new();
    let create = fixture.spawn(&["new-window", "--no-focus"]);
    let open = fixture.pending(WindowKind::Open);
    fixture
        .desktop
        .native_window_event(NativeWindowEvent::Failed {
            epoch: open.epoch,
            operation_id: open.operation_id,
            message: "Native host creation failed".into(),
        })
        .unwrap();
    let output = finish(create);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains(&open.operation_id.to_string()), "{error}");
    assert!(error.contains("Native host creation failed"), "{error}");
    let handle = open.window_uuid.to_string();
    let listed = fixture.value(&["list-windows"]);
    let failed = listed["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|window| window["window_uuid"] == handle)
        .expect("A failed native open must retain its recoverable model row");
    assert_eq!(failed["native_state"], "failed");
    assert_eq!(failed["native_error"], "Native host creation failed");
    assert!(failed["recovery"].as_str().unwrap().contains("Close"));
    let snapshot = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(snapshot.model.windows.len(), 2);
    assert!(
        snapshot
            .window_requests
            .iter()
            .all(|request| request.window_id != open.window_id),
        "Native failure must not automatically replay its open operation"
    );
    let close = fixture.spawn(&["close-window", "--window", &handle]);
    let request = fixture.pending(WindowKind::Close);
    assert_eq!(request.window_id, open.window_id);
    fixture.acknowledge(&request);
    assert_eq!(value(finish(close))["kind"], "close");
    assert!(
        fixture.value(&["list-windows"])["windows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|window| window["window_uuid"] != handle)
    );
    let retry = fixture.spawn(&["new-window", "--no-focus"]);
    let replacement = fixture.pending(WindowKind::Open);
    assert_ne!(replacement.window_id, open.window_id);
    fixture.acknowledge(&replacement);
    assert_eq!(value(finish(retry))["state"], "completed");
}

#[test]
fn window_grammar_and_help_fail_offline_before_connecting() {
    let absent = Path::new("/missing-window-cli.sock");
    for args in [
        vec!["new-window", "--focus", "false"],
        vec!["new-window", "--no-focus", "--no-focus"],
        vec!["new-window", "--window", "1"],
        vec!["focus-window"],
        vec!["focus-window", "1"],
        vec!["focus-window", "--window", "0"],
        vec!["close-window", "--window", "1", "--window", "2"],
        vec!["current-window", "--pane", "1"],
        vec!["list-windows", "--window", "1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(absent)
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("cannot connect"),
            "Grammar reached socket: {args:?}"
        );
    }
    for name in [
        "list-windows",
        "current-window",
        "new-window",
        "focus-window",
        "close-window",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-must-not-be-used", name, "--help"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn reorder_cli_preview_and_apply_share_window_order_and_preserve_live_selection() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let fixture = Fixture::new();
    for name in ["B", "C", "D"] {
        fixture.value(&["new-workspace", name]);
    }
    let before = desktop_snapshot::read(&fixture.desktop);
    let window = before.model.windows[0].id;
    let ids = before.model.windows[0].workspace_ids.clone();
    let subject = format!("workspace:{}", ids[0].0);
    let handle = format!("window:{}", window.0);
    fixture
        .server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let preview = fixture.value(&[
        "reorder-workspace",
        &subject,
        "--after-workspace",
        "index:3",
        "--window",
        &handle,
        "--dry-run",
    ]);
    assert_eq!(preview["from_index"], 0);
    assert_eq!(preview["to_index"], 2);
    assert_eq!(preview["events"], serde_json::json!([]));
    assert_eq!(desktop_snapshot::read(&fixture.desktop).model, before.model);
    let denied = fixture
        .command(&["reorder-workspace", &subject, "--index", "2"])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    fixture.server.set_policy(ControlPolicy::default()).unwrap();
    let applied = fixture.value(&[
        "reorder-workspace",
        &subject,
        "--after",
        "index:3",
        "--window",
        &handle,
    ]);
    assert_eq!(preview["plan"], applied["plan"]);
    assert_eq!(applied["events"].as_array().unwrap().len(), 1);
    let reordered = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(
        reordered.model.windows[0].workspace_ids,
        vec![ids[1], ids[2], ids[0], ids[3]]
    );
    assert_eq!(
        reordered.model.focused_workspace,
        before.model.focused_workspace
    );
    assert_eq!(
        reordered.model.windows[0].selected_workspace,
        before.model.windows[0].selected_workspace
    );
    assert_eq!(reordered.model.workspaces, before.model.workspaces);
    let scoped = fixture.value(&[
        "reorder-workspace",
        "index:1",
        "--index",
        "-10",
        "--window",
        &handle,
        "--dry-run",
    ]);
    assert_eq!(
        scoped["workspace_id"], ids[1].0,
        "index:N must follow canonical order after reorder"
    );
    let order = format!(" {}, {} ", ids[3].0, ids[2].0);
    let bulk_preview = fixture.value(&["reorder-workspaces", "--order", &order, "--dry-run"]);
    let bulk = fixture.value(&["reorder-workspaces", "--order", &order]);
    assert_eq!(bulk["plan"], bulk_preview["plan"]);
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).model.windows[0].workspace_ids,
        vec![ids[3], ids[2], ids[1], ids[0]]
    );
    let stable = desktop_snapshot::read(&fixture.desktop).model.clone();
    for arguments in [
        vec![
            "reorder-workspace",
            &subject,
            "--index",
            "1",
            "--before",
            &subject,
        ],
        vec![
            "reorder-workspace",
            &subject,
            "--workspace",
            &subject,
            "--index",
            "1",
        ],
        vec!["reorder-workspaces", "--order", ""],
        vec!["reorder-workspaces", "--order", "1,,2"],
        vec![
            "reorder-workspaces",
            "--order",
            &format!("{},{}", ids[0].0, ids[0].0),
        ],
        vec![
            "reorder-workspace",
            &subject,
            "--index",
            "0",
            "--expect-revision",
            "0",
        ],
    ] {
        assert!(
            !fixture
                .command(&arguments)
                .output()
                .unwrap()
                .status
                .success(),
            "{arguments:?}"
        );
        assert_eq!(desktop_snapshot::read(&fixture.desktop).model, stable);
    }
}

#[test]
fn reorder_cli_resolves_caller_objects_and_never_borrows_another_window() {
    let fixture = Fixture::new();
    fixture.value(&["new-workspace", "source B"]);
    let original = desktop_snapshot::read(&fixture.desktop);
    let source = original.model.windows[0].id;
    let moving = original.model.workspaces[0].id;
    let surface = original.model.workspaces[0].root.panes()[0].selected_surface;
    let child = fixture.spawn(&["new-window", "--no-focus"]);
    let open = fixture.pending(WindowKind::Open);
    fixture.acknowledge(&open);
    value(finish(child));
    let destination = format!("window:{}", open.window_id.0);
    fixture.value(&[
        "move-workspace-to-window",
        "--workspace",
        &moving.0.to_string(),
        "--window",
        &destination,
    ]);
    let current = desktop_snapshot::read(&fixture.desktop).model.clone();
    let mut command = fixture.command(&[
        "reorder-workspace",
        "index:1",
        "--index",
        "100",
        "--dry-run",
    ]);
    command
        .env("ZMUX_SURFACE_ID", surface.0.to_string())
        .env("ZMUX_WORKSPACE_ID", moving.0.to_string())
        .env("ZMUX_WINDOW_ID", source.0.to_string());
    let resolved = value(command.output().unwrap());
    assert_eq!(resolved["window_id"], open.window_id.0);
    assert_eq!(
        resolved["workspace_id"],
        current.windows[1].workspace_ids[0].0
    );
    let mut stale = fixture.command(&["reorder-workspace", "index:1", "--index", "0", "--dry-run"]);
    stale.env("ZMUX_SURFACE_ID", "surface:999999");
    assert!(!stale.output().unwrap().status.success());
    let mut ambiguous =
        fixture.command(&["reorder-workspace", "index:1", "--index", "0", "--dry-run"]);
    ambiguous.env("ZMUX_SURFACE_ID", surface.0.to_string()).env(
        "ZMUX_WORKSPACE_ID",
        original.model.workspaces[1].id.0.to_string(),
    );
    assert!(!ambiguous.output().unwrap().status.success());
    assert!(
        !fixture
            .command(&[
                "reorder-workspace",
                &moving.0.to_string(),
                "--index",
                "0",
                "--window",
                &source.0.to_string()
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    let cross = format!("{},{}", moving.0, original.model.workspaces[1].id.0);
    assert!(
        !fixture
            .command(&["reorder-workspaces", "--order", &cross])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(desktop_snapshot::read(&fixture.desktop).model, current);
}

#[test]
fn surface_reorder_cli_resolves_one_pane_retains_legacy_and_obeys_socket_policy() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let fixture = Fixture::new();
    let initial = desktop_snapshot::read(&fixture.desktop);
    let pane = initial.model.workspaces[0].root.panes()[0].id;
    for _ in 0..2 {
        fixture.value(&["new-surface", &pane.0.to_string()]);
    }
    fixture.value(&["new-workspace", "foreground"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let before = loop {
        let s = desktop_snapshot::read_until(&fixture.desktop, deadline);
        if s.control_frames.len() == 4 && s.control_frames.iter().all(|f| f.process_id.is_some()) {
            break s;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    };
    let tabs = &before.model.workspaces[0].root.panes()[0].surfaces;
    let ids: Vec<_> = tabs.iter().map(|s| s.id).collect();
    let subject = ids[0].0.to_string();
    let mut explicit = fixture.command(&[
        "reorder-surface",
        "--surface",
        &subject,
        "--after-surface",
        &ids[1].0.to_string(),
    ]);
    explicit.env("ZMUX_SURFACE_ID", "bad inherited context");
    let receipt = value(explicit.output().unwrap());
    assert_eq!(receipt["state"], "order_committed");
    assert_eq!(receipt["from_index"], 0);
    assert_eq!(
        receipt["to_index"], 1,
        "adjacent downward placement must move"
    );
    assert_eq!(receipt["focus_requested"], false);
    assert!(receipt["native_focus"].is_null());
    let after = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(
        after.model.focused_workspace,
        before.model.focused_workspace
    );
    assert_eq!(
        after.model.workspaces[0].root.panes()[0].selected_surface,
        before.model.workspaces[0].root.panes()[0].selected_surface
    );
    assert_eq!(
        after
            .control_frames
            .iter()
            .map(|f| (f.session_id.0, f.process_id))
            .collect::<std::collections::BTreeMap<_, _>>(),
        before
            .control_frames
            .iter()
            .map(|f| (f.session_id.0, f.process_id))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    assert_eq!(
        after.model.workspaces[0].root.panes()[0]
            .surfaces
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![ids[1], ids[0], ids[2]]
    );
    assert_eq!(
        fixture.value(&["reorder-surface", &subject, "--after", &subject])["to_index"],
        1
    );
    assert_eq!(
        fixture.value(&["reorder-surface", "--tab", &subject, "--before", &subject])["to_index"],
        1
    );
    assert_eq!(
        fixture.value(&["reorder-surface", &subject, "-9223372036854775808"])["to_index"],
        0
    );
    let mut ambient = fixture.command(&["reorder-surface", "9223372036854775807"]);
    ambient.env("ZMUX_SURFACE_ID", &subject);
    let ambient = value(ambient.output().unwrap());
    assert_eq!(ambient["to_index"], 2);
    assert_eq!(ambient["focus_requested"], false);
    let model = desktop_snapshot::read(&fixture.desktop).model.clone();
    let foreign = model.workspaces[1].root.panes()[0].selected_surface;
    for args in [
        vec![
            "reorder-surface".into(),
            subject.clone(),
            "--before".into(),
            foreign.0.to_string(),
        ],
        vec![
            "reorder-surface".into(),
            subject.clone(),
            "--index".into(),
            "0".into(),
            "--expect-revision".into(),
            (model.revision - 1).to_string(),
        ],
    ] {
        assert!(
            !fixture
                .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
                .output()
                .unwrap()
                .status
                .success()
        );
        assert_eq!(desktop_snapshot::read(&fixture.desktop).model, model);
    }
    fixture
        .server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let denied = fixture
        .command(&["reorder-surface", &subject, "--index", "0"])
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    assert_eq!(desktop_snapshot::read(&fixture.desktop).model, model);
}

#[test]
fn surface_order_waits_for_exact_native_focus_and_never_replays_committed_order() {
    let fixture = Fixture::new();
    let pane = desktop_snapshot::read(&fixture.desktop).model.workspaces[0]
        .root
        .panes()[0]
        .id;
    fixture.value(&["new-surface", &pane.0.to_string()]);
    let initial = desktop_snapshot::read(&fixture.desktop);
    let id = initial.model.workspaces[0].root.panes()[0].surfaces[0]
        .id
        .0
        .to_string();
    let mut first = fixture.spawn(&["reorder-surface", &id, "--index", "1", "--focus", "true"]);
    let pending = fixture.pending(WindowKind::Focus);
    assert!(
        first.try_wait().unwrap().is_none(),
        "logical order is not native completion"
    );
    fixture
        .desktop
        .native_window_event(NativeWindowEvent::Failed {
            epoch: pending.epoch,
            operation_id: pending.operation_id,
            message: "deliberate focus failure".into(),
        })
        .unwrap();
    let failed = finish(first);
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(
        error.contains("already committed") && error.contains("not replayed or rolled back"),
        "{error}"
    );
    assert_eq!(
        desktop_snapshot::read(&fixture.desktop).model.workspaces[0]
            .root
            .panes()[0]
            .surfaces[1]
            .id
            .0
            .to_string(),
        id
    );
    let second = fixture.spawn(&["reorder-surface", &id, "--index", "0", "--focus", "true"]);
    let success = fixture.pending(WindowKind::Focus);
    assert_ne!(success.operation_id, pending.operation_id);
    fixture.acknowledge(&success);
    let completed = value(finish(second));
    assert_eq!(
        completed["native_focus"]["operation_id"],
        success.operation_id.to_string()
    );
    assert_eq!(completed["native_focus"]["state"], "completed");
    let timed = fixture.spawn(&[
        "--timeout",
        "100",
        "reorder-surface",
        &id,
        "--index",
        "1",
        "--focus",
        "true",
    ]);
    let unknown = fixture.pending(WindowKind::Focus);
    let timed = finish(timed);
    assert!(!timed.status.success());
    assert!(String::from_utf8_lossy(&timed.stderr).contains("already committed"));
    let snapshot = desktop_snapshot::read(&fixture.desktop);
    assert_eq!(
        snapshot
            .window_requests
            .iter()
            .filter(|r| r.kind == WindowKind::Focus)
            .count(),
        1
    );
    assert_eq!(
        snapshot.model.workspaces[0].root.panes()[0].surfaces[1]
            .id
            .0
            .to_string(),
        id
    );
    fixture.acknowledge(&unknown);
}

#[test]
fn surface_reorder_invalid_placements_fail_before_connecting() {
    for args in [
        vec!["reorder-surface"],
        vec!["reorder-surface", "--index", "1"],
        vec!["reorder-surface", "1", "--index", "1", "--before", "2"],
        vec!["reorder-surface", "1", "--before", "2", "--after", "2"],
        vec!["reorder-surface", "1", "--index", "9223372036854775808"],
        vec![
            "reorder-surface",
            "1",
            "--index",
            "0",
            "--focus",
            "true",
            "--no-focus",
        ],
        vec!["reorder-surface", "1", "--surface", "2", "--index", "0"],
        vec!["reorder-surface", "1", "--index", "0", "--dry-run"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "/missing-surface-order.sock"])
            .args(&args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("cannot connect"),
            "{args:?}"
        );
    }
}
