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
    let portable = fs::read_to_string(root.0.join("state/session.json")).unwrap();
    assert!(
        !portable.contains(message["id"].as_str().unwrap()),
        "portable session copied inbox state"
    );
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

#[test]
fn cli_message_literals_and_rejections_preserve_exact_inbox_contents() {
    let root = Directory::new();
    let app = App::open(&root.0);
    let initial = desktop_snapshot::read(&app.desktop);
    let surface = initial.model.workspaces[0].root.panes()[0].selected_surface;
    let target = initial
        .model
        .object_uuid(TargetKind::Surface, surface.0)
        .to_string();
    let literal = app.call(&["agent", "msg", &target, "--from", "--help", "--", "--help"]);
    assert_eq!(literal["message"]["body"], "--help");
    assert_eq!(literal["message"]["sender"]["name"], "--help");
    let uuid = uuid::Uuid::new_v4().to_string();
    for args in [
        vec!["agent", "message", target.as_str()],
        vec![
            "agent",
            "message",
            target.as_str(),
            "--request-id",
            uuid.as_str(),
            "--request-id",
            uuid.as_str(),
            "bad",
        ],
        vec![
            "agent",
            "message",
            target.as_str(),
            "--from",
            "a",
            "--from",
            "b",
            "bad",
        ],
        vec!["agent", "inbox", "--limit", "33"],
        vec!["agent", "messages", "off", "--global", target.as_str()],
    ] {
        let output = app.output(&args);
        assert!(
            !output.status.success(),
            "invalid grammar succeeded: {args:?}"
        );
    }
    for bytes in [vec![b'x'; 4097], vec![0xff, 0xfe], Vec::new()] {
        let mut child = app
            .command(&["agent", "message", &target, "-"], None)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        assert!(!child.wait_with_output().unwrap().status.success());
    }
    let rows = app.call(&["agent", "inbox"]);
    assert_eq!(rows["messages"].as_array().unwrap().len(), 1);
    assert_eq!(rows["messages"][0]["id"], literal["message"]["id"]);
    assert!(!root.0.join("terminal-input-counter").exists());
}

fn rpc(
    app: &App,
    command: desktop_runtime::control::ControlCommand,
) -> desktop_runtime::control::ControlResponse {
    use std::io::Read;
    let mut stream = std::os::unix::net::UnixStream::connect(&app.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let request = desktop_runtime::control::ControlRequest {
        version: desktop_runtime::control::CONTROL_VERSION,
        request_id: 42,
        password: None,
        command,
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).unwrap();
    let mut response = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut response).unwrap();
    serde_json::from_slice(&response).unwrap()
}
fn agent_operation(
    app: &App,
    action: desktop_runtime::agent_messages::MessageAction,
) -> (uuid::Uuid, Value) {
    use desktop_runtime::{agent_messages::MessageRequest, control::ControlCommand};
    let id = uuid::Uuid::new_v4();
    let response = rpc(
        app,
        ControlCommand::AgentMessages {
            request: MessageRequest {
                request_id: id,
                caller: Default::default(),
                action,
            },
        },
    );
    assert!(response.ok, "{:?}", response.error);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut receipt = response.result.unwrap();
    while receipt["state"] == "pending" {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
        let response = rpc(app, ControlCommand::AgentMessagesStatus { request_id: id });
        assert!(response.ok, "{:?}", response.error);
        receipt = response.result.unwrap();
    }
    assert_eq!(receipt["state"], "completed", "{receipt}");
    (id, receipt["result"].clone())
}
#[test]
fn read_only_socket_can_list_but_cannot_obtain_consumer_capabilities_or_mutate() {
    use desktop_runtime::{
        agent_messages::MessageAction,
        control::ControlCommand,
        control_policy::{ControlAccess, ControlPolicy},
    };
    let root = Directory::new();
    let app = App::open(&root.0);
    let initial = desktop_snapshot::read(&app.desktop);
    let surface = initial.model.workspaces[0].root.panes()[0].selected_surface;
    let target = initial
        .model
        .object_uuid(TargetKind::Surface, surface.0)
        .to_string();
    app.call(&["agent", "message", &target, "queued"]);
    let (operation, registration) = agent_operation(
        &app,
        MessageAction::Register {
            surface: target.clone(),
            provider: "neutral".into(),
            interactive: true,
        },
    );
    let token = registration["token"].as_str().unwrap();
    app.server
        .as_ref()
        .unwrap()
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let rows = app.call(&["agent", "inbox"]);
    assert_eq!(rows["messages"][0]["state"], "queued");
    assert!(
        !app.output(&["agent", "message", &target, "denied"])
            .status
            .success()
    );
    assert!(
        !app.output(&["agent", "inbox", "--mark-read"])
            .status
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = rpc(
            &app,
            ControlCommand::AgentMessagesStatus {
                request_id: operation,
            },
        );
        let text = serde_json::to_string(&response).unwrap();
        assert!(!text.contains(token));
        if !response.ok
            || response
                .result
                .as_ref()
                .is_some_and(|r| r["state"] == "failed")
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!root.0.join("terminal-input-counter").exists());
}

#[test]
fn explicit_cli_retry_preserves_unknown_message_identity_without_terminal_input() {
    use desktop_runtime::agent_messages::{MessageAction, ProviderExposure};
    let root = Directory::new();
    let app = App::open(&root.0);
    let snapshot = desktop_snapshot::read(&app.desktop);
    let surface = snapshot
        .model
        .object_uuid(
            TargetKind::Surface,
            snapshot.model.workspaces[0].root.panes()[0].surfaces[0]
                .id
                .0,
        )
        .to_string();
    let sent = app.call(&["agent", "message", &surface, "retry me once"]);
    let (_, registered) = agent_operation(
        &app,
        MessageAction::Register {
            surface: surface.clone(),
            provider: "synthetic-provider".into(),
            interactive: true,
        },
    );
    let make_claim = || MessageAction::Claim {
        surface: surface.clone(),
        consumer: registered["consumer"].as_str().unwrap().parse().unwrap(),
        token: registered["token"].as_str().unwrap().into(),
        max_messages: Some(1),
        exposure: Some(ProviderExposure {
            provider: "claude".into(),
            provider_version: "2.1.301".into(),
            runtime_id: uuid::Uuid::new_v4(),
            adapter_generation: uuid::Uuid::new_v4(),
            session_id: "synthetic-root".into(),
            prompt_id: uuid::Uuid::new_v4().to_string(),
        }),
    };
    let (_, exposed) = agent_operation(&app, make_claim());
    let message_id = sent["message"]["id"].as_str().unwrap();
    assert_eq!(exposed["messages"][0]["id"], message_id);
    let denied = app.output(&["agent", "retry", "--surface", &surface, message_id]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("--accept-duplicate"));
    let result = app.call(&[
        "agent",
        "retry",
        "--surface",
        &surface,
        "--accept-duplicate",
        message_id,
    ]);
    assert_eq!(result["possible_duplicate"], true);
    let (_, retried) = agent_operation(&app, make_claim());
    assert_eq!(retried["messages"][0]["id"], message_id);
    assert_eq!(
        retried["messages"][0]["delivery_attempts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let inbox = app.call(&["agent", "inbox", "--surface", &surface]);
    assert_eq!(inbox["messages"][0]["state"], "queued");
    assert!(!root.0.join("terminal-input-counter").exists());
}
