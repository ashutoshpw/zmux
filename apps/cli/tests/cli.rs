use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

fn socket_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    std::env::temp_dir()
        .join(format!(
            "zl{:x}{nonce:x}{:x}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
        .join("c")
}

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args(args)
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap()
}

fn invoke_app(socket: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(socket)
        .arg("--timeout")
        .arg("5000")
        .args(args)
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "zmux {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn id(value: &Value, key: &str) -> String {
    value[key].as_u64().unwrap().to_string()
}

#[test]
fn help_and_version_are_complete_and_offline() {
    let mut help = None;
    for arguments in [
        Vec::<&str>::new(),
        vec!["--help"],
        vec!["--timeout", "100"],
        vec!["--socket", "relative-path-must-not-be-read"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(arguments)
            .env("ZMUX_SOCKET", "relative-path-must-not-be-read")
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains("Usage: zmux"));
        help.get_or_insert(output);
    }
    let help = help.unwrap();
    for command in [
        "doctor",
        "status",
        "list-workspaces",
        "list-panes",
        "new-workspace",
        "rename-workspace",
        "focus-pane",
        "split-pane",
        "close-pane",
        "browser",
        "new-surface",
        "list-pane-surfaces",
        "select-surface",
        "close-surface",
        "rename-tab",
        "reorder-surface",
        "move-surface",
        "close-other-tabs",
        "close-workspace",
        "reopen-closed",
        "recently-closed",
        "notify",
        "list-notifications",
        "mark-notification-read",
        "open-notification",
        "dismiss-notification",
        "clear-notifications",
        "send-text",
        "read-screen",
        "prototype",
        "help",
    ] {
        assert!(help.contains(command), "top-level help omitted {command}");
        let output = invoke(&[command, "--help"]);
        assert!(
            output.status.success(),
            "{command} --help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    for (args, expected) in [
        (
            vec!["rename-workspace", "--bogus"],
            "unknown rename-workspace option",
        ),
        (
            vec!["focus-pane", "--pane", "1", "--pane", "2"],
            "repeated --pane",
        ),
        (vec!["focus-pane", "--pane", "pane:0"], "positive ID"),
        (
            vec!["notify", "--title", "t", "--body", "", "--body", "again"],
            "invalid or repeated notify option",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-socket-must-not-be-used"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "parser did not reject before connecting: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let version = invoke(&["--version"]);
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .starts_with("zmux ")
    );
}

#[test]
fn offline_topics_and_agent_discovery_do_not_require_a_valid_socket() {
    let offline = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-path-must-not-be-used"])
            .args(args)
            .env("ZMUX_SOCKET", "relative-path-must-not-be-used")
            .output()
            .unwrap()
    };
    let topic = offline(&["help", "navigate"]);
    assert!(topic.status.success());
    let topic = String::from_utf8(topic.stdout).unwrap();
    assert!(topic.contains("focus-pane"));
    assert!(topic.contains("move-surface"));
    assert!(!topic.contains("zmux prototype"));

    let guide = offline(&["guide"]);
    let alias = offline(&["--skill"]);
    assert!(guide.status.success());
    assert!(alias.status.success());
    assert_eq!(guide.stdout, alias.stdout);
    let guide = String::from_utf8(guide.stdout).unwrap();
    assert!(guide.contains("Never retry a timeout"));
    assert!(guide.contains("--expect-revision"));

    let catalog = offline(&["commands"]);
    assert!(catalog.status.success());
    let catalog: Value = serde_json::from_slice(&catalog.stdout).unwrap();
    assert_eq!(catalog["schema_version"], 1);
    assert_eq!(catalog["parity_status"], "incomplete");
    let guide = catalog["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "guide")
        .unwrap();
    assert_eq!(guide["socket_required"], false);
    assert_eq!(guide["aliases"], serde_json::json!(["--skill"]));

    let version = offline(&["version"]);
    assert!(version.status.success());
    assert_eq!(version.stdout, offline(&["--version"]).stdout);
    for args in [
        vec!["guide", "unexpected"],
        vec!["help", "nonexistent-topic"],
        vec!["browser", "bogus", "--help"],
        vec!["browser", "help", "unexpected"],
    ] {
        let rejected = offline(&args);
        assert!(!rejected.status.success());
        assert!(!String::from_utf8_lossy(&rejected.stderr).contains("cannot connect"));
    }
}

#[test]
fn send_text_delimiter_preserves_literal_stdin_marker_on_the_wire() {
    use std::process::Stdio;
    for (arguments, expected) in [
        (vec!["send-text", "--", "--stdin"], "--stdin"),
        (vec!["send-text", "--stdin"], "input from stdin"),
    ] {
        let socket = socket_path();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let capture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut body = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let response = serde_json::to_vec(&serde_json::json!({
                "version": 1, "request_id": request["request_id"], "ok": true, "result": {}
            }))
            .unwrap();
            stream
                .write_all(&(response.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&response).unwrap();
            request
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(&socket)
            .args(arguments)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"input from stdin")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let request = capture.join().unwrap();
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
        assert_eq!(request["command"]["request"]["operation"]["text"], expected);
    }
}

#[test]
fn public_cli_controls_a_real_desktop_over_its_unix_socket() {
    let socket = socket_path();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    assert_eq!(
        fs::metadata(socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let startup_deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let status = invoke_app(&socket, &["status"]);
        if status["workspace_count"] == 1 {
            break status;
        }
        assert!(
            Instant::now() < startup_deadline,
            "desktop never created its initial workspace"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status["workspace_count"], 1);
    assert_eq!(status["protocol_version"], 1);
    let capabilities = invoke_app(&socket, &["capabilities"]);
    assert_eq!(capabilities["protocol"], "zmux-local-control");
    assert_eq!(capabilities["version"], status["protocol_version"]);
    assert_eq!(capabilities["model_version"], status["model_version"]);
    assert_eq!(capabilities["authorization"], "same_uid");
    assert_eq!(capabilities["remote_relay"], false);
    for _ in 0..20 {
        let workspaces = invoke_app(&socket, &["list-workspaces"]);
        assert_eq!(workspaces["workspaces"].as_array().unwrap().len(), 1);
    }

    let rejected = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", "999999", "Missing"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("target_not_found")
    );

    let initial_catalog = invoke_app(&socket, &["list-workspaces"]);
    let initial_workspace_uuid = initial_catalog["targets"][0]["uuid"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = invoke_app(&socket, &["new-workspace", "CLI Workspace"]);
    let workspace_id = id(&created, "focused_workspace");
    let workspace_uuid = created["focused_workspace_uuid"]
        .as_str()
        .expect("created workspace must expose its stable UUID")
        .to_owned();
    let pane_uuid = created["focused_pane_uuid"]
        .as_str()
        .expect("created pane must expose its stable UUID")
        .to_owned();
    invoke_app(&socket, &["rename-workspace", &workspace_uuid, "By UUID"]);
    let wrong_kind = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", &pane_uuid, "Wrong target"])
        .output()
        .unwrap();
    assert!(!wrong_kind.status.success());
    assert!(String::from_utf8_lossy(&wrong_kind.stderr).contains("wrong_target_kind"));
    invoke_app(
        &socket,
        &[
            "rename-workspace",
            &format!("workspace:{workspace_id}"),
            "CLI Workspace",
        ],
    );
    let original_pane = id(&created, "focused_pane");
    let via_index = invoke_app(&socket, &["list-panes", "--workspace", "index:2"]);
    assert_eq!(id(&via_index["panes"][0], "pane_id"), original_pane);
    let inherited = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", "Caller workspace"])
        .env("ZMUX_WORKSPACE_ID", &initial_workspace_uuid)
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap();
    assert!(
        inherited.status.success(),
        "{}",
        String::from_utf8_lossy(&inherited.stderr)
    );
    let stale = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "rename-workspace",
            "--expect-revision",
            "18446744073709551615",
            &workspace_uuid,
            "Stale rename",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale_revision"));
    let foreign = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "focus-pane",
            "--workspace",
            &initial_workspace_uuid,
            &pane_uuid,
        ])
        .output()
        .unwrap();
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("target_scope_mismatch"));

    let workspaces = invoke_app(&socket, &["list-workspaces"]);
    assert!(
        workspaces["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["id"].as_u64().unwrap().to_string() == workspace_id
                    && item["name"] == "CLI Workspace"
            })
    );

    invoke_app(
        &socket,
        &["rename-workspace", &workspace_id, "Renamed by CLI"],
    );
    let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
    assert_eq!(panes["panes"].as_array().unwrap().len(), 1);

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
        if panes["panes"][0]["state"] == "running" {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "CLI pane never became ready"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let created_tab = invoke_app(&socket, &["new-surface", &original_pane]);
    let tab_id = id(&created_tab, "focused_surface");
    invoke_app(&socket, &["rename-tab", &tab_id, "CLI Tab"]);
    let tabs = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    let tabs = tabs["surfaces"].as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert!(tabs.iter().any(
        |tab| tab["surface_id"].as_u64().unwrap().to_string() == tab_id
            && tab["title"] == "CLI Tab"
            && tab["selected"] == true
    ));
    let first_tab_id = id(&tabs[0], "surface_id");
    invoke_app(
        &socket,
        &[
            "notify",
            "--surface",
            &tab_id,
            "--title",
            "CLI attention",
            "--body",
            "Review requested",
        ],
    );
    let notifications = invoke_app(&socket, &["list-notifications"]);
    let notification_id = id(&notifications["notifications"][0], "id");
    assert_eq!(
        notifications["notifications"][0]["body"],
        "Review requested"
    );
    assert_eq!(notifications["unread"], 1);
    invoke_app(&socket, &["open-notification", "--id", &notification_id]);
    let notifications = invoke_app(&socket, &["list-notifications"]);
    assert_eq!(notifications["unread"], 0);
    invoke_app(&socket, &["dismiss-notification", "--id", &notification_id]);
    assert!(
        invoke_app(&socket, &["list-notifications"])["notifications"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    invoke_app(&socket, &["reorder-surface", &tab_id, "0"]);
    let reordered = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    assert_eq!(id(&reordered["surfaces"][0], "surface_id"), tab_id);
    invoke_app(&socket, &["select-surface", &first_tab_id]);
    invoke_app(&socket, &["close-surface", &tab_id]);
    let closed = invoke_app(&socket, &["recently-closed"]);
    assert_eq!(closed["closed_items"].as_array().unwrap().len(), 1);
    let history_id = id(&closed["closed_items"][0], "id");
    invoke_app(&socket, &["reopen-closed", &history_id]);
    let restore_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let closed = invoke_app(&socket, &["recently-closed"]);
        if closed["closed_items"].as_array().unwrap().is_empty() {
            break;
        }
        assert!(Instant::now() < restore_deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let restored = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    assert_eq!(restored["surfaces"].as_array().unwrap().len(), 2);
    let restored_surface = restored["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| id(surface, "surface_id") != first_tab_id)
        .expect("reopening creates a fresh tab alongside the surviving original");
    let restored_id = id(restored_surface, "surface_id");
    assert_ne!(restored_id, tab_id);
    invoke_app(&socket, &["close-surface", &restored_id]);

    let split = invoke_app(&socket, &["split-pane", &original_pane, "horizontal"]);
    let new_pane = id(&split, "focused_pane");
    invoke_app(&socket, &["focus-pane", &original_pane]);
    invoke_app(
        &socket,
        &[
            "send-text",
            &original_pane,
            "printf '\\nZMUX_CLI_SOCKET_OK\\n'\r",
        ],
    );

    invoke_app(
        &socket,
        &[
            "send-text",
            &original_pane,
            "printf '\\nCONTEXT_W=%s\\nCONTEXT_S=%s\\n' \"$ZMUX_WORKSPACE_ID\" \"$ZMUX_SURFACE_ID\"\r",
        ],
    );
    let output_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = invoke_app(&socket, &["read-screen", &original_pane]);
        if screen["contents"].as_str().is_some_and(|contents| {
            contents.contains("ZMUX_CLI_SOCKET_OK")
                && contents.contains(&format!("CONTEXT_W={workspace_uuid}"))
                && contents.contains(&format!(
                    "CONTEXT_S={}",
                    created["focused_surface_uuid"].as_str().unwrap()
                ))
        }) {
            break;
        }
        assert!(
            Instant::now() < output_deadline,
            "CLI input/context did not reach the real terminal: {screen}; expected workspace {workspace_uuid}, surface {}",
            created["focused_surface_uuid"]
        );
        thread::sleep(Duration::from_millis(10));
    }

    let browser = invoke_app(&socket, &["browser", "open", &new_pane, "about:blank"]);
    let browser_id = id(&browser, "focused_surface");
    let surfaces = invoke_app(&socket, &["list-pane-surfaces", &new_pane]);
    let browser = surfaces["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "browser")
        .unwrap();
    assert!(browser["session_id"].is_null());
    invoke_app(
        &socket,
        &["browser", "navigate", &browser_id, "https://example.com/"],
    );
    let surfaces = invoke_app(&socket, &["list-pane-surfaces", &new_pane]);
    assert!(
        surfaces["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["url"] == "https://example.com/")
    );
    let browser_uuid = browser["surface_uuid"].as_str().unwrap();
    for command in [
        serde_json::json!({"method":"send_text", "pane_id":new_pane.parse::<u64>().unwrap(), "text":"must not reach a browser"}),
        serde_json::json!({"method":"targeted", "request": {"scope":{"surface":browser_uuid}, "operation":{"operation":"send_text","text":"must not reach a browser"}}}),
    ] {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"request_id":90001,"command":command}),
        )
        .unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut body = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut body).unwrap();
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(
            response["error"]["code"], "not_terminal",
            "browser rejection lost its typed error: {response}"
        );
    }
    let rejected_browser_input = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "send-text",
            "--surface",
            browser_uuid,
            "must not reach a browser",
        ])
        .output()
        .unwrap();
    assert!(!rejected_browser_input.status.success());
    assert!(String::from_utf8_lossy(&rejected_browser_input.stderr).contains("not_terminal"));
    invoke_app(&socket, &["close-pane", &new_pane]);
    let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
    assert_eq!(panes["panes"].as_array().unwrap().len(), 1);

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let replacement = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let replacement_server = ControlServer::start(&socket, Arc::clone(&replacement)).unwrap();
    let replaced = invoke_app(&socket, &["list-workspaces"]);
    // Startup is asynchronous; wait for the production owner's initial workspace.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut replaced = replaced;
    while replaced["workspaces"].as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
        replaced = invoke_app(&socket, &["list-workspaces"]);
    }
    assert_eq!(replaced["workspaces"][0]["id"], 1);
    let stale_uuid = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", &initial_workspace_uuid, "Wrong app"])
        .output()
        .unwrap();
    assert!(!stale_uuid.status.success());
    assert!(String::from_utf8_lossy(&stale_uuid.stderr).contains("target_not_found"));
    replacement_server.shutdown(Duration::from_secs(2)).unwrap();
    replacement.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}
