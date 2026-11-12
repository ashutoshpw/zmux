//! Public desktop-owner checks for live OSC 7501 projection and exact selection.
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
#[path = "support/desktop_snapshot.rs"]
mod desktop_snapshot;
use zmux_desktop_runtime::{Command, Desktop, Request, Shell, Snapshot};
use zmux_program_status::State;
use zmux_workspace_core::Action;

fn desktop() -> Desktop {
    Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap()
}

fn submit(desktop: &Desktop, request_id: u64, payload: Command) {
    desktop
        .submit(Request {
            version: 1,
            request_id,
            payload,
        })
        .unwrap();
}

#[track_caller]
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut snapshot = desktop_snapshot::read(desktop);
    loop {
        if predicate(&snapshot) {
            return snapshot;
        }
        if Instant::now() >= deadline {
            panic!(
                "desktop did not reach expected OSC 7501 state: revision={}, request={}, error={:?}",
                snapshot.revision, snapshot.last_request_id, snapshot.last_error
            );
        }
        thread::sleep(Duration::from_millis(5));
        snapshot = desktop_snapshot::read_until(desktop, deadline);
    }
}

#[test]
fn public_desktop_projects_live_osc_alert_and_selects_only_the_exact_program() {
    let desktop = desktop();
    let ready = wait(&desktop, |snapshot| {
        !snapshot.frames.is_empty()
            && snapshot.frames.iter().all(|frame| {
                frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.state == zmux_session_runtime::State::Running)
            })
    });
    let first_workspace = ready.model.workspaces[0].id;
    let first_surface = ready.model.workspaces[0].root.panes()[0].selected().clone();
    let session_id = first_surface.session_id.unwrap();

    submit(
        &desktop,
        1,
        Command::Input {
            session_id,
            text: "printf '\\033]7501;state=blocked:kind=question:id=approval\\007'\n".into(),
        },
    );
    let projected = wait(&desktop, |snapshot| {
        snapshot.program_status.iter().any(|terminal| {
            terminal.session_id == session_id
                && terminal.records.iter().any(|record| {
                    record.id.as_ref() == "approval" && record.state == State::Blocked
                })
        }) && snapshot
            .program_status_alerts
            .events
            .iter()
            .any(|event| event.session_id == session_id && event.record_id == "approval")
            && snapshot.native_notifications.desired.iter().any(|request| {
                request.alert_sound_source.as_deref() == Some("osc7501")
                    && request.alert_sound_kind.as_deref() == Some("needs_user")
            })
    });
    let terminal = projected
        .program_status
        .iter()
        .find(|terminal| terminal.session_id == session_id)
        .unwrap();
    let record = terminal
        .records
        .iter()
        .find(|record| record.id.as_ref() == "approval")
        .unwrap();
    let selection = zmux_desktop_runtime::program_status::SelectionTarget {
        identity_namespace: terminal.identity_namespace,
        window_id: terminal.window_id,
        window_uuid: terminal.window_uuid,
        workspace_id: terminal.workspace_id,
        workspace_uuid: terminal.workspace_uuid,
        pane_id: terminal.pane_id,
        pane_uuid: terminal.pane_uuid,
        surface_id: terminal.surface_id,
        surface_uuid: terminal.surface_uuid,
        session_id: terminal.session_id,
        session_epoch: terminal.session_epoch,
        command_epoch: terminal.command_epoch,
        record_id: record.id.to_string(),
    };
    let wire = serde_json::to_value(&projected).unwrap();
    assert_eq!(wire["program_status"][0]["source"], "osc7501");
    assert!(wire["program_status"][0].get("session_epoch").is_some());
    assert!(
        wire["program_status_alerts"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["record_id"] == "approval" && event["source"] == "osc7501")
    );

    submit(
        &desktop,
        2,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Program selection target".into(),
            },
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.model.workspaces.len() == 2
            && snapshot.model.focused_workspace != Some(first_workspace)
    });

    submit(
        &desktop,
        3,
        Command::SelectProgramTarget {
            target: selection.clone(),
        },
    );
    let selected = wait(&desktop, |snapshot| {
        snapshot.model.focused_workspace == Some(first_workspace)
    });
    assert_eq!(
        selected.model.workspaces[0].root.panes()[0].selected_surface,
        first_surface.id
    );

    let mut stale = selection;
    stale.record_id = "missing-program".into();
    submit(&desktop, 4, Command::SelectProgramTarget { target: stale });
    wait(&desktop, |snapshot| {
        snapshot.last_request_id == 4
            && snapshot
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("stale program target"))
    });
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
