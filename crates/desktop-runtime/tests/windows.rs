//! Physical PTYs and trusted native window acknowledgements at their real owner.
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    Command, Desktop, Request, Shell, Snapshot,
    windows::{NativeWindowEvent, WindowAction, WindowHostToken, WindowKind, WindowRequest},
};
use zmux_workspace_core::{
    Action, WindowId,
    targets::{CallerContext, TargetScope},
};
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(snapshot) = desktop.snapshot()
            && predicate(&snapshot)
        {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "window owner did not settle: {:?}",
            desktop.snapshot().ok().and_then(|s| s.last_error.clone())
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn send(desktop: &Desktop, id: u64, payload: Command) {
    desktop
        .submit(Request {
            version: 1,
            request_id: id,
            payload,
        })
        .unwrap();
}
fn request(action: WindowAction, window: Option<WindowId>) -> Command {
    Command::Windows {
        request: WindowRequest {
            scope: TargetScope {
                window: window.map(|id| format!("window:{}", id.0).parse().unwrap()),
                ..Default::default()
            },
            caller: CallerContext::default(),
            expected_model_revision: None,
            action,
        },
    }
}
fn acknowledge(
    desktop: &Desktop,
    snapshot: &Snapshot,
    window: WindowId,
    kind: WindowKind,
    host: Uuid,
) -> WindowHostToken {
    let request = snapshot
        .window_requests
        .iter()
        .find(|request| request.window_id == window && request.kind == kind)
        .unwrap();
    let token = WindowHostToken {
        epoch: request.epoch,
        identity_namespace: request.identity_namespace,
        window_id: window,
        host_id: host,
    };
    desktop
        .native_window_event(NativeWindowEvent::Complete {
            token: token.clone(),
            operation_id: request.operation_id,
        })
        .unwrap();
    token
}
#[test]
fn two_real_pty_windows_render_without_focus_and_close_requires_exact_native_completion() {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let initial = wait(&desktop, |s| {
        s.frames.len() == 1 && s.frames[0].process_id.is_some() && !s.window_requests.is_empty()
    });
    let original_window = initial.model.windows[0].id;
    let original_session = initial.frames[0].session_id;
    let original_pid = initial.frames[0].process_id;
    let original_host = Uuid::new_v4();
    let original = acknowledge(
        &desktop,
        &initial,
        original_window,
        WindowKind::Open,
        original_host,
    );
    desktop
        .native_window_event(NativeWindowEvent::Key {
            token: original.clone(),
            key: true,
        })
        .unwrap();
    send(
        &desktop,
        1,
        request(WindowAction::New { focus: false }, None),
    );
    let two = wait(&desktop, |s| {
        s.last_request_id == 1
            && s.frames.len() == 2
            && s.frames.iter().all(|f| f.process_id.is_some())
    });
    assert_eq!(two.model.focused_window, Some(original_window));
    assert_eq!(two.observed_key_window, Some(original_window));
    let second = two
        .model
        .windows
        .iter()
        .find(|window| window.id != original_window)
        .unwrap()
        .id;
    let pending = two
        .window_requests
        .iter()
        .find(|r| r.window_id == second && r.kind == WindowKind::Open)
        .unwrap();
    let receipt_id = pending.operation_id;
    assert_eq!(
        two.window_operations
            .iter()
            .find(|r| r.operation_id == receipt_id)
            .unwrap()
            .state,
        "pending"
    );
    let second_host = Uuid::new_v4();
    let second_token = acknowledge(&desktop, &two, second, WindowKind::Open, second_host);
    let second_session = two
        .frames
        .iter()
        .find(|f| f.session_id != original_session)
        .unwrap()
        .session_id;
    send(
        &desktop,
        2,
        Command::Input {
            session_id: original_session,
            text: "printf '\\nWINDOW_ONE_LIVE\\n'\n".into(),
        },
    );
    send(
        &desktop,
        3,
        Command::Input {
            session_id: second_session,
            text: "printf '\\nWINDOW_TWO_LIVE\\n'\n".into(),
        },
    );
    wait(&desktop, |s| {
        ["WINDOW_ONE_LIVE", "WINDOW_TWO_LIVE"].iter().all(|marker| {
            s.frames.iter().any(|f| {
                f.snapshot
                    .as_ref()
                    .is_some_and(|v| v.viewport.contents.lines().any(|line| line == *marker))
            })
        })
    });
    send(&desktop, 4, request(WindowAction::Close, Some(second)));
    let closing = wait(&desktop, |s| {
        s.last_request_id == 4 && s.model.windows.len() == 1
    });
    let close_request = closing
        .window_requests
        .iter()
        .find(|r| r.window_id == second && r.kind == WindowKind::Close)
        .unwrap();
    let mut forged = second_token.clone();
    forged.host_id = Uuid::new_v4();
    assert!(
        desktop
            .native_window_event(NativeWindowEvent::Complete {
                token: forged,
                operation_id: close_request.operation_id
            })
            .is_err()
    );
    send(
        &desktop,
        5,
        request(WindowAction::New { focus: false }, None),
    );
    let rejected = wait(&desktop, |s| s.last_request_id == 5);
    assert!(
        rejected
            .last_error
            .as_deref()
            .is_some_and(|m| m.contains("capacity remains charged"))
    );
    assert_eq!(rejected.model, closing.model);
    acknowledge(&desktop, &closing, second, WindowKind::Close, second_host);
    let cleaned = wait(&desktop, |s| {
        s.window_hosts.len() == 1 && s.control_frames.len() == 1
    });
    assert_eq!(cleaned.frames[0].process_id, original_pid);
    assert!(
        desktop
            .native_window_event(NativeWindowEvent::Key {
                token: second_token,
                key: true
            })
            .is_err()
    );
    send(
        &desktop,
        6,
        request(WindowAction::Close, Some(original_window)),
    );
    let last = wait(&desktop, |s| s.last_request_id == 6);
    assert_eq!(
        last.model, cleaned.model,
        "last close must retain the layout for final save"
    );
    assert_eq!(last.frames[0].process_id, original_pid);
    acknowledge(
        &desktop,
        &last,
        original_window,
        WindowKind::Terminate,
        original_host,
    );
    assert_eq!(desktop.snapshot().unwrap().model, last.model);
    desktop.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn empty_key_window_does_not_hide_other_windows_frames_and_native_tokens_are_not_public_commands() {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let initial = wait(&desktop, |s| {
        !s.frames.is_empty() && s.frames[0].process_id.is_some() && !s.window_requests.is_empty()
    });
    acknowledge(
        &desktop,
        &initial,
        initial.model.windows[0].id,
        WindowKind::Open,
        Uuid::new_v4(),
    );
    send(
        &desktop,
        1,
        request(WindowAction::New { focus: false }, None),
    );
    let two = wait(&desktop, |s| {
        s.last_request_id == 1 && s.model.windows.len() == 2
    });
    let second = two.model.windows[1].clone();
    let token = acknowledge(&desktop, &two, second.id, WindowKind::Open, Uuid::new_v4());
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: second.selected_workspace.unwrap(),
            },
        },
    );
    wait(&desktop, |s| s.last_request_id == 2);
    desktop
        .native_window_event(NativeWindowEvent::Key {
            token: token.clone(),
            key: true,
        })
        .unwrap();
    let empty = wait(&desktop, |s| {
        s.model.focused_window == Some(second.id) && s.model.focused_workspace.is_none()
    });
    assert_eq!(empty.frames.len(), 1);
    assert_eq!(empty.frames[0].session_id, initial.frames[0].session_id);
    let mut stale = token;
    stale.epoch = Uuid::new_v4();
    assert!(
        desktop
            .native_window_event(NativeWindowEvent::Key {
                token: stale,
                key: true
            })
            .is_err()
    );
    let forged = serde_json::json!({"command":"native_window","event":{"event":"failed","epoch":empty.window_epoch,"operation_id":Uuid::new_v4(),"message":"forged"}});
    assert!(serde_json::from_value::<Command>(forged).is_err());
    desktop.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn close_before_open_ack_drains_both_requests_and_late_open_cannot_revive_the_host() {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let initial = wait(&desktop, |s| {
        !s.frames.is_empty() && !s.window_requests.is_empty()
    });
    acknowledge(
        &desktop,
        &initial,
        initial.model.windows[0].id,
        WindowKind::Open,
        Uuid::new_v4(),
    );
    // Repeated generations exercise bounded retention as well as the first race.
    for generation in 0..40_u64 {
        send(
            &desktop,
            generation * 3 + 1,
            request(WindowAction::New { focus: false }, None),
        );
        let admitted = wait(&desktop, |s| {
            s.last_request_id == generation * 3 + 1 && s.model.windows.len() == 2
        });
        let window = admitted.model.windows[1].id;
        let open = admitted
            .window_requests
            .iter()
            .find(|r| r.window_id == window && r.kind == WindowKind::Open)
            .unwrap()
            .clone();
        send(
            &desktop,
            generation * 3 + 2,
            request(WindowAction::Close, Some(window)),
        );
        let closing = wait(&desktop, |s| {
            s.last_request_id == generation * 3 + 2 && s.model.windows.len() == 1
        });
        let host = Uuid::new_v4();
        acknowledge(&desktop, &closing, window, WindowKind::Close, host);
        let late = WindowHostToken {
            epoch: open.epoch,
            identity_namespace: open.identity_namespace,
            window_id: window,
            host_id: host,
        };
        assert!(
            desktop
                .native_window_event(NativeWindowEvent::Complete {
                    token: late,
                    operation_id: open.operation_id
                })
                .is_err()
        );
        let drained = wait(&desktop, |s| {
            !s.window_requests.iter().any(|r| r.window_id == window)
        });
        assert_eq!(drained.window_hosts.len(), 1);
        assert!(drained.window_operations.len() <= 32);
    }
    desktop.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn workspace_moves_preflight_native_destination_and_preserve_two_real_sessions() {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let initial = wait(&desktop, |s| {
        s.frames.first().is_some_and(|f| f.process_id.is_some()) && !s.window_requests.is_empty()
    });
    let source = initial.model.windows[0].id;
    let moving = initial.model.workspaces[0].clone();
    let source_host = Uuid::new_v4();
    let source_token = acknowledge(&desktop, &initial, source, WindowKind::Open, source_host);
    desktop
        .native_window_event(NativeWindowEvent::Key {
            token: source_token,
            key: true,
        })
        .unwrap();
    send(
        &desktop,
        1,
        request(WindowAction::New { focus: false }, None),
    );
    let two = wait(&desktop, |s| {
        s.last_request_id == 1
            && s.control_frames.len() == 2
            && s.control_frames.iter().all(|f| f.process_id.is_some())
    });
    let destination = two.model.windows[1].id;
    let movement = |destination, focus| Command::Action {
        action: Action::MoveWorkspaceToWindow {
            workspace_id: moving.id,
            destination_window_id: destination,
            index: None,
            focus,
        },
    };
    send(&desktop, 2, movement(destination, false));
    let denied = wait(&desktop, |s| s.last_request_id == 2);
    assert!(
        denied
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("native host is not ready"))
    );
    assert_eq!(denied.model, two.model);
    let destination_host = Uuid::new_v4();
    acknowledge(
        &desktop,
        &denied,
        destination,
        WindowKind::Open,
        destination_host,
    );
    send(&desktop, 3, movement(destination, false));
    let moved = wait(&desktop, |s| s.last_request_id == 3);
    assert_eq!(moved.model.focused_window, Some(source));
    assert_eq!(moved.model.focused_workspace, None);
    assert_eq!(moved.observed_key_window, Some(source));
    assert!(moved.window_requests.is_empty());
    assert_eq!(
        moved.model.windows[1].selected_workspace,
        two.model.windows[1].selected_workspace
    );
    assert_eq!(
        moved
            .control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>(),
        two.control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>()
    );
    send(
        &desktop,
        4,
        Command::Targeted {
            request: zmux_desktop_runtime::targets::TargetRequest {
                caller: CallerContext {
                    window: Some(format!("window:{}", source.0).parse().unwrap()),
                    surface: Some(
                        format!("surface:{}", moving.root.panes()[0].selected_surface.0)
                            .parse()
                            .unwrap(),
                    ),
                    ..Default::default()
                },
                operation: zmux_desktop_runtime::targets::TargetOperation::SendText {
                    text: "printf '\\nMOVED_ORIGINAL_SHELL\\n'\n".into(),
                },
                ..Default::default()
            },
        },
    );
    wait(&desktop, |s| {
        s.last_request_id == 4
            && s.control_frames.iter().any(|f| {
                f.snapshot.as_ref().is_some_and(|t| {
                    t.viewport
                        .contents
                        .lines()
                        .any(|l| l == "MOVED_ORIGINAL_SHELL")
                })
            })
    });
    // A move needs its own exact focus operation, never an older coalesced one.
    // Saturation must reject before committing the 33rd topology change.
    for sequence in 0..32_u64 {
        let target = if sequence % 2 == 0 {
            source
        } else {
            destination
        };
        send(&desktop, sequence + 5, movement(target, true));
        let focused = wait(&desktop, |s| s.last_request_id == sequence + 5);
        assert!(focused.last_error.is_none(), "{:?}", focused.last_error);
    }
    let full = desktop.snapshot().unwrap();
    assert_eq!(full.window_requests.len(), 32);
    send(&desktop, 40, movement(source, true));
    let rejected = wait(&desktop, |s| s.last_request_id == 40);
    assert!(
        rejected
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("request capacity is full"))
    );
    assert_eq!(rejected.model, full.model);
    desktop.shutdown(Duration::from_secs(3)).unwrap();
}
