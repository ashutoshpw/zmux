//! Workspace metadata contracts through the shipped CLI and real control server.
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};
struct App {
    desktop: Arc<Desktop>,
    server: ControlServer,
    socket: PathBuf,
}
impl App {
    fn new() -> Self {
        let socket = std::env::temp_dir()
            .join(format!(
                "zm{:x}{:x}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("c");
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: None,
            })
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let app = Self {
            desktop,
            server,
            socket,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.run(&["list-workspaces"])["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        app
    }
    fn output(&self, args: &[&str], caller: Option<(&str, &str)>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_zmux"));
        c.arg("--socket")
            .arg(&self.socket)
            .args(args)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .env_remove("ZMUX_SOCKET_PASSWORD_FILE");
        if let Some((w, s)) = caller {
            c.env("ZMUX_WORKSPACE_ID", w).env("ZMUX_SURFACE_ID", s);
        }
        c.output().unwrap()
    }
    fn call(&self, args: &[&str], caller: Option<(&str, &str)>) -> Value {
        let o = self.output(args, caller);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn run(&self, args: &[&str]) -> Value {
        self.call(args, None)
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.server.shutdown(Duration::from_secs(2)).unwrap();
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
        fs::remove_dir(self.socket.parent().unwrap()).unwrap();
    }
}
fn id(v: &Value, key: &str) -> String {
    v[key].as_u64().unwrap().to_string()
}
#[test]
fn metadata_channels_target_the_caller_workspace_and_preserve_foreground_focus() {
    let a = App::new();
    let first = a.run(&["new-workspace", "metadata owner"]);
    let w = id(&first, "focused_workspace");
    let s = id(&first, "focused_surface");
    let second = a.run(&["new-workspace", "foreground"]);
    let foreground = id(&second, "focused_workspace");
    a.call(
        &[
            "set-status",
            "build",
            "compiling",
            "--icon",
            "hammer",
            "--color",
            "#ff9500",
            "--priority",
            "80",
        ],
        Some((&w, &s)),
    );
    let initial = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(initial["statuses"][0]["icon"], "hammer");
    assert_eq!(initial["statuses"][0]["color"], "#ff9500");
    a.run(&["set-status", "deploy", "waiting", "--workspace", &w]);
    a.run(&[
        "set-status",
        "build",
        "done",
        "--workspace",
        &w,
        "--priority",
        "80",
    ]);
    let status = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(status["workspace_id"].as_u64().unwrap().to_string(), w);
    assert_eq!(status["statuses"][0]["key"], "build");
    assert_eq!(status["statuses"][0]["value"], "done");
    assert_eq!(status["statuses"].as_array().unwrap().len(), 2);
    assert!(
        a.run(&["list-status"])["statuses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    a.run(&[
        "set-progress",
        "0.5",
        "--workspace",
        &w,
        "--label",
        "Building",
    ]);
    assert_eq!(
        a.run(&["list-status", "--workspace", &w])["progress"]["value"],
        0.5
    );
    for message in ["first", "second", "third"] {
        a.run(&[
            "log",
            message,
            "--workspace",
            &w,
            "--level",
            "success",
            "--source",
            "build",
        ]);
    }
    let log = a.run(&["list-log", "--workspace", &w, "--limit", "2"]);
    assert_eq!(
        log["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );
    assert_eq!(log["logs"][0]["level"], "success");
    assert_eq!(
        id(&a.run(&["list-workspaces"]), "focused_workspace"),
        foreground
    );
    a.run(&["new-surface", &id(&first, "focused_pane")]);
    a.run(&["move-surface", &s, &id(&second, "focused_pane"), "1"]);
    assert_eq!(
        a.run(&["list-status", "--workspace", &w])["statuses"][0]["key"],
        "build"
    );
    assert!(
        a.run(&["list-status", "--workspace", &foreground])["statuses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    a.run(&["clear-status", "build", "--workspace", &w]);
    a.run(&["clear-progress", "--workspace", &w]);
    a.run(&["clear-log", "--workspace", &w]);
    let state = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(state["statuses"].as_array().unwrap().len(), 1);
    assert!(state["progress"].is_null());
    assert!(state["logs"].as_array().unwrap().is_empty());
}
#[test]
fn metadata_rejects_bad_values_and_stale_or_wrong_owner_context_without_mutation() {
    let a = App::new();
    let first = a.run(&["new-workspace", "owner"]);
    let w = id(&first, "focused_workspace");
    let other = a.run(&["new-workspace", "other"]);
    let s = id(&other, "focused_surface");
    a.run(&["set-status", "safe", "unchanged", "--workspace", &w]);
    for args in [
        vec!["set-progress", "NaN"],
        vec!["set-progress", "1.01"],
        vec!["set-progress", "-0.1"],
        vec!["set-status", "x", "y", "--color", "red"],
        vec!["set-status", "x", "y", "--priority", "bad"],
        vec!["log", "bad", "--level", "fatal"],
        vec!["list-log", "--limit", "0"],
        vec!["list-status", "--workspace", &w, "--workspace", &w],
    ] {
        assert!(!a.output(&args, None).status.success(), "{args:?}");
    }
    assert!(
        !a.output(&["set-status", "bad", "owner"], Some((&w, &s)))
            .status
            .success()
    );
    assert!(
        !a.output(&["clear-status", "safe", "--workspace", "999999"], None)
            .status
            .success()
    );
    let rows = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(rows["statuses"].as_array().unwrap().len(), 1);
    assert_eq!(rows["statuses"][0]["value"], "unchanged");
}
#[test]
fn readonly_socket_can_inspect_metadata_but_cannot_change_any_channel() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let a = App::new();
    a.run(&["set-status", "build", "preserved"]);
    a.run(&["log", "preserved"]);
    a.server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    assert_eq!(a.run(&["list-status"])["statuses"][0]["value"], "preserved");
    assert_eq!(a.run(&["list-log"])["logs"][0]["message"], "preserved");
    for args in [
        vec!["set-status", "x", "y"],
        vec!["clear-status", "build"],
        vec!["set-progress", "0.5"],
        vec!["clear-progress"],
        vec!["log", "denied"],
        vec!["clear-log"],
    ] {
        let o = a.output(&args, None);
        assert!(!o.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("read-only"));
    }
    assert_eq!(a.run(&["list-status"])["statuses"][0]["value"], "preserved");
    assert_eq!(a.run(&["list-log"])["logs"].as_array().unwrap().len(), 1);
}

#[test]
fn escaped_metadata_budget_rejection_keeps_accepted_records_readable() {
    use zmux_desktop_runtime::{
        Command as NativeCommand, Request,
        metadata::{
            LogLevel, MAX_LOG_ENTRIES, MAX_LOG_MESSAGE_BYTES, MAX_NAME_BYTES, MAX_RETAINED_BYTES,
            MAX_STATUS_ENTRIES, MAX_TEXT_BYTES, MetadataAction,
        },
        targets::{TargetOperation, TargetRequest},
    };
    let a = App::new();
    let mut next = 0;
    let mut send = |action| {
        next += 1;
        a.desktop
            .submit(Request {
                version: 1,
                request_id: next,
                payload: NativeCommand::Targeted {
                    request: TargetRequest {
                        operation: TargetOperation::Metadata { action },
                        ..Default::default()
                    },
                },
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(s) = a.desktop.snapshot()
                && s.last_request_id == next
            {
                break s;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    };
    for n in 0..MAX_STATUS_ENTRIES {
        let result = send(MetadataAction::SetStatus {
            key: format!("{n:03}{}", "\\".repeat(MAX_NAME_BYTES - 3)),
            value: "\\".repeat(MAX_TEXT_BYTES),
            icon: Some("\\".repeat(MAX_NAME_BYTES)),
            color: Some("#abcdef".into()),
            priority: i32::MIN,
        });
        assert!(result.last_error.is_none(), "{:?}", result.last_error);
    }
    let progress = send(MetadataAction::SetProgress {
        value: 0.5,
        label: Some("\\".repeat(MAX_TEXT_BYTES)),
    });
    assert!(progress.last_error.is_none());
    let mut accepted = 0;
    let mut rejected = false;
    for _ in 0..MAX_LOG_ENTRIES {
        let before = a.desktop.snapshot().unwrap();
        let result = send(MetadataAction::Log {
            message: "\\".repeat(MAX_LOG_MESSAGE_BYTES),
            level: LogLevel::Progress,
            source: Some("\\".repeat(MAX_NAME_BYTES)),
        });
        if let Some(error) = &result.last_error {
            assert!(error.contains("byte capacity"), "{error}");
            assert_eq!(
                serde_json::to_value(&result.workspace_metadata).unwrap(),
                serde_json::to_value(&before.workspace_metadata).unwrap()
            );
            rejected = true;
            break;
        }
        accepted += 1;
        assert!(
            serde_json::to_vec(&result.workspace_metadata)
                .unwrap()
                .len()
                <= MAX_RETAINED_BYTES
        );
    }
    assert!(rejected && accepted > 0 && accepted < MAX_LOG_ENTRIES);
    let listed = a.run(&["list-log"]);
    assert_eq!(listed["logs"].as_array().unwrap().len(), accepted);
    assert_eq!(
        listed["statuses"].as_array().unwrap().len(),
        MAX_STATUS_ENTRIES
    );
    assert_eq!(
        listed["logs"][0]["message"].as_str().unwrap().len(),
        MAX_LOG_MESSAGE_BYTES
    );
}
