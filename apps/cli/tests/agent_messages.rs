//! Durable agent messages through the shipped CLI, real socket and actual PTYs.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};
use zmux_workspace_core::targets::TargetKind;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("am{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct App {
    desktop: Arc<Desktop>,
    server: Option<ControlServer>,
    socket: PathBuf,
}
impl App {
    fn open(root: &Path) -> Self {
        let desktop = Arc::new(
            Desktop::with_persistence(
                Shell {
                    program: "/bin/sh".into(),
                    args: vec![
                        "-c".into(),
                        "printf 'AGENT_MESSAGE_PTY_READY\\n'; while IFS= read -r line; do printf '%s\\n' \"$line\" >> terminal-input-counter; done".into(),
                    ],
                    cwd: Some(root.into()),
                },
                root.join("state"),
            )
            .unwrap(),
        );
        let socket = root.join("socket/c");
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let app = Self {
            desktop,
            server: Some(server),
            socket,
        };
        app.wait_terminals(0);
        app
    }
    fn wait_terminals(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let snapshot = desktop_snapshot::read_until(&self.desktop, deadline);
            if !snapshot.persistence.startup_pending
                && !snapshot.persistence.restoring
                && snapshot.pending_launches == 0
                && !snapshot.control_frames.is_empty()
                && (count == 0 || snapshot.control_frames.len() == count)
                && snapshot.control_frames.iter().all(|f| {
                    f.snapshot
                        .as_ref()
                        .is_some_and(|s| s.viewport.contents.contains("AGENT_MESSAGE_PTY_READY"))
                })
            {
                return;
            }
            assert!(Instant::now() < deadline, "actual PTY fixture not ready");
            thread::sleep(Duration::from_millis(3));
        }
    }
    fn command(&self, args: &[&str], caller: Option<(&str, &str)>) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_zmux"));
        c.arg("--socket")
            .arg(&self.socket)
            .args(["--timeout", "5000"])
            .args(args);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                c.env_remove(name);
            }
        }
        if let Some((workspace, surface)) = caller {
            c.env("ZMUX_WORKSPACE_ID", workspace)
                .env("ZMUX_SURFACE_ID", surface);
        }
        c
    }
    fn output(&self, args: &[&str]) -> Output {
        self.command(args, None).output().unwrap()
    }
    fn call(&self, args: &[&str]) -> Value {
        decoded(self.output(args), args)
    }
    fn stop(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown(Duration::from_secs(3)).unwrap();
            self.desktop.shutdown(Duration::from_secs(5)).unwrap();
        }
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.stop();
    }
}
fn decoded(output: Output, args: &[&str]) -> Value {
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn durable_queued_message_survives_restart_without_terminal_input() {
    let root = Directory::new();
    let mut app = App::open(&root.0);
    let initial = desktop_snapshot::read(&app.desktop);
    let recipient = &initial.model.workspaces[0].root.panes()[0].surfaces[0];
    let recipient_uuid = initial
        .model
        .object_uuid(TargetKind::Surface, recipient.id.0)
        .to_string();
    let old_session = recipient.session_id.unwrap();
    let sender = app.call(&["new-workspace", "message sender"]);
    let sender_workspace = sender["focused_workspace_uuid"].as_str().unwrap();
    let sender_surface = sender["focused_surface_uuid"].as_str().unwrap();
    app.wait_terminals(2);
    let request_id = uuid::Uuid::new_v4().to_string();
    let args = [
        "agent",
        "message",
        &recipient_uuid,
        "--request-id",
        &request_id,
        "--",
        "-",
    ];
    let body = "  durable α界 e\u{301}🙂\t\nsecond line  \n";
    let mut child = app
        .command(&args, Some((sender_workspace, sender_surface)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let sent = decoded(child.wait_with_output().unwrap(), &args);
    assert_eq!(sent["durability"], "saved");
    assert_eq!(sent["recipient_has_agent"], false);
    let message = &sent["message"];
    assert_eq!(message["request_id"], request_id);
    assert!(uuid::Uuid::parse_str(message["id"].as_str().unwrap()).is_ok());
    assert_eq!(message["recipient"]["surface_id"], recipient_uuid);
    assert_eq!(message["sender"]["surface_id"], sender_surface);
    assert_eq!(message["sender"]["workspace_id"], sender_workspace);
    assert_eq!(message["sender"]["name"], "message sender");
    assert_eq!(message["body"], body);
    assert_eq!(message["state"], "queued");
    let inbox = app.call(&["agent", "inbox", "--surface", &recipient_uuid]);
    assert_eq!(inbox["messages"], serde_json::json!([message]));
    assert!(!root.0.join("terminal-input-counter").exists());
    let save = app.call(&["save-session"]);
    let operation = save["operation_id"].as_u64().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let receipt = app.call(&["session-status", "--id", &operation]);
        if receipt["state"] == "completed" {
            break;
        }
        assert_eq!(receipt["state"], "pending", "{receipt}");
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(3));
    }
    app.stop();
    let reopened = App::open(&root.0);
    reopened.wait_terminals(2);
    let restored = desktop_snapshot::read(&reopened.desktop);
    let target = restored
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .find(|s| {
            restored
                .model
                .object_uuid(TargetKind::Surface, s.id.0)
                .to_string()
                == recipient_uuid
        })
        .unwrap();
    assert_ne!(target.session_id, Some(old_session));
    let inbox = reopened.call(&["agent", "inbox", "--surface", &recipient_uuid]);
    assert_eq!(inbox["messages"], serde_json::json!([message]));
    assert!(!root.0.join("terminal-input-counter").exists());
}
