use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{
    Desktop, DesktopError, Request, Shell, Snapshot,
    shared_terminals::{
        SharedTerminalCallError, SharedTerminalEnvelope, SharedTerminalOperation,
        SharedTerminalResponse, SharedTerminalSession, TerminalSharingError,
    },
};
use zmux_workspace_core::Action;

fn new_desktop() -> Desktop {
    Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap()
}

fn envelope(grant: uuid::Uuid, request: SharedTerminalOperation) -> SharedTerminalEnvelope {
    SharedTerminalEnvelope { grant, request }
}

#[track_caller]
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match desktop.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(_) | Err(DesktopError::Busy) => {}
            Err(error) => panic!("desktop snapshot failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "desktop did not reach expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn listed_sessions(desktop: &Desktop, grant: uuid::Uuid) -> Vec<SharedTerminalSession> {
    match desktop
        .shared_terminal_operation(envelope(grant, SharedTerminalOperation::List))
        .unwrap()
    {
        SharedTerminalResponse::List { sessions, .. } => sessions,
        _ => panic!("list returned a non-list response"),
    }
}

fn expect_error<T>(result: Result<T, SharedTerminalCallError>) -> SharedTerminalCallError {
    match result {
        Ok(_) => panic!("shared terminal request unexpectedly succeeded"),
        Err(error) => error,
    }
}

#[test]
fn registered_device_terminal_access_is_bound_to_real_background_sessions_and_grants() {
    let desktop = new_desktop();
    let initial = wait(&desktop, |snapshot| {
        snapshot.frames.iter().any(|frame| {
            frame
                .snapshot
                .as_ref()
                .is_some_and(|terminal| terminal.state == zmux_session_runtime::State::Running)
        })
    });
    let namespace = initial.model.identity_namespace;
    let first_pane = initial.model.workspaces[0].root.panes()[0].clone();
    let first_surface = first_pane.selected().id;
    let first_session = first_pane.selected().session_id.unwrap();

    let grant = desktop.set_terminal_sharing(true).unwrap().unwrap();
    assert_eq!(desktop.set_terminal_sharing(true).unwrap(), Some(grant));
    let initial_list = listed_sessions(&desktop, grant);
    assert!(initial_list.iter().any(|session| {
        session.surface_id == first_surface && session.session_id == first_session
    }));

    desktop
        .submit(Request {
            version: 1,
            request_id: 1,
            payload: zmux_desktop_runtime::Command::Action {
                action: Action::CreateSurface {
                    pane_id: first_pane.id,
                },
            },
        })
        .unwrap();
    let background = wait(&desktop, |snapshot| {
        snapshot.model.workspaces[0].root.panes()[0].surfaces.len() == 2
            && snapshot.control_frames.len() == 2
            && snapshot.frames.len() == 1
            && snapshot.control_frames.iter().all(|frame| {
                frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|terminal| terminal.state == zmux_session_runtime::State::Running)
            })
    });
    assert!(
        !background
            .frames
            .iter()
            .any(|frame| frame.session_id == first_session)
    );
    let second_surface = background.model.workspaces[0].root.panes()[0]
        .surfaces
        .iter()
        .find(|surface| surface.id != first_surface)
        .unwrap()
        .id;
    let second_session = background.model.workspaces[0].root.panes()[0]
        .surfaces
        .iter()
        .find(|surface| surface.id == second_surface)
        .unwrap()
        .session_id
        .unwrap();
    let sessions = listed_sessions(&desktop, grant);
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().any(|session| {
        session.surface_id == first_surface && session.session_id == first_session
    }));
    assert!(sessions.iter().any(|session| {
        session.surface_id == second_surface && session.session_id == second_session
    }));

    let hidden_frame = desktop
        .shared_terminal_operation(envelope(
            grant,
            SharedTerminalOperation::Frame {
                namespace,
                surface_id: first_surface,
                session_id: first_session,
            },
        ))
        .unwrap();
    assert!(matches!(
        hidden_frame,
        SharedTerminalResponse::Frame { ref frame, .. }
            if frame.session_id == first_session && frame.snapshot.is_some()
    ));

    let wrong_namespace = expect_error(desktop.shared_terminal_operation(envelope(
        grant,
        SharedTerminalOperation::Frame {
            namespace: uuid::Uuid::new_v4(),
            surface_id: first_surface,
            session_id: first_session,
        },
    )));
    assert!(matches!(
        wrong_namespace.error,
        DesktopError::TerminalSharing(TerminalSharingError::StaleNamespace)
    ));
    assert!(!wrong_namespace.outcome_unknown);

    let mismatched_pair = expect_error(desktop.shared_terminal_operation(envelope(
        grant,
        SharedTerminalOperation::Input {
            namespace,
            surface_id: second_surface,
            session_id: first_session,
            text: "must_not_reach_terminal\r".into(),
        },
    )));
    assert!(matches!(
        mismatched_pair.error,
        DesktopError::TerminalSharing(TerminalSharingError::StaleTarget)
    ));
    assert!(!mismatched_pair.outcome_unknown);

    let marker = "REGISTERED_DEVICE_SHARED_INPUT";
    let accepted = desktop
        .shared_terminal_operation(envelope(
            grant,
            SharedTerminalOperation::Input {
                namespace,
                surface_id: first_surface,
                session_id: first_session,
                text: format!("printf '\\n{marker}\\n'\r"),
            },
        ))
        .unwrap();
    assert!(matches!(
        accepted,
        SharedTerminalResponse::Accepted { accepted: true, .. }
    ));
    let observed = wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == first_session
                && frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|terminal| terminal.viewport.contents.contains(marker))
        })
    });
    assert!(
        !observed
            .frames
            .iter()
            .any(|frame| frame.session_id == first_session)
    );
    assert_eq!(
        observed.model.workspaces[0].root.panes()[0].selected_surface,
        second_surface
    );

    let resized = desktop
        .shared_terminal_operation(envelope(
            grant,
            SharedTerminalOperation::Resize {
                namespace,
                surface_id: first_surface,
                session_id: first_session,
                rows: 19,
                cols: 71,
            },
        ))
        .unwrap();
    assert!(matches!(
        resized,
        SharedTerminalResponse::Accepted { accepted: true, .. }
    ));
    wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == first_session
                && frame.snapshot.as_ref().is_some_and(|terminal| {
                    terminal.viewport.size.rows == 19 && terminal.viewport.size.cols == 71
                })
        })
    });

    desktop.set_terminal_sharing(false).unwrap();
    let disabled = expect_error(
        desktop.shared_terminal_operation(envelope(grant, SharedTerminalOperation::List)),
    );
    assert!(matches!(
        disabled.error,
        DesktopError::TerminalSharing(TerminalSharingError::Disabled)
    ));
    assert!(!disabled.outcome_unknown);
    let replacement_grant = desktop.set_terminal_sharing(true).unwrap().unwrap();
    assert_ne!(replacement_grant, grant);
    let stale_grant = expect_error(
        desktop.shared_terminal_operation(envelope(grant, SharedTerminalOperation::List)),
    );
    assert!(matches!(
        stale_grant.error,
        DesktopError::TerminalSharing(TerminalSharingError::InvalidGrant)
    ));

    desktop
        .submit(Request {
            version: 1,
            request_id: 2,
            payload: zmux_desktop_runtime::Command::Session {
                action: zmux_desktop_runtime::sessions::SessionCommand::Restore {
                    path: Some(std::env::temp_dir().join(format!(
                        "zmux-missing-shared-terminal-restore-{}.json",
                        uuid::Uuid::new_v4()
                    ))),
                },
            },
        })
        .unwrap();
    let restore_failure = wait(&desktop, |snapshot| snapshot.last_request_id == 2);
    assert!(restore_failure.last_error.is_some());
    let revoked_by_restore = expect_error(
        desktop
            .shared_terminal_operation(envelope(replacement_grant, SharedTerminalOperation::List)),
    );
    assert!(matches!(
        revoked_by_restore.error,
        DesktopError::TerminalSharing(TerminalSharingError::Disabled)
    ));
    let after_restore_grant = desktop.set_terminal_sharing(true).unwrap().unwrap();
    assert_ne!(after_restore_grant, replacement_grant);
    desktop.shutdown(Duration::from_secs(5)).unwrap();

    let restarted = new_desktop();
    let restart_disabled =
        expect_error(restarted.shared_terminal_operation(envelope(
            after_restore_grant,
            SharedTerminalOperation::List,
        )));
    assert!(matches!(
        restart_disabled.error,
        DesktopError::TerminalSharing(TerminalSharingError::Disabled)
    ));
    assert_ne!(
        restarted.set_terminal_sharing(true).unwrap().unwrap(),
        after_restore_grant
    );
    restarted.shutdown(Duration::from_secs(5)).unwrap();
}
