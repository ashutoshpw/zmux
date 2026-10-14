use zmux_workspace_core::*;

fn workspace(store: &mut WorkspaceStore) -> (WorkspaceId, PaneId, SurfaceId, SessionId) {
    store
        .apply(Action::CreateWorkspace {
            name: "work".into(),
        })
        .unwrap();
    let state = store.snapshot();
    let w = state.workspaces.last().unwrap();
    let p = w.root.panes()[0];
    (w.id, p.id, p.selected_surface, p.session_id().unwrap())
}
fn add_tab(store: &mut WorkspaceStore, pane: PaneId) -> Surface {
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    store
        .snapshot()
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .find(|p| p.id == pane)
        .unwrap()
        .selected()
        .clone()
}
fn sessions(effects: &[Effect]) -> Vec<SessionId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StartSession { session_id, .. } => Some(*session_id),
            _ => None,
        })
        .collect()
}

#[test]
fn tabs_move_and_reorder_without_restarting_sessions_and_close_only_their_owner() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, first_session) = workspace(&mut store);
    let second = add_tab(&mut store, pane);
    store
        .apply(Action::SelectSurface { surface_id: first })
        .unwrap();
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0]
            .session_id()
            .unwrap(),
        first_session
    );
    assert!(
        store
            .apply(Action::ReorderSurface {
                surface_id: second.id,
                index: 0
            })
            .unwrap()
            .effects
            .is_empty()
    );
    let (other, destination, _, _) = workspace(&mut store);
    let result = store
        .apply(Action::MoveSurface {
            surface_id: second.id,
            pane_id: destination,
            index: 1,
        })
        .unwrap();
    assert!(result.effects.is_empty());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.focused_workspace, Some(other));
    assert_eq!(
        snapshot.workspaces[0].root.panes()[0].selected_surface,
        first
    );
    assert_eq!(snapshot.workspaces[1].root.panes()[0].selected(), &second);
    let closed = store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    assert_eq!(
        closed.effects,
        vec![Effect::StopSession {
            session_id: second.session_id.unwrap()
        }]
    );
    assert_eq!(store.snapshot().workspaces.len(), 2);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::SelectSurface {
                surface_id: second.id
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
}

#[test]
fn reopen_consumes_history_only_after_launch_and_failed_restore_is_retriable() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, first_session) = workspace(&mut store);
    let second = add_tab(&mut store, pane);
    store.set_session_cwd(second.session_id.unwrap(), "/tmp/original directory".into());
    store
        .apply(Action::RenameSurface {
            surface_id: second.id,
            name: "build".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    let history_id = store.snapshot().closed_items[0].id;
    let reopen = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(
        matches!(&reopen.effects[0], Effect::StartSession { cwd: Some(cwd), .. } if cwd == "/tmp/original directory")
    );
    let session = sessions(&reopen.effects)[0];
    let state = store.snapshot();
    let restored = state.workspaces[0].root.panes()[0].selected();
    assert_eq!(restored.title, "build");
    assert_ne!(restored.id, second.id);
    assert_ne!(session, second.session_id.unwrap());
    assert!(state.closed_items[0].restoring);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::ReopenClosed {
                closed_id: Some(history_id)
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(
        store.finish_restore(session, false),
        vec![Effect::StopSession {
            session_id: session
        }]
    );
    let state = store.snapshot();
    assert_eq!(state.workspaces[0].root.panes()[0].selected_surface, first);
    assert_eq!(
        state.workspaces[0].root.panes()[0].session_id().unwrap(),
        first_session
    );
    assert!(!state.closed_items[0].restoring);
    let retry = store
        .apply(Action::ReopenClosed {
            closed_id: Some(history_id),
        })
        .unwrap();
    let replacement = sessions(&retry.effects)[0];
    assert_ne!(session, replacement);
    assert!(store.finish_restore(replacement, true).is_empty());
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn selected_history_and_last_tab_fallback_preserve_order_without_stealing_live_ids() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    store
        .apply(Action::RenameSurface {
            surface_id: first,
            name: "A".into(),
        })
        .unwrap();
    let second = add_tab(&mut store, pane);
    store
        .apply(Action::RenameSurface {
            surface_id: second.id,
            name: "B".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface { surface_id: first })
        .unwrap();
    let a_history = store.snapshot().closed_items[0].id;
    store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    assert!(store.snapshot().workspaces.is_empty());
    let (_, live_pane, live_surface, _) = workspace(&mut store);
    for (closed_id, title) in [(Some(a_history), "A"), (None, "B")] {
        let result = store.apply(Action::ReopenClosed { closed_id }).unwrap();
        let session = sessions(&result.effects)[0];
        store.finish_restore(session, true);
        let state = store.snapshot();
        let restored = state.workspaces.last().unwrap().root.panes()[0].selected();
        assert_eq!(restored.title, title);
        assert_ne!(restored.id, live_surface);
        assert_eq!(state.workspaces[0].root.panes()[0].id, live_pane);
    }
    assert_eq!(store.snapshot().workspaces.len(), 2);
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn grouped_workspace_restore_preserves_layout_and_rolls_back_all_provisional_sessions() {
    let mut store = WorkspaceStore::default();
    let (wid, pane, _, _) = workspace(&mut store);
    add_tab(&mut store, pane);
    store
        .apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Vertical,
        })
        .unwrap();
    let old = store.snapshot().workspaces[0].clone();
    store
        .apply(Action::CloseWorkspace { workspace_id: wid })
        .unwrap();
    assert_eq!(store.snapshot().closed_items.len(), 1);
    let result = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    let started = sessions(&result.effects);
    assert_eq!(started.len(), 3);
    assert!(store.finish_restore(started[0], true).is_empty());
    let cleanup = store.finish_restore(started[1], false);
    assert_eq!(cleanup.len(), 3);
    assert!(store.snapshot().workspaces.is_empty());
    assert!(!store.snapshot().closed_items[0].restoring);
    // A late completion for the aborted group cannot consume history or attach anything.
    assert!(store.finish_restore(started[2], true).is_empty());
    let retry = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    for id in sessions(&retry.effects) {
        store.finish_restore(id, true);
    }
    let state = store.snapshot();
    let restored = &state.workspaces[0];
    assert!(matches!(
        restored.root,
        PaneTree::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
    assert_eq!(
        restored
            .root
            .panes()
            .iter()
            .map(|p| p.surfaces.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(restored.focused_pane, restored.root.panes()[1].id);
    assert_ne!(restored.id, old.id);
    assert!(state.closed_items.is_empty());
}

#[test]
fn batch_close_has_one_history_entry_and_invalid_moves_and_capacity_are_atomic() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    for _ in 1..MAX_SURFACES_PER_PANE {
        add_tab(&mut store, pane);
    }
    let before = store.snapshot();
    for action in [
        Action::CreateSurface { pane_id: pane },
        Action::MoveSurface {
            surface_id: first,
            pane_id: PaneId(999999),
            index: 0,
        },
        Action::ReorderSurface {
            surface_id: first,
            index: MAX_SURFACES_PER_PANE,
        },
    ] {
        assert!(store.apply(action).is_err());
        assert_eq!(store.snapshot(), before);
    }
    let result = store
        .apply(Action::CloseOtherSurfaces { surface_id: first })
        .unwrap();
    assert_eq!(result.effects.len(), MAX_SURFACES_PER_PANE - 1);
    assert_eq!(store.snapshot().closed_items.len(), 1);
    let restored = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    for session in sessions(&restored.effects) {
        store.finish_restore(session, true);
    }
    let after = store.snapshot();
    assert_eq!(
        after.workspaces[0].root.panes()[0].surfaces.len(),
        MAX_SURFACES_PER_PANE
    );
    assert!(after.closed_items.is_empty());
}

#[test]
fn close_history_evicts_old_entries_and_single_tab_reorder_is_safe() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    assert!(
        store
            .apply(Action::ReorderSurface {
                surface_id: first,
                index: 0
            })
            .unwrap()
            .effects
            .is_empty()
    );
    for _ in 0..MAX_CLOSED_ITEMS + 2 {
        let tab = add_tab(&mut store, pane);
        store
            .apply(Action::CloseSurface { surface_id: tab.id })
            .unwrap();
    }
    let state = store.snapshot();
    assert_eq!(state.closed_items.len(), MAX_CLOSED_ITEMS);
    assert!(state.closed_items.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(state.workspaces[0].root.panes()[0].selected_surface, first);
}

#[test]
fn browser_tabs_have_no_pty_and_restore_the_observed_url_without_replaying_navigation() {
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    let created = store
        .apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.com/initial".into(),
        })
        .unwrap();
    assert!(created.effects.is_empty());
    let browser = store.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert_eq!(browser.kind, SurfaceKind::Browser);
    assert_eq!(browser.session_id, None);
    store
        .apply(Action::NavigateBrowser {
            surface_id: browser.id,
            url: "https://example.com/next".into(),
        })
        .unwrap();
    let generation = store.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .navigation;
    store
        .apply(Action::ObserveBrowser {
            surface_id: browser.id,
            url: "https://example.com/redirected".into(),
            title: "Docs".into(),
        })
        .unwrap();
    let before = store.snapshot();
    assert_eq!(
        before.workspaces[0].root.panes()[0].selected().navigation,
        generation
    );
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,test",
        "https://user:secret@example.com/",
        "https://",
    ] {
        assert!(
            store
                .apply(Action::NavigateBrowser {
                    surface_id: browser.id,
                    url: url.into()
                })
                .is_err()
        );
        assert_eq!(store.snapshot(), before);
    }
    let closed = store
        .apply(Action::CloseSurface {
            surface_id: browser.id,
        })
        .unwrap();
    assert!(closed.effects.is_empty());
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0].selected_surface,
        terminal
    );
    let restored = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(restored.effects.is_empty());
    let state = store.snapshot();
    let restored = state.workspaces[0].root.panes()[0].selected();
    assert_eq!(
        restored.url.as_deref(),
        Some("https://example.com/redirected")
    );
    assert_eq!(restored.title, "Docs");
    assert_ne!(restored.id, browser.id);
    assert!(state.closed_items.is_empty());
}
