use std::{
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{
    Command, Desktop, DesktopError, MAX_TERMINALS, Request, Shell, Snapshot,
};
use zmux_workspace_core::{Action, Axis};

fn desktop() -> Desktop {
    Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap()
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
#[track_caller]
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> std::sync::Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match desktop.snapshot() {
            Ok(s) if predicate(&s) => return s,
            Ok(_) | Err(DesktopError::Busy) => {}
            Err(e) => panic!("{e}"),
        }
        assert!(
            Instant::now() < deadline,
            "desktop did not reach expected state: {}",
            format_args!(
                "revision={}, last_request={}, error={:?}, pending={}, workspaces={}",
                desktop.snapshot().unwrap().revision,
                desktop.snapshot().unwrap().last_request_id,
                desktop.snapshot().unwrap().last_error,
                desktop.snapshot().unwrap().pending_launches,
                desktop.snapshot().unwrap().model.workspaces.len()
            )
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn ready(s: &Snapshot) -> bool {
    !s.frames.is_empty()
        && s.frames.iter().all(|f| {
            f.snapshot
                .as_ref()
                .is_some_and(|s| s.state == zmux_session_runtime::State::Running)
        })
}

#[test]
fn native_commands_route_input_and_resize_to_a_live_shell() {
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let session = initial.frames[0].session_id;
    send(
        &desktop,
        1,
        Command::Resize {
            session_id: session,
            rows: 13,
            cols: 51,
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.size.rows == 13)
    });
    send(
        &desktop,
        2,
        Command::Input {
            session_id: session,
            text: "printf '\\nDESKTOP_ROUTED_OUTPUT\\n'; stty size\n".into(),
        },
    );
    let observed = wait(&desktop, |s| {
        s.frames[0].snapshot.as_ref().is_some_and(|v| {
            v.viewport
                .contents
                .lines()
                .any(|l| l == "DESKTOP_ROUTED_OUTPUT")
                && v.viewport.contents.lines().any(|l| l == "13 51")
        })
    });
    assert_eq!(observed.last_request_id, 2);
    assert!(observed.last_error.is_none());
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert!(desktop.snapshot().unwrap().closed);
}

#[test]
fn workspace_switch_keeps_hidden_session_alive_and_close_reaps_only_its_pane() {
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let first_pane = initial.model.workspaces[0].focused_pane;
    let first_pid = initial.frames[0].process_id.unwrap();
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "second".into(),
            },
        },
    );
    let second = wait(&desktop, |s| s.model.workspaces.len() == 2 && ready(s));
    let second_pane = second.model.workspaces[1].focused_pane;
    let second_pid = second.frames[0].process_id.unwrap();
    assert_eq!(nix_kill(first_pid), Ok(()));
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::FocusPane {
                pane_id: first_pane,
            },
        },
    );
    wait(&desktop, |s| {
        s.model.focused_workspace == Some(initial.model.workspaces[0].id)
    });
    send(
        &desktop,
        3,
        Command::Action {
            action: Action::ClosePane {
                pane_id: second_pane,
            },
        },
    );
    wait(&desktop, |s| s.model.workspaces.len() == 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while nix_kill(second_pid).is_ok() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(nix_kill(first_pid), Ok(()));
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn split_creates_a_real_independent_session_and_limits_reject_before_mutation() {
    let desktop = desktop();
    wait(&desktop, ready);
    for request in 1..MAX_TERMINALS {
        let before = desktop.snapshot().unwrap();
        let pane = before.model.workspaces[0].focused_pane;
        send(
            &desktop,
            request as u64,
            Command::Action {
                action: Action::SplitPane {
                    pane_id: pane,
                    axis: Axis::Horizontal,
                },
            },
        );
        wait(&desktop, |s| s.frames.len() == request + 1 && ready(s));
    }
    let before = desktop.snapshot().unwrap();
    let pids: std::collections::HashSet<_> = before
        .frames
        .iter()
        .map(|f| f.process_id.unwrap())
        .collect();
    assert_eq!(pids.len(), MAX_TERMINALS);
    send(
        &desktop,
        100,
        Command::Action {
            action: Action::SplitPane {
                pane_id: before.model.workspaces[0].focused_pane,
                axis: Axis::Vertical,
            },
        },
    );
    let rejected = wait(&desktop, |s| s.last_request_id == 100);
    assert!(rejected.last_error.as_ref().unwrap().contains("at most"));
    assert_eq!(rejected.model, before.model);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert!(pids.into_iter().all(|pid| nix_kill(pid).is_err()));
}

fn nix_kill(pid: u32) -> Result<(), nix::errno::Errno> {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None)
}

#[test]
fn close_during_startup_never_reattaches_a_closed_pane() {
    let desktop = desktop();
    let first = wait(&desktop, |s| !s.model.workspaces.is_empty());
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::ClosePane {
                pane_id: first.model.workspaces[0].focused_pane,
            },
        },
    );
    wait(&desktop, |s| {
        s.model.workspaces.is_empty() && s.frames.is_empty() && s.pending_launches == 0
    });
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let final_state = desktop.snapshot().unwrap();
    assert!(final_state.closed);
    assert!(final_state.frames.is_empty());
    assert_eq!(final_state.pending_launches, 0);
    assert!(final_state.cleanup_error.is_none());
}

#[test]
fn tab_switch_move_close_and_reopen_preserve_sibling_process_and_never_replay_input() {
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let pane = initial.model.workspaces[0].focused_pane;
    let first = initial.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    let first_pid = initial.frames[0].process_id.unwrap();
    send(
        &desktop,
        1,
        Command::Input {
            session_id: first.session_id.unwrap(),
            text: "export ZMUX_OWNED_VALUE=first; printf '\\nFIRST_READY\\n'\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.contents.lines().any(|l| l == "FIRST_READY"))
    });
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::CreateSurface { pane_id: pane },
        },
    );
    let second_state = wait(&desktop, |s| {
        s.model.workspaces[0].root.panes()[0].surfaces.len() == 2 && ready(s)
    });
    assert_eq!(
        second_state.frames.len(),
        1,
        "native frames project only selected tabs"
    );
    assert_eq!(
        second_state.control_frames.len(),
        2,
        "inspection retains hidden sessions"
    );
    let second = second_state.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    let second_pid = second_state.frames[0].process_id.unwrap();
    assert_ne!(second_pid, first_pid);
    send(
        &desktop,
        3,
        Command::Input {
            session_id: second.session_id.unwrap(),
            text: "export ZMUX_OWNED_VALUE=second; printf '\\nSECOND_READY\\n'\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.contents.lines().any(|l| l == "SECOND_READY"))
    });
    send(
        &desktop,
        4,
        Command::Action {
            action: Action::SelectSurface {
                surface_id: first.id,
            },
        },
    );
    wait(&desktop, |s| s.frames[0].process_id == Some(first_pid));
    send(
        &desktop,
        5,
        Command::Input {
            session_id: first.session_id.unwrap(),
            text: "printf '\\nPRESERVED_%s\\n' \"$ZMUX_OWNED_VALUE\"\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.contents.lines().any(|l| l == "PRESERVED_first"))
    });
    send(
        &desktop,
        6,
        Command::Action {
            action: Action::SplitPane {
                pane_id: pane,
                axis: Axis::Horizontal,
            },
        },
    );
    let split = wait(&desktop, |s| {
        s.model.workspaces[0].root.panes().len() == 2 && ready(s)
    });
    let destination = split.model.workspaces[0].focused_pane;
    send(
        &desktop,
        7,
        Command::Action {
            action: Action::MoveSurface {
                surface_id: second.id,
                pane_id: destination,
                index: 1,
            },
        },
    );
    let moved = wait(&desktop, |s| s.last_request_id == 7 && ready(s));
    assert_eq!(
        moved
            .control_frames
            .iter()
            .find(|f| f.session_id == second.session_id.unwrap())
            .unwrap()
            .process_id,
        Some(second_pid)
    );
    send(
        &desktop,
        8,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: second.id,
            },
        },
    );
    wait(&desktop, |s| {
        s.last_request_id == 8 && s.model.closed_items.len() == 1
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while nix_kill(second_pid).is_ok() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(nix_kill(first_pid), Ok(()));
    send(
        &desktop,
        9,
        Command::Action {
            action: Action::ReopenClosed { closed_id: None },
        },
    );
    let restored = wait(&desktop, |s| {
        s.last_request_id == 9 && s.model.closed_items.is_empty() && ready(s)
    });
    let tab = restored.model.workspaces[0]
        .root
        .panes()
        .into_iter()
        .find(|p| p.id == destination)
        .unwrap()
        .selected()
        .clone();
    assert_ne!(tab.session_id.unwrap(), second.session_id.unwrap());
    send(
        &desktop,
        10,
        Command::Input {
            session_id: tab.session_id.unwrap(),
            text: "printf '\\nRESTORED_%s\\n' \"${ZMUX_OWNED_VALUE-fresh}\"\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.control_frames.iter().any(|f| {
            f.session_id == tab.session_id.unwrap()
                && f.snapshot
                    .as_ref()
                    .is_some_and(|v| v.viewport.contents.lines().any(|l| l == "RESTORED_fresh"))
        })
    });
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn restore_uses_saved_cwd_and_missing_directory_retains_retryable_history() {
    let directory = std::env::temp_dir().join(format!("zmux-restore-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let directory = std::fs::canonicalize(directory).unwrap();
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let pane = initial.model.workspaces[0].focused_pane;
    let surface = initial.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    // OSC 7 is supplied by a local shell integration; no arbitrary input is retained.
    let command = format!(
        "cd '{}'; printf '\\033]7;file://localhost{}\\007'; printf '\\nCWD_READY\\n'\r",
        directory.display(),
        directory.display()
    );
    send(
        &desktop,
        1,
        Command::Input {
            session_id: surface.session_id.unwrap(),
            text: command,
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.working_directory.as_deref() == directory.to_str())
    });
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: surface.id,
            },
        },
    );
    wait(&desktop, |s| {
        s.model.workspaces.is_empty() && s.pending_launches == 0
    });
    send(
        &desktop,
        3,
        Command::Action {
            action: Action::ReopenClosed { closed_id: None },
        },
    );
    let restored = wait(&desktop, |s| {
        s.last_request_id == 3 && ready(s) && s.model.closed_items.is_empty()
    });
    let restored_session = restored.frames[0].session_id;
    send(
        &desktop,
        4,
        Command::Input {
            session_id: restored_session,
            text: "printf '\\nRESTORED_CWD='; pwd\r".into(),
        },
    );
    let expected = format!("RESTORED_CWD={}", directory.display());
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .is_some_and(|v| v.viewport.contents.lines().any(|l| l == expected))
    });
    let restored_surface = restored.model.workspaces[0].root.panes()[0].selected_surface;
    send(
        &desktop,
        5,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: restored_surface,
            },
        },
    );
    wait(&desktop, |s| s.model.workspaces.is_empty());
    std::fs::remove_dir(&directory).unwrap();
    send(
        &desktop,
        6,
        Command::Action {
            action: Action::ReopenClosed { closed_id: None },
        },
    );
    let failure = wait(&desktop, |s| {
        s.last_request_id == 6
            && s.pending_launches == 0
            && s.last_error.is_some()
            && s.model.workspaces.is_empty()
    });
    assert_eq!(failure.model.closed_items.len(), 1);
    assert!(!failure.model.closed_items[0].restoring);
    assert!(failure.control_frames.is_empty());
    std::fs::create_dir(&directory).unwrap();
    send(
        &desktop,
        7,
        Command::Action {
            action: Action::ReopenClosed { closed_id: None },
        },
    );
    wait(&desktop, |s| {
        s.last_request_id == 7 && ready(s) && s.model.closed_items.is_empty()
    });
    assert_ne!(
        pane,
        desktop.snapshot().unwrap().model.workspaces[0].focused_pane
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn real_terminal_signals_and_explicit_notifications_route_by_surface_with_generation_safe_ack() {
    use zmux_desktop_runtime::attention::AttentionCommand as Attention;
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let first = initial.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    let pane = initial.model.workspaces[0].focused_pane;
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateSurface { pane_id: pane },
        },
    );
    wait(&desktop, |s| s.control_frames.len() == 2 && ready(s));
    send(
        &desktop,
        2,
        Command::Attention {
            action: Attention::AppActive { active: true },
        },
    );
    send(
        &desktop,
        3,
        Command::Input {
            session_id: first.session_id.unwrap(),
            text: "printf '\\nNORMAL_BACKGROUND_OUTPUT\\n'\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.control_frames.iter().any(|f| {
            f.session_id == first.session_id.unwrap()
                && f.snapshot.as_ref().is_some_and(|v| {
                    v.viewport
                        .contents
                        .lines()
                        .any(|l| l == "NORMAL_BACKGROUND_OUTPUT")
                })
        })
    });
    assert!(desktop.snapshot().unwrap().notifications.is_empty());
    send(
        &desktop,
        4,
        Command::Input {
            session_id: first.session_id.unwrap(),
            text: "printf '\\007\\033]777;notify;Agent;Ready for review\\007'\r".into(),
        },
    );
    let attention = wait(&desktop, |s| s.notifications.len() == 2);
    assert!(
        attention
            .notifications
            .iter()
            .all(|n| n.surface_id == first.id && !n.read)
    );
    let bell = attention.notifications[0].id;
    let agent = attention.notifications[1].id;
    send(
        &desktop,
        5,
        Command::Attention {
            action: Attention::Open { id: bell },
        },
    );
    let opened = wait(&desktop, |s| s.last_request_id == 5);
    assert_eq!(
        opened.model.workspaces[0].root.panes()[0].selected_surface,
        first.id
    );
    assert!(opened.notifications[0].read);
    assert!(!opened.notifications[1].read);
    send(
        &desktop,
        6,
        Command::Attention {
            action: Attention::Notify {
                surface_id: first.id,
                title: "Foreground".into(),
                body: String::new(),
            },
        },
    );
    let foreground = wait(&desktop, |s| s.last_request_id == 6);
    assert!(foreground.notifications.last().unwrap().read);
    let last = foreground.notifications.last().unwrap().id;
    send(
        &desktop,
        7,
        Command::Attention {
            action: Attention::Clear { through_id: agent },
        },
    );
    let cleared = wait(&desktop, |s| s.last_request_id == 7);
    assert_eq!(cleared.notifications.len(), 1);
    assert_eq!(cleared.notifications[0].id, last);
    send(
        &desktop,
        8,
        Command::Attention {
            action: Attention::AppActive { active: false },
        },
    );
    send(
        &desktop,
        9,
        Command::Attention {
            action: Attention::Notify {
                surface_id: first.id,
                title: "Background".into(),
                body: String::new(),
            },
        },
    );
    let background = wait(&desktop, |s| s.last_request_id == 9);
    let target = background.notifications.last().unwrap().id;
    assert!(!background.notifications.last().unwrap().read);
    send(
        &desktop,
        10,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: first.id,
            },
        },
    );
    wait(&desktop, |s| s.last_request_id == 10);
    send(
        &desktop,
        11,
        Command::Attention {
            action: Attention::Open { id: target },
        },
    );
    let stale = wait(&desktop, |s| s.last_request_id == 11);
    assert!(stale.last_error.is_some());
    assert!(!stale.notifications.last().unwrap().read);
    assert_ne!(
        stale.model.workspaces[0].root.panes()[0].selected_surface,
        first.id
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn browser_metadata_never_allocates_a_terminal_and_has_independent_capacity() {
    use zmux_desktop_runtime::MAX_BROWSERS;
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let pane = initial.model.workspaces[0].focused_pane;
    let first_pid = initial.frames[0].process_id;
    for index in 0..MAX_BROWSERS {
        let id = index as u64 + 1;
        send(
            &desktop,
            id,
            Command::Action {
                action: Action::CreateBrowser {
                    pane_id: pane,
                    url: "about:blank".into(),
                },
            },
        );
        let state = wait(&desktop, |s| s.last_request_id == id);
        assert!(state.frames.is_empty());
        assert_eq!(state.control_frames.len(), 1);
        assert_eq!(state.control_frames[0].process_id, first_pid);
        assert_eq!(state.pending_launches, 0);
    }
    let before = desktop.snapshot().unwrap();
    send(
        &desktop,
        99,
        Command::Action {
            action: Action::CreateBrowser {
                pane_id: pane,
                url: "about:blank".into(),
            },
        },
    );
    let rejected = wait(&desktop, |s| s.last_request_id == 99);
    assert!(rejected.last_error.is_some());
    assert_eq!(rejected.model, before.model);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn selecting_and_reactivating_acknowledges_only_the_focused_surface() {
    use zmux_desktop_runtime::attention::AttentionCommand as Attention;
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let pane = initial.model.workspaces[0].focused_pane;
    let first = initial.model.workspaces[0].root.panes()[0].selected_surface;
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateSurface { pane_id: pane },
        },
    );
    let created = wait(&desktop, |s| s.last_request_id == 1 && ready(s));
    let second = created.model.workspaces[0].root.panes()[0].selected_surface;
    for (id, surface_id) in [(2, first), (3, second)] {
        send(
            &desktop,
            id,
            Command::Attention {
                action: Attention::Notify {
                    surface_id,
                    title: format!("Attention {id}"),
                    body: String::new(),
                },
            },
        );
    }
    wait(&desktop, |s| s.last_request_id == 3);
    send(
        &desktop,
        4,
        Command::Attention {
            action: Attention::AppActive { active: true },
        },
    );
    let active = wait(&desktop, |s| s.last_request_id == 4);
    assert!(!active.notifications[0].read);
    assert!(
        active.notifications[1].read,
        "activation must acknowledge the focused surface"
    );
    send(
        &desktop,
        5,
        Command::Action {
            action: Action::SelectSurface { surface_id: first },
        },
    );
    let selected = wait(&desktop, |s| s.last_request_id == 5);
    assert!(selected.notifications.iter().all(|n| n.read));
    send(
        &desktop,
        6,
        Command::Attention {
            action: Attention::AppActive { active: false },
        },
    );
    send(
        &desktop,
        7,
        Command::Attention {
            action: Attention::Notify {
                surface_id: first,
                title: "Later generation".into(),
                body: String::new(),
            },
        },
    );
    let later = wait(&desktop, |s| s.last_request_id == 7);
    assert!(!later.notifications.last().unwrap().read);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn scroll_history_updates_idle_snapshots_and_input_returns_to_live_output() {
    let desktop = desktop();
    let initial = wait(&desktop, ready);
    let session = initial.frames[0].session_id;
    send(
        &desktop,
        1,
        Command::Resize {
            session_id: session,
            rows: 4,
            cols: 40,
        },
    );
    wait(&desktop, |s| {
        s.frames[0].snapshot.as_ref().unwrap().viewport.size.rows == 4
    });
    send(&desktop, 2, Command::Input { session_id: session,
        text: "printf '\\033[2J\\033[H'; i=0; while [ $i -lt 20 ]; do printf 'HISTORY_%02d\\n' $i; i=$((i+1)); done\r".into() });
    let output = wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .contains("HISTORY_19")
    });
    assert!(
        !output.frames[0]
            .snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .contains("HISTORY_00")
    );
    let scroll: Command = serde_json::from_value(serde_json::json!({
        "command":"scroll", "session_id":session, "lines":1000
    }))
    .expect("native runtime must accept scroll navigation");
    send(&desktop, 3, scroll);
    let history = wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .contains("HISTORY_00")
    });
    assert!(
        !history.frames[0]
            .snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .contains("HISTORY_19")
    );
    send(
        &desktop,
        4,
        Command::Input {
            session_id: session,
            text: "printf '\\nLIVE_AGAIN\\n'\r".into(),
        },
    );
    wait(&desktop, |s| {
        s.frames[0]
            .snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .lines()
            .any(|l| l == "LIVE_AGAIN")
    });
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
