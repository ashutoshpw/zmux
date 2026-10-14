use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
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
    let version = invoke(&["--version"]);
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .starts_with("zmux ")
    );
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
            .contains("command_rejected")
    );

    let created = invoke_app(&socket, &["new-workspace", "CLI Workspace"]);
    let workspace_id = id(&created, "focused_workspace");
    let original_pane = id(&created, "focused_pane");
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
    let restored_id = id(&restored["surfaces"][1], "surface_id");
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

    let output_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = invoke_app(&socket, &["read-screen", &original_pane]);
        if screen["contents"]
            .as_str()
            .is_some_and(|contents| contents.contains("ZMUX_CLI_SOCKET_OK"))
        {
            break;
        }
        assert!(
            Instant::now() < output_deadline,
            "CLI input did not reach the real terminal"
        );
        thread::sleep(Duration::from_millis(10));
    }

    invoke_app(&socket, &["close-pane", &new_pane]);
    let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
    assert_eq!(panes["panes"].as_array().unwrap().len(), 1);

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}
