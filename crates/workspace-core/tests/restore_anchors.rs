use zmux_workspace_core::*;

fn create(store: &mut WorkspaceStore) -> (WorkspaceId, PaneId) {
    store
        .apply(Action::CreateWorkspace {
            name: "work".into(),
        })
        .unwrap();
    let state = store.snapshot();
    let workspace = state.workspaces.last().unwrap();
    (workspace.id, workspace.focused_pane)
}
fn split(store: &mut WorkspaceStore, pane_id: PaneId, axis: Axis) -> PaneId {
    store.apply(Action::SplitPane { pane_id, axis }).unwrap();
    let state = store.snapshot();
    state
        .workspaces
        .iter()
        .find(|w| Some(w.id) == state.focused_workspace)
        .unwrap()
        .focused_pane
}
fn reopen(store: &mut WorkspaceStore, closed_id: Option<u64>) -> Vec<SessionId> {
    store
        .apply(Action::ReopenClosed { closed_id })
        .unwrap()
        .effects
        .into_iter()
        .filter_map(|effect| {
            if let Effect::StartSession { session_id, .. } = effect {
                Some(session_id)
            } else {
                None
            }
        })
        .collect()
}
fn finish(store: &mut WorkspaceStore, sessions: &[SessionId]) {
    for session in sessions {
        assert!(store.finish_restore(*session, true).is_empty());
    }
}

#[test]
fn closed_pane_returns_to_its_original_split_around_unchanged_survivor_subtree() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Horizontal);
    split(&mut store, right, Axis::Vertical);
    store
        .apply(Action::CreateSurface { pane_id: left })
        .unwrap();
    let old = store.snapshot().workspaces[0].clone();
    let selected = old.root.panes()[0].selected_surface;
    let survivors: Vec<_> = old.root.panes()[1..].iter().map(|p| (*p).clone()).collect();
    store.apply(Action::ClosePane { pane_id: left }).unwrap();
    let sessions = reopen(&mut store, None);
    assert_eq!(sessions.len(), 2);
    let state = store.snapshot();
    assert_eq!(state.workspaces.len(), 1);
    let restored = &state.workspaces[0];
    assert_eq!(restored.id, workspace_id);
    let PaneTree::Split {
        axis: Axis::Horizontal,
        first,
        second,
    } = &restored.root
    else {
        panic!("original horizontal placement missing")
    };
    assert!(matches!(first.as_ref(), PaneTree::Leaf { .. }));
    assert!(matches!(
        second.as_ref(),
        PaneTree::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
    assert_eq!(
        second.panes().into_iter().cloned().collect::<Vec<_>>(),
        survivors
    );
    let fresh = first.panes()[0];
    assert_ne!(fresh.id, left);
    assert_eq!(fresh.surfaces.len(), 2);
    assert_ne!(fresh.selected_surface, selected);
    assert_eq!(fresh.selected_surface, fresh.surfaces[1].id);
    assert_eq!(restored.focused_pane, fresh.id);
    finish(&mut store, &sessions);
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn failed_grouped_pane_restore_preserves_live_topology_and_retries_original_placement() {
    let mut store = WorkspaceStore::default();
    let (_, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Vertical);
    store
        .apply(Action::CreateSurface { pane_id: right })
        .unwrap();
    store.apply(Action::ClosePane { pane_id: right }).unwrap();
    let before = store.snapshot().workspaces;
    let started = reopen(&mut store, None);
    assert_eq!(store.snapshot().workspaces.len(), 1);
    store.finish_restore(started[0], true);
    let stopped = store.finish_restore(started[1], false);
    assert_eq!(stopped.len(), 2);
    assert_eq!(store.snapshot().workspaces, before);
    assert!(!store.snapshot().closed_items[0].restoring);
    assert!(store.finish_restore(started[0], true).is_empty());
    let retry = reopen(&mut store, None);
    finish(&mut store, &retry);
    let state = store.snapshot();
    let PaneTree::Split {
        axis: Axis::Vertical,
        first,
        second,
    } = &state.workspaces[0].root
    else {
        panic!("retry lost original placement")
    };
    assert_eq!(first.panes()[0].id, left);
    assert_eq!(second.panes()[0].surfaces.len(), 2);
    assert!(state.closed_items.is_empty());
}

#[test]
fn last_tab_restores_its_removed_pane_and_missing_near_anchor_uses_surviving_ancestor() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Horizontal);
    let bottom = split(&mut store, right, Axis::Vertical);
    let surface = store.snapshot().workspaces[0].root.panes()[2].selected_surface;
    store
        .apply(Action::CloseSurface {
            surface_id: surface,
        })
        .unwrap();
    let closed_id = store.snapshot().closed_items[0].id;
    store.apply(Action::ClosePane { pane_id: right }).unwrap();
    let started = reopen(&mut store, Some(closed_id));
    finish(&mut store, &started);
    let state = store.snapshot();
    assert_eq!(state.workspaces.len(), 1);
    assert_eq!(state.workspaces[0].id, workspace_id);
    let PaneTree::Split {
        axis: Axis::Horizontal,
        first,
        second,
    } = &state.workspaces[0].root
    else {
        panic!("ancestor placement missing")
    };
    assert_eq!(first.panes()[0].id, left);
    assert_ne!(second.panes()[0].id, bottom);
    assert_eq!(second.panes()[0].surfaces.len(), 1);
}

#[test]
fn restored_sibling_ids_remain_usable_and_missing_workspaces_fall_back_safely() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Vertical);
    store.apply(Action::ClosePane { pane_id: right }).unwrap();
    let closed_right = store.snapshot().closed_items[0].id;
    store.apply(Action::ClosePane { pane_id: left }).unwrap();
    let started = reopen(&mut store, None);
    finish(&mut store, &started);
    let recovered = store.snapshot().workspaces[0].id;
    assert_ne!(workspace_id, recovered);
    let started = reopen(&mut store, Some(closed_right));
    finish(&mut store, &started);
    let state = store.snapshot();
    assert_eq!(state.workspaces.len(), 1);
    assert_eq!(state.workspaces[0].id, recovered);
    assert!(matches!(
        state.workspaces[0].root,
        PaneTree::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
    assert_eq!(state.workspaces[0].root.panes().len(), 2);
}

#[test]
fn chained_restoration_aliases_survive_a_failed_retry_for_later_sibling_reopen() {
    let mut store = WorkspaceStore::default();
    let (_, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Horizontal);
    let bottom = split(&mut store, right, Axis::Vertical);
    for pane_id in [bottom, right, left] {
        store.apply(Action::ClosePane { pane_id }).unwrap();
    }
    let closed_bottom = store.snapshot().closed_items[0].id;
    let closed_right = store.snapshot().closed_items[1].id;
    let started = reopen(&mut store, None);
    finish(&mut store, &started);
    let started = reopen(&mut store, Some(closed_right));
    finish(&mut store, &started);
    let fresh_right = store.snapshot().workspaces[0].root.panes()[1].id;
    store
        .apply(Action::ClosePane {
            pane_id: fresh_right,
        })
        .unwrap();
    let failed = reopen(&mut store, None);
    store.finish_restore(failed[0], false);
    let started = reopen(&mut store, None);
    finish(&mut store, &started);
    let survivor = store.snapshot().workspaces[0].root.panes()[1].clone();
    let started = reopen(&mut store, Some(closed_bottom));
    finish(&mut store, &started);
    let state = store.snapshot();
    let PaneTree::Split {
        axis: Axis::Horizontal,
        second,
        ..
    } = &state.workspaces[0].root
    else {
        panic!("outer placement lost")
    };
    let PaneTree::Split {
        axis: Axis::Vertical,
        first,
        ..
    } = second.as_ref()
    else {
        panic!("nearest remapped sibling anchor lost after retry")
    };
    assert_eq!(first.panes()[0], &survivor);
}

#[test]
fn missing_anchors_fall_back_inside_the_live_workspace_without_touching_survivor() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Horizontal);
    store.apply(Action::ClosePane { pane_id: left }).unwrap();
    let closed_left = store.snapshot().closed_items[0].id;
    let replacement = split(&mut store, right, Axis::Vertical);
    store.apply(Action::ClosePane { pane_id: right }).unwrap();
    let survivor = store.snapshot().workspaces[0].root.panes()[0].clone();
    assert_eq!(survivor.id, replacement);
    let started = reopen(&mut store, Some(closed_left));
    finish(&mut store, &started);
    let state = store.snapshot();
    assert_eq!(state.workspaces.len(), 1);
    assert_eq!(state.workspaces[0].id, workspace_id);
    assert_eq!(state.workspaces[0].root.panes().len(), 2);
    assert!(state.workspaces[0].root.panes().contains(&&survivor));
}

#[test]
fn anchored_restore_rejects_excessive_depth_atomically_and_can_retry_after_space_is_freed() {
    let mut store = WorkspaceStore::default();
    let (_, left) = create(&mut store);
    let right = split(&mut store, left, Axis::Horizontal);
    store.apply(Action::ClosePane { pane_id: left }).unwrap();
    let mut added = Vec::new();
    for _ in 0..MAX_SPLIT_DEPTH {
        added.push(split(&mut store, right, Axis::Vertical));
    }
    let before = store.snapshot();
    assert!(matches!(
        store.apply(Action::ReopenClosed { closed_id: None }),
        Err(ActionError::Capacity {
            resource: "split depth",
            ..
        })
    ));
    assert_eq!(store.snapshot(), before);
    let closed_left = before.closed_items[0].id;
    store
        .apply(Action::ClosePane {
            pane_id: added.pop().unwrap(),
        })
        .unwrap();
    let started = reopen(&mut store, Some(closed_left));
    finish(&mut store, &started);
    assert_eq!(store.snapshot().workspaces.len(), 1);
}
