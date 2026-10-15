//! Public session commands use real socket requests and isolated persistence files.
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Keep sockaddr_un paths short on macOS as well as Linux.
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            PathBuf::from("/tmp").join(format!("zp{}-{nonce:x}-{sequence:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("session fixture cleanup: {error}");
        }
    }
}

fn client(socket: &Path, directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
    command
        .arg("--socket")
        .arg(socket)
        .current_dir(directory)
        .env("HOME", directory)
        .env_remove("ZMUX_SOCKET_PASSWORD")
        .env_remove("ZMUX_SOCKET_PASSWORD_FILE")
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID");
    command
}

fn invoke(socket: &Path, args: &[&str], directory: &Path) -> Output {
    client(socket, directory).args(args).output().unwrap()
}

fn json(socket: &Path, args: &[&str], directory: &Path) -> Value {
    let output = invoke(socket, args, directory);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn completed(socket: &Path, admission: &Value, directory: &Path) -> Value {
    assert_eq!(admission["state"], "pending", "{admission}");
    assert!(admission["revision"].is_u64(), "{admission}");
    let id = admission["operation_id"].as_u64().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let receipt = json(socket, &["session-status", "--id", &id], directory);
        if receipt["state"] != "pending" {
            return receipt;
        }
        assert!(
            Instant::now() < deadline,
            "operation stayed pending: {receipt}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn session_help_is_offline_and_malformed_options_fail_before_connecting() {
    let fixture = Fixture::new();
    let invalid_socket = Path::new("relative-unused-socket");
    for command in ["restore-session", "save-session", "session-status"] {
        let output = invoke(invalid_socket, &[command, "--help"], &fixture.0);
        assert!(output.status.success(), "{command} help required a socket");
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Usage:"));
        assert!(help.contains("asynchronous") || help.contains("pending"));
    }
    for (args, expected) in [
        (vec!["restore-session", "--from"], "--from path"),
        (vec!["restore-session", "--export"], "--export path"),
        (vec!["restore-session", "--from", ""], "nonempty path"),
        (
            vec!["restore-session", "--export", "--force"],
            "nonempty path",
        ),
        (
            vec!["restore-session", "--from", "one", "--from", "two"],
            "repeated --from",
        ),
        (
            vec!["restore-session", "--export", "one", "--export", "two"],
            "repeated --export",
        ),
        (
            vec!["restore-session", "--from", "one", "--export", "two"],
            "conflicts",
        ),
        (
            vec!["restore-session", "--force"],
            "--force requires --export",
        ),
        (
            vec!["restore-session", "--export", "one", "--force", "--force"],
            "repeated --force",
        ),
        (
            vec!["restore-session", "--unknown"],
            "unknown restore-session",
        ),
        (vec!["restore-session", "one"], "unknown restore-session"),
        (
            vec!["restore-session", "--workspace", "1"],
            "unknown restore-session",
        ),
        (vec!["session-status", "--id"], "--id operation ID"),
        (vec!["session-status", "--id", "0"], "positive integer"),
        (vec!["session-status", "--id", "word"], "positive integer"),
        (
            vec!["session-status", "--id", "1", "--id", "2"],
            "repeated --id",
        ),
        (vec!["session-status", "1"], "unknown session-status"),
        (
            vec!["session-status", "--unknown"],
            "unknown session-status",
        ),
        (vec!["save-session", "--force"], "accepts no arguments"),
    ] {
        let output = invoke(invalid_socket, &args, &fixture.0);
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{args:?} did not fail at parsing: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn session_cli_reports_real_export_save_and_restore_completion() {
    let fixture = Fixture::new();
    let state = fixture.0.join("state");
    let socket = fixture.0.join("control.sock");
    let desktop = Arc::new(
        Desktop::with_persistence(
            Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: Some(fixture.0.clone()),
            },
            state.clone(),
        )
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = desktop.snapshot().unwrap();
        if !snapshot.persistence.startup_pending
            && snapshot.pending_launches == 0
            && !snapshot.model.workspaces.is_empty()
            && snapshot.control_frames.iter().any(|frame| {
                frame.snapshot.as_ref().is_some_and(|terminal| {
                    matches!(terminal.state, zmux_session_runtime::State::Running)
                })
            })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "initial shell did not start: {}",
            serde_json::to_string(&*snapshot).unwrap()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let initial = json(&socket, &["list-workspaces"], &fixture.0);
    let workspace = initial["targets"][0]["uuid"].as_str().unwrap();
    json(
        &socket,
        &["rename-workspace", workspace, "Saved workspace"],
        &fixture.0,
    );

    // Paths are resolved by the CLI. Shell syntax and spaces remain filename bytes.
    let export_name = "saved ' $(touch injected) session.json";
    let output = client(&socket, &fixture.0)
        .args(["restore-session", "--export", export_name])
        .env("ZMUX_WORKSPACE_ID", "stale-workspace")
        .env("ZMUX_SURFACE_ID", "stale-surface")
        .env("ZMUX_TAB_ID", "stale-tab")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "session export used caller targets: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let admission = serde_json::from_slice(&output.stdout).unwrap();
    let receipt = completed(&socket, &admission, &fixture.0);
    assert_eq!(receipt["state"], "completed", "{receipt}");
    assert_eq!(receipt["kind"], "export");
    let export = fixture.0.join(export_name);
    let original = fs::read(&export).unwrap();
    assert_eq!(fs::metadata(&export).unwrap().mode() & 0o777, 0o600);
    assert!(!fixture.0.join("injected").exists());

    let collision = json(
        &socket,
        &["restore-session", "--export", export_name],
        &fixture.0,
    );
    let failed = completed(&socket, &collision, &fixture.0);
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(failed["error"].as_str().unwrap().contains("exists"));
    assert_eq!(fs::read(&export).unwrap(), original);

    json(
        &socket,
        &["rename-workspace", workspace, "Updated workspace"],
        &fixture.0,
    );
    let forced = json(
        &socket,
        &["restore-session", "--force", "--export", export_name],
        &fixture.0,
    );
    assert_eq!(
        completed(&socket, &forced, &fixture.0)["state"],
        "completed"
    );
    let updated = fs::read(&export).unwrap();
    assert_ne!(updated, original);
    let save = json(&socket, &["save-session"], &fixture.0);
    assert_eq!(completed(&socket, &save, &fixture.0)["state"], "completed");
    assert_eq!(fs::read(state.join("session.json")).unwrap(), updated);

    let refused = invoke(
        &socket,
        &["restore-session", "--from", export_name],
        &fixture.0,
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("empty desktop"));
    json(&socket, &["close-workspace", workspace], &fixture.0);
    let restored = json(
        &socket,
        &["restore-session", "--from", export_name],
        &fixture.0,
    );
    let receipt = completed(&socket, &restored, &fixture.0);
    assert_eq!(receipt["state"], "completed", "{receipt}");
    let restored = json(&socket, &["list-workspaces"], &fixture.0);
    assert_eq!(restored["targets"][0]["uuid"], workspace);
    assert_eq!(restored["workspaces"][0]["name"], "Updated workspace");
    let pane = restored["workspaces"][0]["root"]["pane"]["id"]
        .as_u64()
        .unwrap()
        .to_string();
    let screen = json(&socket, &["read-screen", "--pane", &pane], &fixture.0);
    assert_eq!(screen["state"], "running");

    let unknown = invoke(
        &socket,
        &["session-status", "--id", "18446744073709551615"],
        &fixture.0,
    );
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("operation_not_found"));

    server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let before = json(&socket, &["session-status"], &fixture.0);
    for args in [
        vec!["save-session"],
        vec!["restore-session", "--from", export_name],
        vec!["restore-session", "--export", export_name, "--force"],
    ] {
        let denied = invoke(&socket, &args, &fixture.0);
        assert!(!denied.status.success(), "read-only mode admitted {args:?}");
        assert!(
            String::from_utf8_lossy(&denied.stderr).contains("read-only"),
            "session action was not denied at authorization: {}",
            String::from_utf8_lossy(&denied.stderr)
        );
    }
    let after = json(&socket, &["session-status"], &fixture.0);
    assert_eq!(after["operations"], before["operations"]);
    assert_eq!(fs::read(&export).unwrap(), updated);
    assert_eq!(
        json(
            &socket,
            &[
                "session-status",
                "--id",
                &receipt["operation_id"].to_string()
            ],
            &fixture.0,
        )["state"],
        "completed"
    );
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
