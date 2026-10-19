use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{Command, Desktop, Request, Shell, Snapshot};
use zmux_workspace_core::{Action, PaneId, SessionId, SurfaceId};

fn desktop() -> Desktop {
    Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap()
}

fn read_until(
    desktop: &Desktop,
    deadline: Instant,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Arc<Snapshot> {
    loop {
        let snapshot = desktop.snapshot().unwrap();
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "desktop state did not settle: revision={}, last request={}, error={:?}, terminal={:?}",
            snapshot.revision,
            snapshot.last_request_id,
            snapshot.last_error,
            snapshot
                .control_frames
                .iter()
                .filter_map(|frame| frame
                    .snapshot
                    .as_ref()
                    .map(|terminal| terminal.viewport.contents.clone()))
                .collect::<Vec<_>>(),
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn send(desktop: &Desktop, request_id: u64, payload: Command) -> Arc<Snapshot> {
    desktop
        .submit(Request {
            version: 1,
            request_id,
            payload,
        })
        .unwrap();
    read_until(
        desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| snapshot.last_request_id == request_id,
    )
}

fn terminal_owner(snapshot: &Snapshot) -> (PaneId, SurfaceId, SessionId) {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .find_map(|pane| {
            pane.surfaces
                .iter()
                .find(|surface| surface.session_id.is_some())
                .map(|surface| (pane.id, surface.id, surface.session_id.unwrap()))
        })
        .expect("starter terminal owner")
}

fn terminal_contents(snapshot: &Snapshot, session_id: SessionId) -> Option<&str> {
    snapshot
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)
        .and_then(|frame| frame.snapshot.as_ref())
        .map(|frame| frame.viewport.contents.as_str())
}

fn terminal_viewport(
    snapshot: &Snapshot,
    session_id: SessionId,
) -> Option<(u64, u16, u16, Vec<zmux_terminal_adapter::LinkSpan>)> {
    let viewport = snapshot
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)?
        .snapshot
        .as_ref()?
        .viewport
        .clone();
    Some((
        viewport.viewport_generation,
        viewport.size.rows,
        viewport.size.cols,
        viewport.links,
    ))
}

fn browser_in_pane(snapshot: &Snapshot, pane_id: PaneId, url: &str) -> bool {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .find(|pane| pane.id == pane_id)
        .is_some_and(|pane| {
            pane.surfaces.iter().any(|surface| {
                surface.kind == zmux_workspace_core::SurfaceKind::Browser
                    && surface.url.as_deref() == Some(url)
            })
        })
}

fn browser_count(snapshot: &Snapshot) -> usize {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .filter(|surface| surface.kind == zmux_workspace_core::SurfaceKind::Browser)
        .count()
}

#[test]
fn explicit_cli_opener_routes_to_the_origin_and_private_events_stay_ephemeral() {
    let desktop = desktop();
    let initial = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            snapshot
                .control_frames
                .iter()
                .any(|frame| frame.snapshot.is_some())
        },
    );
    let (origin_pane, _origin_surface, origin_session) = terminal_owner(&initial);

    send(
        &desktop,
        1,
        Command::Input {
            session_id: origin_session,
            text: "stty -echo; printf 'ECHO_%s\\n' OFF >/dev/tty\n".into(),
        },
    );
    read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            terminal_contents(snapshot, origin_session)
                .is_some_and(|contents| contents.contains("ECHO_OFF"))
        },
    );

    let printed_url = "https://example.test/printed-only";
    send(
        &desktop,
        2,
        Command::Input {
            session_id: origin_session,
            text: format!(
                "printf '%s\\n' '{printed_url}'; printf 'PLAIN_%s\\n' PRINTED >/dev/tty\n"
            ),
        },
    );
    let printed = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            terminal_contents(snapshot, origin_session)
                .is_some_and(|contents| contents.contains("PLAIN_PRINTED"))
        },
    );
    assert_eq!(
        browser_count(&printed),
        0,
        "plain URL output is not an open request"
    );

    let other_workspace = send(
        &desktop,
        3,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Other focus".into(),
            },
        },
    );
    let focused_elsewhere = other_workspace.model.focused_workspace;
    assert!(
        focused_elsewhere.is_some_and(|focused| focused != other_workspace.model.workspaces[0].id),
        "fixture must change focus before the PTY emits its request"
    );

    let login_url = "https://example.test/device-login";
    let fixture = format!(
        "if [ \"${{BROWSER-}}\" = \"${{ZMUX_SHELL_INTEGRATION_DIRECTORY-}}/bin/zmux-open-url\" ]; then printf 'GH_FIXTURE_%s\\n' READY >/dev/tty; \"$BROWSER\" '{login_url}'; else printf 'GH_FIXTURE_%s\\n' MISSING >/dev/tty; fi\n"
    );
    send(
        &desktop,
        4,
        Command::Input {
            session_id: origin_session,
            text: fixture,
        },
    );

    let fixture_called = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            terminal_contents(snapshot, origin_session).is_some_and(|contents| {
                contents.contains("GH_FIXTURE_READY") || contents.contains("GH_FIXTURE_MISSING")
            })
        },
    );
    assert!(
        terminal_contents(&fixture_called, origin_session)
            .is_some_and(|contents| contents.contains("GH_FIXTURE_READY")),
        "the fixture only invokes a shell-owned opener when Zmux installed the private helper"
    );
    let opened = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(8),
        |snapshot| browser_in_pane(snapshot, origin_pane, login_url),
    );
    assert!(browser_in_pane(&opened, origin_pane, login_url));
    assert!(
        opened
            .model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .filter(|pane| pane.id != origin_pane)
            .flat_map(|pane| &pane.surfaces)
            .all(|surface| surface.url.as_deref() != Some(login_url)),
        "the opener must use its live PTY pane, not the currently focused workspace"
    );

    let browser_bytes = serde_json::to_vec(opened.as_ref()).unwrap();
    let native_frame_bytes = serde_json::to_vec(&opened.frames).unwrap();
    let control_frame_bytes = serde_json::to_vec(&opened.control_frames).unwrap();
    for bytes in [browser_bytes, native_frame_bytes, control_frame_bytes] {
        assert!(
            !bytes
                .windows(b"zmux-open;v1;".len())
                .any(|window| window == b"zmux-open;v1;"),
            "private opener protocol bytes must not enter published frames"
        );
        assert!(
            !bytes.windows(32).any(|window| {
                window
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            }),
            "the private per-session nonce must not enter serialized desktop state"
        );
    }
}

#[test]
fn command_click_revalidates_live_owner_and_same_size_resize_generation() {
    let desktop = desktop();
    let initial = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            snapshot
                .control_frames
                .iter()
                .any(|frame| frame.snapshot.is_some())
        },
    );
    let (origin_pane, origin_surface, origin_session) = terminal_owner(&initial);
    send(
        &desktop,
        1,
        Command::Input {
            session_id: origin_session,
            text: "stty -echo; printf '%s\\n' 'https://example.test/click-target'; printf 'LINK_%s\\n' READY >/dev/tty\n"
                .to_owned(),
        },
    );
    let linked = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            terminal_viewport(snapshot, origin_session).is_some_and(|(_, _, _, links)| {
                links
                    .iter()
                    .any(|link| link.url == "https://example.test/click-target")
            })
        },
    );
    let (old_generation, rows, cols, links) =
        terminal_viewport(&linked, origin_session).expect("terminal viewport");
    let link = links
        .iter()
        .find(|link| link.url == "https://example.test/click-target")
        .expect("visible HTTP link")
        .clone();
    let request =
        |identity_namespace, pane_id, session_id, viewport_generation| Command::OpenTerminalLink {
            request: zmux_desktop_runtime::TerminalLinkRequest {
                identity_namespace,
                surface_id: origin_surface,
                pane_id,
                session_id,
                viewport_generation,
                row: link.row,
                column: link.start_column,
                expected_url: link.url.clone(),
            },
        };
    let mut request_id = 2;
    for invalid in [
        request(
            uuid::Uuid::new_v4(),
            origin_pane,
            origin_session,
            old_generation,
        ),
        request(
            linked.model.identity_namespace,
            origin_pane,
            SessionId(origin_session.0.saturating_add(10_000)),
            old_generation,
        ),
        request(
            linked.model.identity_namespace,
            PaneId(origin_pane.0.saturating_add(10_000)),
            origin_session,
            old_generation,
        ),
    ] {
        let rejected = send(&desktop, request_id, invalid);
        assert_eq!(
            browser_count(&rejected),
            0,
            "stale owner must not open a browser"
        );
        request_id += 1;
    }

    send(
        &desktop,
        request_id,
        Command::Resize {
            session_id: origin_session,
            rows,
            cols,
        },
    );
    request_id += 1;
    let resized = read_until(
        &desktop,
        Instant::now() + Duration::from_secs(5),
        |snapshot| {
            terminal_viewport(snapshot, origin_session)
                .is_some_and(|(generation, _, _, _)| generation > old_generation)
        },
    );
    let (new_generation, _, _, _) = terminal_viewport(&resized, origin_session).unwrap();
    assert_eq!(browser_count(&resized), 0);

    let stale = send(
        &desktop,
        request_id,
        request(
            resized.model.identity_namespace,
            origin_pane,
            origin_session,
            old_generation,
        ),
    );
    assert_eq!(
        browser_count(&stale),
        0,
        "old viewport generation must be rejected"
    );
    request_id += 1;
    let opened = send(
        &desktop,
        request_id,
        request(
            resized.model.identity_namespace,
            origin_pane,
            origin_session,
            new_generation,
        ),
    );
    assert!(browser_in_pane(
        &opened,
        origin_pane,
        "https://example.test/click-target"
    ));

    let split = send(
        &desktop,
        request_id + 1,
        Command::Action {
            action: Action::SplitPane {
                pane_id: origin_pane,
                axis: zmux_workspace_core::Axis::Vertical,
            },
        },
    );
    let moved_to = split
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .find(|pane| pane.id != origin_pane)
        .expect("split pane")
        .id;
    let moved = send(
        &desktop,
        request_id + 2,
        Command::Action {
            action: Action::MoveSurface {
                surface_id: origin_surface,
                pane_id: moved_to,
                index: split
                    .model
                    .workspaces
                    .iter()
                    .flat_map(|workspace| workspace.root.panes())
                    .find(|pane| pane.id == moved_to)
                    .unwrap()
                    .surfaces
                    .len(),
            },
        },
    );
    let current_generation = terminal_viewport(&moved, origin_session)
        .expect("moved terminal viewport")
        .0;
    let moved_rejected = send(
        &desktop,
        request_id + 3,
        request(
            moved.model.identity_namespace,
            origin_pane,
            origin_session,
            current_generation,
        ),
    );
    assert_eq!(
        browser_count(&moved_rejected),
        1,
        "moved pane binding must reject stale owner"
    );

    send(
        &desktop,
        request_id + 4,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: origin_surface,
            },
        },
    );
    let closed_rejected = send(
        &desktop,
        request_id + 5,
        request(
            moved.model.identity_namespace,
            moved_to,
            origin_session,
            current_generation,
        ),
    );
    assert_eq!(
        browser_count(&closed_rejected),
        1,
        "closed origin must reject stale link"
    );
}
