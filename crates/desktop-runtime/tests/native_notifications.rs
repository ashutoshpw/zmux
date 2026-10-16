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
    Command, Desktop, DesktopError, Request, Shell, Snapshot,
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

#[test]
fn native_menu_mutations_capture_scope_cursor_and_epoch_without_changing_focus() {
    let fixture = Fixture::new();
    let desktop = fixture.desktop();
    let initial = wait(&desktop, |s| {
        !s.persistence.startup_pending && s.pending_launches == 0 && !s.model.workspaces.is_empty()
    });
    let first = initial.model.workspaces[0].id;
    let first_surface = initial.model.workspaces[0].root.panes()[0].selected().id;
    let second = execute(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Other notification owner".into(),
            },
        },
    );
    let second_surface = second.model.workspaces.last().unwrap().root.panes()[0]
        .selected()
        .id;
    let focused = execute(
        &desktop,
        2,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Keep selected".into(),
            },
        },
    );
    let notify = |surface_id, title: &str| Command::Attention {
        action: AttentionCommand::Notify {
            surface_id,
            title: title.into(),
            subtitle: String::new(),
            body: String::new(),
            desktop: Some(false),
            reply: false,
        },
    };
    let first_event = execute(&desktop, 3, notify(first_surface, "Captured first"));
    let first_id = first_event.notifications.last().unwrap().id;
    let captured = execute(&desktop, 4, notify(second_surface, "Captured other owner"));
    let cursor = captured.notifications.last().unwrap().id;
    let arrival = execute(
        &desktop,
        5,
        notify(first_surface, "Arrived after menu opening"),
    );
    let latest_id = arrival.notifications.last().unwrap().id;
    let mutation = |kind, scope, epoch, namespace| {
        serde_json::from_value::<NativeAttentionAction>(serde_json::json!({
            "action":"mutate", "mutation":kind, "workspace_id":scope,
            "through_id":cursor, "epoch":epoch, "identity_namespace":namespace,
        }))
        .expect("native menu mutations must be admitted by the shared owner")
    };
    let epoch = captured.native_notifications.epoch;
    let namespace = captured.model.identity_namespace;
    let receipt = desktop
        .native_notification_action(mutation("mark_read", Some(first), epoch, namespace))
        .unwrap();
    assert!(receipt.focused_window_id.is_none());
    assert!(receipt.focused_workspace_id.is_none());
    let read = wait(&desktop, |s| {
        s.notifications.iter().any(|n| n.id == first_id && n.read)
    });
    assert_eq!(
        read.model.focused_workspace,
        focused.model.focused_workspace
    );
    assert!(
        read.notifications
            .iter()
            .filter(|n| n.id != first_id)
            .all(|n| !n.read)
    );
    for (wrong_epoch, wrong_namespace) in [
        (uuid::Uuid::new_v4(), namespace),
        (epoch, uuid::Uuid::new_v4()),
    ] {
        assert!(
            desktop
                .native_notification_action(mutation("clear", None, wrong_epoch, wrong_namespace))
                .is_err()
        );
        assert_eq!(wait(&desktop, |_| true).notifications.len(), 3);
    }
    desktop
        .native_notification_action(mutation("clear", None, epoch, namespace))
        .unwrap();
    let cleared = wait(&desktop, |s| s.notifications.len() == 1);
    assert_eq!(cleared.notifications[0].id, latest_id);
    assert!(!cleared.notifications[0].read);
    assert_eq!(
        cleared.model.focused_workspace,
        focused.model.focused_workspace
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

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
        match desktop.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(_) | Err(DesktopError::Busy) => {}
            Err(error) => panic!("desktop snapshot failed: {error}"),
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

fn reply_desktop(script: &str) -> Desktop {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
    })
    .unwrap();
    wait(&desktop, |snapshot| {
        snapshot.frames.iter().any(|frame| {
            frame
                .snapshot
                .as_ref()
                .is_some_and(|frame| frame.viewport.contents.contains("REPLY_READY"))
        })
    });
    desktop
}
fn reply_notification(
    desktop: &Desktop,
    request_id: u64,
    surface: zmux_workspace_core::SurfaceId,
) -> zmux_desktop_runtime::attention::NotificationTarget {
    let action = serde_json::from_value(serde_json::json!({
        "operation":"notify", "surface_id":surface.0, "title":"Explicit terminal reply",
        "desktop":false, "reply":true,
    }))
    .expect("reply opt-in must be accepted");
    let created = execute(desktop, request_id, Command::Attention { action });
    assert!(created.last_error.is_none(), "{:?}", created.last_error);
    let row = created.notifications.last().unwrap();
    assert_eq!(serde_json::to_value(row).unwrap()["reply_enabled"], true);
    row.target.clone().unwrap()
}
fn reply_action(
    target: &zmux_desktop_runtime::attention::NotificationTarget,
    text: &str,
) -> NativeAttentionAction {
    serde_json::from_value(serde_json::json!({"action":"reply", "target":target, "text":text}))
        .unwrap()
}

#[test]
fn native_reply_confirms_full_write_without_focus_change_and_never_replays_a_consumed_generation() {
    let desktop = reply_desktop(
        "stty -echo; printf REPLY_READY; i=0; while IFS= read -r line; do i=$((i+1)); printf '\\nREPLY_%s:%s\\n' \"$i\" \"$line\"; done",
    );
    let initial = wait(&desktop, |_| true);
    let first = initial.model.workspaces[0].clone();
    let surface = first.root.panes()[0].selected();
    let session_id = surface.session_id.unwrap();
    let foreground = execute(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Keep foreground".into(),
            },
        },
    )
    .model
    .focused_workspace;
    let target = reply_notification(&desktop, 2, surface.id);
    assert!(
        desktop
            .native_notification_action(reply_action(&target, &"é".repeat(2048)))
            .is_err()
    );
    let mut forged = target.clone();
    if let NotificationDestination::Surface { session_id, .. } = &mut forged.destination {
        *session_id = Some(zmux_workspace_core::SessionId(999_999));
    }
    assert!(
        desktop
            .native_notification_action(reply_action(&forged, "wrong owner"))
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(wait(&desktop, |_| true).notifications.last().unwrap()).unwrap()["reply_state"],
        "ready"
    );
    let result = desktop
        .native_notification_action(reply_action(&target, " \nhello 世界\t "))
        .unwrap();
    let wire = serde_json::to_value(result).unwrap();
    assert_eq!(
        wire["reply"]["target"],
        serde_json::to_value(&target).unwrap()
    );
    assert_eq!(wire["reply"]["outcome"], "complete");
    assert_eq!(wire["reply"]["bytes_written"], "hello 世界".len() + 1);
    assert_eq!(wire["reply"]["total_bytes"], "hello 世界".len() + 1);
    assert!(wire["navigation"].is_null());
    assert!(wire["focused_workspace_id"].is_null());
    let written = wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == session_id
                && frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|frame| frame.viewport.contents.contains("REPLY_1:hello 世界"))
        })
    });
    assert_eq!(written.model.focused_workspace, foreground);
    assert!(
        written
            .notifications
            .iter()
            .find(|row| row.id == target.notification_id)
            .unwrap()
            .read
    );
    assert!(
        desktop
            .native_notification_action(reply_action(&target, "duplicate"))
            .is_err()
    );
    execute(
        &desktop,
        3,
        Command::Input {
            session_id,
            text: "BARRIER\n".into(),
        },
    );
    let barrier = wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == session_id
                && frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|frame| frame.viewport.contents.contains("REPLY_2:BARRIER"))
        })
    });
    assert_eq!(barrier.model.focused_workspace, foreground);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn empty_and_definite_zero_byte_reply_failures_open_only_the_exact_owner_with_truthful_outcomes() {
    for (script, text, expected) in [
        (
            "stty -echo; printf REPLY_READY; exec sleep 60",
            " \t\n",
            "empty",
        ),
        ("printf REPLY_READY; exit 0", "not written", "not_written"),
    ] {
        let desktop = reply_desktop(script);
        let initial = wait(&desktop, |_| true);
        let first = initial.model.workspaces[0].clone();
        let surface = first.root.panes()[0].selected().id;
        if expected == "not_written" {
            wait(&desktop, |snapshot| {
                snapshot.frames.iter().all(|frame| {
                    frame
                        .snapshot
                        .as_ref()
                        .is_some_and(|frame| frame.state == zmux_session_runtime::State::Exited)
                })
            });
        }
        execute(
            &desktop,
            1,
            Command::Action {
                action: Action::CreateWorkspace {
                    name: "Other".into(),
                },
            },
        );
        let target = reply_notification(&desktop, 2, surface);
        let result = desktop
            .native_notification_action(reply_action(&target, text))
            .unwrap();
        let wire = serde_json::to_value(result).unwrap();
        assert_eq!(wire["reply"]["outcome"], expected);
        assert_eq!(wire["reply"]["bytes_written"], 0);
        assert_eq!(wire["navigation"]["opened"], true);
        assert_eq!(
            wire["navigation"]["target"],
            serde_json::to_value(&target).unwrap()
        );
        assert_eq!(wire["focused_workspace_id"], first.id.0);
        assert!(
            desktop
                .native_notification_action(reply_action(&target, "duplicate"))
                .is_err()
        );
        desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn pending_reply_keeps_the_executor_responsive_and_expires_without_writing_or_replaying() {
    let desktop = Arc::new(reply_desktop(
        "stty raw -echo; printf REPLY_READY; kill -STOP $$; cat",
    ));
    let initial = wait(&desktop, |_| true);
    let first = initial.model.workspaces[0].clone();
    let surface = first.root.panes()[0].selected();
    let session_id = surface.session_id.unwrap();
    let process_id = initial
        .frames
        .iter()
        .find(|frame| frame.session_id == session_id)
        .unwrap()
        .process_id
        .unwrap();
    for index in 0..8 {
        let sent = execute(
            &desktop,
            10 + index,
            Command::Input {
                session_id,
                text: "x".repeat(4096),
            },
        );
        assert!(sent.last_error.is_none());
    }
    let foreground = execute(
        &desktop,
        20,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Still responsive".into(),
            },
        },
    )
    .model
    .focused_workspace
    .unwrap();
    let target = reply_notification(&desktop, 21, surface.id);
    let (sender, receiver) = std::sync::mpsc::channel();
    let owned_desktop = Arc::clone(&desktop);
    let owned_target = target.clone();
    let task = thread::spawn(move || {
        sender
            .send(
                owned_desktop
                    .native_notification_action(reply_action(&owned_target, "MUST_NOT_REPLAY")),
            )
            .unwrap()
    });
    let pending = wait(&desktop, |snapshot| {
        snapshot.notifications.iter().any(|row| {
            row.id == target.notification_id
                && serde_json::to_value(row).unwrap()["reply_state"] == "pending"
        })
    });
    assert_eq!(pending.model.focused_workspace, Some(foreground));
    assert!(
        !pending
            .native_notifications
            .desired
            .iter()
            .any(|request| request.target == target)
    );
    let responsive = execute(
        &desktop,
        22,
        Command::Action {
            action: Action::RenameWorkspace {
                workspace_id: foreground,
                name: "Renamed while writing".into(),
            },
        },
    );
    assert!(responsive.last_error.is_none());
    assert!(
        !responsive
            .notifications
            .iter()
            .find(|row| row.id == target.notification_id)
            .unwrap()
            .read
    );
    let result = receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    task.join().unwrap();
    let wire = serde_json::to_value(result).unwrap();
    assert_eq!(wire["reply"]["outcome"], "not_written");
    assert_eq!(wire["reply"]["bytes_written"], 0);
    assert_eq!(wire["navigation"]["opened"], true);
    assert_eq!(wire["focused_workspace_id"], first.id.0);
    assert!(
        desktop
            .native_notification_action(reply_action(&target, "MUST_NOT_REPLAY"))
            .is_err()
    );
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(process_id as i32),
        nix::sys::signal::Signal::SIGCONT,
    )
    .unwrap();
    execute(
        &desktop,
        23,
        Command::Input {
            session_id,
            text: "\r\nAFTER_REPLY\r\n".into(),
        },
    );
    let after = wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == session_id
                && frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|frame| frame.viewport.contents.contains("AFTER_REPLY"))
        })
    });
    let contents = &after
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)
        .unwrap()
        .snapshot
        .as_ref()
        .unwrap()
        .viewport
        .contents;
    assert!(!contents.contains("MUST_NOT_REPLAY"));
    desktop.shutdown(Duration::from_secs(5)).unwrap();
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
                reply: false,
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
                id: notification_id.into(),
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
        let rejected = wait(&desktop, |_| true);
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
                reply: false,
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
        wait(&desktop, |_| true).native_notifications.authorization,
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
    let rejected = wait(&desktop, |_| true);
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
    let opened = wait(&desktop, |_| true);
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
                reply: false,
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
    let after = wait(&desktop, |_| true);
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
                reply: false,
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
    let after = wait(&desktop, |_| true);
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
    let closed = wait(&desktop, |_| true);
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
            reply: false,
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
    let rejected = wait(&desktop, |_| true);
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
    let rejected = wait(&desktop, |_| true);
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
    let after = wait(&desktop, |_| true);
    assert_eq!(
        after.native_notifications.authorization,
        NotificationAuthorization::Unknown
    );
    assert_eq!(after.native_notifications.authorization_generation, 0);
    assert_eq!(after.model, initial.model);
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
