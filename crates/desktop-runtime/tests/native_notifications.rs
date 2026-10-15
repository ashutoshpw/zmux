//! Notification ownership is verified at the real desktop executor and persistence boundary.
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{
    Command, Desktop, Request, Shell, Snapshot,
    attention::{
        AttentionCommand, NativeAttentionAction, NativeNotificationEvent,
        NotificationAuthorization, NotificationDestination,
    },
    control::{CONTROL_VERSION, ControlServer},
    control_policy::{ControlAccess, ControlPolicy},
    sessions::{OperationState, SessionCommand},
};
use zmux_workspace_core::Action;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            PathBuf::from("/tmp").join(format!("zn{}-{nonce:x}-{sequence:x}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn desktop(&self) -> Desktop {
        Desktop::with_persistence(
            Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: Some(self.0.clone()),
            },
            self.0.join("state"),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("native notification fixture cleanup: {error}");
        }
    }
}
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(snapshot) = desktop.snapshot()
            && predicate(&snapshot)
        {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "desktop state did not settle");
        thread::sleep(Duration::from_millis(5));
    }
}
fn execute(desktop: &Desktop, id: u64, payload: Command) -> Arc<Snapshot> {
    desktop
        .submit(Request {
            version: 1,
            request_id: id,
            payload,
        })
        .unwrap();
    wait(desktop, |snapshot| snapshot.last_request_id == id)
}
fn persist(desktop: &Desktop, id: u64, action: SessionCommand) -> Arc<Snapshot> {
    let accepted = execute(desktop, id, Command::Session { action });
    assert!(accepted.last_error.is_none(), "{:?}", accepted.last_error);
    let operation_id = accepted.persistence.operations.last().unwrap().operation_id;
    let finished = wait(desktop, |snapshot| {
        snapshot.persistence.operations.iter().any(|op| {
            op.operation_id == operation_id && !matches!(op.state, OperationState::Pending)
        })
    });
    let receipt = finished
        .persistence
        .operations
        .iter()
        .find(|op| op.operation_id == operation_id)
        .unwrap();
    assert!(
        matches!(receipt.state, OperationState::Completed),
        "{:?}",
        receipt.error
    );
    finished
}

#[test]
fn a_pre_restore_notification_cannot_open_a_new_shell_reusing_its_surface_id() {
    let fixture = Fixture::new();
    let desktop = fixture.desktop();
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let first = &initial.model.workspaces[0];
    let target = first.root.panes()[0].selected();
    let second = execute(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Keep focus".into(),
            },
        },
    );
    assert!(second.last_error.is_none());
    wait(&desktop, |s| s.pending_launches == 0);
    let notified = execute(
        &desktop,
        2,
        Command::Attention {
            action: AttentionCommand::Notify {
                subtitle: String::new(),
                desktop: None,
                surface_id: target.id,
                title: "Earlier shell".into(),
                body: "Review its output".into(),
            },
        },
    );
    assert!(notified.last_error.is_none());
    let notification_id = notified.notifications.last().unwrap().id;
    let old_target = notified
        .notifications
        .last()
        .unwrap()
        .target
        .clone()
        .unwrap();
    let export = fixture.0.join("checkpoint.json");
    persist(
        &desktop,
        3,
        SessionCommand::Export {
            path: export.clone(),
            force: false,
        },
    );
    for (index, workspace) in second.model.workspaces.iter().enumerate() {
        let closed = execute(
            &desktop,
            4 + index as u64,
            Command::Action {
                action: Action::CloseWorkspace {
                    workspace_id: workspace.id,
                },
            },
        );
        assert!(closed.last_error.is_none());
    }
    let restored = persist(&desktop, 6, SessionCommand::Restore { path: Some(export) });
    let replacement = restored
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .find(|s| s.id == target.id)
        .unwrap();
    assert_ne!(
        replacement.session_id, target.session_id,
        "fixture must restore a different shell generation"
    );
    let attempted = execute(
        &desktop,
        7,
        Command::Attention {
            action: AttentionCommand::Open {
                id: notification_id,
            },
        },
    );
    assert!(
        attempted.last_error.is_some(),
        "a stale notification selected the replacement shell"
    );
    assert_eq!(
        attempted.model.focused_workspace,
        restored.model.focused_workspace
    );
    assert!(
        !attempted
            .notifications
            .iter()
            .find(|n| n.id == notification_id)
            .unwrap()
            .read
    );
    assert_ne!(restored.native_notifications.epoch, old_target.epoch);
    for action in [
        NativeAttentionAction::Open {
            target: old_target.clone(),
        },
        NativeAttentionAction::OpenInbox {
            target: old_target.clone(),
        },
        NativeAttentionAction::Dismiss {
            target: old_target.clone(),
        },
    ] {
        assert!(desktop.native_notification_action(action).is_err());
        let rejected = desktop.snapshot().unwrap();
        assert_eq!(
            rejected.model.focused_workspace,
            restored.model.focused_workspace
        );
        assert!(
            !rejected
                .notifications
                .iter()
                .find(|n| n.id == notification_id)
                .unwrap()
                .read
        );
    }
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn native_actions_confirm_exact_selection_and_read_before_returning() {
    let fixture = Fixture::new();
    let desktop = Arc::new(fixture.desktop());
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let first = &initial.model.workspaces[0];
    let first_surface = first.root.panes()[0].selected();
    let second = execute(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Other".into(),
            },
        },
    );
    assert!(second.last_error.is_none());
    let second_workspace = second.model.focused_workspace;
    let second_surface = second
        .model
        .workspaces
        .iter()
        .find(|w| Some(w.id) == second_workspace)
        .unwrap()
        .root
        .panes()[0]
        .selected()
        .clone();
    let notified = execute(
        &desktop,
        2,
        Command::Attention {
            action: AttentionCommand::Notify {
                subtitle: String::new(),
                desktop: None,
                surface_id: first_surface.id,
                title: "Exact target".into(),
                body: "".into(),
            },
        },
    );
    assert!(notified.last_error.is_none());
    let target = notified
        .notifications
        .last()
        .unwrap()
        .target
        .clone()
        .unwrap();
    desktop
        .native_notification_event(NativeNotificationEvent::Authorization {
            epoch: notified.native_notifications.epoch,
            generation: 1,
            state: NotificationAuthorization::Authorized,
        })
        .unwrap();
    assert_eq!(
        desktop
            .snapshot()
            .unwrap()
            .native_notifications
            .authorization,
        NotificationAuthorization::Authorized
    );

    // The wrapper must validate the captured token before selecting or acknowledging.
    let mut wrong_session = target.clone();
    match &mut wrong_session.destination {
        NotificationDestination::Surface { session_id, .. } => {
            *session_id = second_surface.session_id;
        }
        NotificationDestination::Workspace { .. } => panic!("surface notification expected"),
    }
    assert!(
        desktop
            .native_notification_action(NativeAttentionAction::Open {
                target: wrong_session
            })
            .is_err()
    );
    let rejected = desktop.snapshot().unwrap();
    assert_eq!(rejected.model.focused_workspace, second_workspace);
    assert!(
        !rejected
            .notifications
            .iter()
            .find(|n| n.id == target.notification_id)
            .unwrap()
            .read
    );

    // Socket disablement does not revoke the separate trusted native callback path.
    let server =
        ControlServer::start(&fixture.0.join("control.sock"), Arc::clone(&desktop)).unwrap();
    server
        .set_policy(ControlPolicy::same_user(ControlAccess::Disabled))
        .unwrap();
    let receipt = desktop
        .native_notification_action(NativeAttentionAction::Open {
            target: target.clone(),
        })
        .unwrap();
    assert_eq!(receipt.focused_surface_id, Some(first_surface.id));
    let opened = desktop.snapshot().unwrap();
    assert!(opened.revision >= receipt.revision);
    assert_eq!(opened.model.focused_workspace, Some(first.id));
    assert_eq!(
        opened.model.workspaces[0].root.panes()[0].selected_surface,
        first_surface.id
    );
    assert!(
        opened
            .notifications
            .iter()
            .find(|n| n.id == target.notification_id)
            .unwrap()
            .read
    );
    assert!(
        opened
            .native_notifications
            .desired
            .iter()
            .all(|request| request.target != target)
    );

    let to_dismiss = execute(
        &desktop,
        3,
        Command::Attention {
            action: AttentionCommand::Notify {
                subtitle: String::new(),
                desktop: None,
                surface_id: second_surface.id,
                title: "Dismiss this row".into(),
                body: "".into(),
            },
        },
    );
    let dismiss_target = to_dismiss
        .notifications
        .last()
        .unwrap()
        .target
        .clone()
        .unwrap();
    let dismissed = desktop
        .native_notification_action(NativeAttentionAction::Dismiss {
            target: dismiss_target.clone(),
        })
        .unwrap();
    assert_eq!(dismissed.focused_surface_id, None);
    let after = desktop.snapshot().unwrap();
    assert_eq!(after.model, to_dismiss.model);
    assert!(
        after
            .notifications
            .iter()
            .find(|n| n.id == dismiss_target.notification_id)
            .unwrap()
            .read
    );

    // Permission denial suppresses OS delivery, but an exact inbox action remains valid.
    desktop
        .native_notification_event(NativeNotificationEvent::Authorization {
            epoch: after.native_notifications.epoch,
            generation: 2,
            state: NotificationAuthorization::Denied,
        })
        .unwrap();
    let inbox = execute(
        &desktop,
        4,
        Command::Attention {
            action: AttentionCommand::Notify {
                subtitle: String::new(),
                desktop: None,
                surface_id: second_surface.id,
                title: "Inbox only".into(),
                body: "".into(),
            },
        },
    );
    let inbox_target = inbox.notifications.last().unwrap().target.clone().unwrap();
    assert!(
        desktop
            .native_notification_action(NativeAttentionAction::Open {
                target: inbox_target.clone()
            })
            .is_err()
    );
    let receipt = desktop
        .native_notification_action(NativeAttentionAction::OpenInbox {
            target: inbox_target.clone(),
        })
        .unwrap();
    assert_eq!(receipt.focused_surface_id, Some(second_surface.id));
    let after = desktop.snapshot().unwrap();
    assert_eq!(after.model.focused_workspace, second_workspace);
    assert!(
        after
            .notifications
            .iter()
            .find(|n| n.id == inbox_target.notification_id)
            .unwrap()
            .read
    );
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let closed = desktop.snapshot().unwrap();
    assert!(closed.native_notifications.desired.is_empty());
    assert_ne!(
        closed.native_notifications.epoch,
        after.native_notifications.epoch
    );
}

#[test]
fn native_inbox_callbacks_reject_coalesced_and_closed_targets_without_acknowledging_replacements() {
    let fixture = Fixture::new();
    let desktop = fixture.desktop();
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let first = &initial.model.workspaces[0];
    let surface = first.root.panes()[0].selected();
    let second = execute(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Retain focus".into(),
            },
        },
    );
    let notify = || Command::Attention {
        action: AttentionCommand::Notify {
            subtitle: String::new(),
            desktop: None,
            surface_id: surface.id,
            title: "Repeated event".into(),
            body: "Same content".into(),
        },
    };
    let original = execute(&desktop, 2, notify());
    let original_target = original
        .notifications
        .last()
        .unwrap()
        .target
        .clone()
        .unwrap();
    let replacement = execute(&desktop, 3, notify());
    let replacement_target = replacement
        .notifications
        .last()
        .unwrap()
        .target
        .clone()
        .unwrap();
    assert_ne!(
        original_target.notification_id,
        replacement_target.notification_id
    );
    assert!(
        replacement
            .notifications
            .iter()
            .all(|n| n.id != original_target.notification_id),
        "fixture must coalesce the earlier unread generation"
    );
    assert!(
        desktop
            .native_notification_action(NativeAttentionAction::OpenInbox {
                target: original_target
            })
            .is_err()
    );
    let rejected = desktop.snapshot().unwrap();
    assert_eq!(
        rejected.model.focused_workspace,
        second.model.focused_workspace
    );
    assert!(
        !rejected
            .notifications
            .iter()
            .find(|n| n.id == replacement_target.notification_id)
            .unwrap()
            .read
    );
    let closed = execute(
        &desktop,
        4,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: first.id,
            },
        },
    );
    assert!(closed.last_error.is_none());
    assert!(
        desktop
            .native_notification_action(NativeAttentionAction::OpenInbox {
                target: replacement_target.clone()
            })
            .is_err()
    );
    let rejected = desktop.snapshot().unwrap();
    assert_eq!(
        rejected.model.focused_workspace,
        second.model.focused_workspace
    );
    assert!(
        !rejected
            .notifications
            .iter()
            .find(|n| n.id == replacement_target.notification_id)
            .unwrap()
            .read
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn socket_and_generic_json_cannot_forge_native_notification_callbacks() {
    let fixture = Fixture::new();
    let desktop = Arc::new(fixture.desktop());
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let socket = fixture.0.join("control.sock");
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let native_action = serde_json::json!({"action":"event","event":{
        "event":"authorization","epoch":initial.native_notifications.epoch,"generation":1,"state":"authorized"
    }});
    assert!(serde_json::from_value::<Request>(serde_json::json!({
        "version":1,"request_id":1,"payload":{"command":"native_attention","action":native_action}
    })).is_err());
    for command in [
        serde_json::json!({"method":"native_attention","action":native_action}),
        serde_json::json!({"method":"attention","action":{"operation":"native_attention","action":native_action}}),
    ] {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":CONTROL_VERSION,"request_id":42,"command":command}),
        )
        .unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let response: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response["ok"], false);
    }
    let after = desktop.snapshot().unwrap();
    assert_eq!(
        after.native_notifications.authorization,
        NotificationAuthorization::Unknown
    );
    assert_eq!(after.native_notifications.authorization_generation, 0);
    assert_eq!(after.model, initial.model);
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
