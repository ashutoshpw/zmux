use serde_json::{Value, json};
use zmux_workspace_core::{Action, Effect, SessionId, WorkspaceStore};

#[test]
fn workspace_move_preserves_live_identity_and_respects_both_windows_selection() {
    use zmux_workspace_core::WindowId;
    for selected_source in [false, true] {
        for empty_destination in [false, true] {
            for focus in [false, true] {
                for destination_focused in [false, true] {
                    let mut store = WorkspaceStore::default();
                    for name in ["first", "moving", "next"] {
                        store
                            .apply(Action::CreateWorkspace { name: name.into() })
                            .unwrap();
                    }
                    let source = store.snapshot().windows[0].id;
                    let moving = store.snapshot().workspaces[1].clone();
                    let selected = store.snapshot().workspaces[usize::from(selected_source)].id;
                    store
                        .apply(Action::SelectWorkspace {
                            workspace_id: selected,
                        })
                        .unwrap();
                    store.apply(Action::CreateWindow { focus: false }).unwrap();
                    let destination = store.snapshot().windows[1].id;
                    let destination_selection = store.snapshot().windows[1].selected_workspace;
                    if empty_destination {
                        store
                            .apply(Action::CloseWorkspace {
                                workspace_id: destination_selection.unwrap(),
                            })
                            .unwrap();
                    }
                    if destination_focused {
                        store
                            .apply(Action::FocusWindow {
                                window_id: destination,
                            })
                            .unwrap();
                    }
                    let before = store.snapshot();
                    let next_id = store.next_identifier();
                    let mut wire = json!({
                        "action":"move_workspace_to_window", "workspace_id":moving.id,
                        "destination_window_id":destination,
                    });
                    if focus {
                        wire["focus"] = json!(true);
                    }
                    let action: Action =
                        serde_json::from_value(wire).expect("shared move action must be available");
                    assert!(store.apply(action).unwrap().effects.is_empty());
                    let after = store.snapshot();
                    assert_eq!(store.next_identifier(), next_id);
                    assert_eq!(
                        after.workspaces, before.workspaces,
                        "move must not recreate any live object"
                    );
                    assert_eq!(
                        after.windows[0].workspace_ids,
                        vec![before.workspaces[0].id, before.workspaces[2].id]
                    );
                    assert_eq!(
                        after.windows[0].selected_workspace,
                        Some(if selected_source {
                            before.workspaces[2].id
                        } else {
                            selected
                        })
                    );
                    assert_eq!(after.windows[1].workspace_ids.last(), Some(&moving.id));
                    assert_eq!(
                        after.windows[1].selected_workspace,
                        if focus || empty_destination {
                            Some(moving.id)
                        } else {
                            destination_selection
                        }
                    );
                    assert_eq!(
                        after.focused_window,
                        if focus {
                            Some(destination)
                        } else {
                            before.focused_window
                        }
                    );
                    let focused = after
                        .windows
                        .iter()
                        .find(|w| Some(w.id) == after.focused_window)
                        .unwrap();
                    assert_eq!(after.focused_workspace, focused.selected_workspace);
                    let valid = after.clone();
                    let invalid: Action = serde_json::from_value(json!({"action":"move_workspace_to_window","workspace_id":moving.id,"destination_window_id":WindowId(u64::MAX),"focus":false})).unwrap();
                    assert!(store.apply(invalid).is_err());
                    assert_eq!(store.snapshot(), valid);
                    assert_eq!(store.next_identifier(), next_id);
                    assert!(after.windows.iter().any(|w| w.id == source));
                }
            }
        }
    }
}

fn window(store: &WorkspaceStore) -> Value {
    let snapshot = serde_json::to_value(store.snapshot()).unwrap();
    let windows = snapshot["windows"]
        .as_array()
        .expect("the production snapshot must publish canonical window ownership");
    assert_eq!(windows.len(), 1);
    assert_eq!(snapshot["focused_window"], windows[0]["id"]);
    assert_eq!(
        snapshot["focused_workspace"],
        windows[0]["selected_workspace"]
    );
    let workspaces: Vec<_> = store.snapshot().workspaces.iter().map(|w| w.id.0).collect();
    assert_eq!(windows[0]["workspace_ids"], json!(workspaces));
    windows[0].clone()
}

#[test]
fn canonical_singleton_tracks_real_workspace_moves_close_and_failed_reopen() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "first".into(),
        })
        .unwrap();
    let first = store.snapshot().workspaces[0].clone();
    // Adding ownership must not renumber the existing first four object IDs.
    assert_eq!(first.id.0, 1);
    let original = first.root.panes()[0].selected().clone();
    assert_eq!(original.id.0, 4);
    let host = window(&store)["id"].clone();
    store
        .apply(Action::CreateWorkspace {
            name: "second".into(),
        })
        .unwrap();
    let second = store.snapshot().workspaces[1].clone();
    assert_eq!(window(&store)["id"], host);
    store
        .apply(Action::SelectWorkspace {
            workspace_id: first.id,
        })
        .unwrap();
    assert_eq!(window(&store)["selected_workspace"], first.id.0);
    store
        .apply(Action::MoveSurface {
            surface_id: original.id,
            pane_id: second.focused_pane,
            index: 1,
        })
        .unwrap();
    assert_eq!(window(&store)["workspace_ids"], json!([second.id.0]));
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0]
            .selected()
            .session_id,
        original.session_id
    );
    store
        .apply(Action::CloseWorkspace {
            workspace_id: second.id,
        })
        .unwrap();
    assert_eq!(window(&store)["id"], host);
    assert_eq!(window(&store)["selected_workspace"], Value::Null);
    let restoring = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    window(&store);
    let sessions: Vec<SessionId> = restoring
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::StartSession { session_id, .. } => Some(*session_id),
            _ => None,
        })
        .collect();
    assert_eq!(sessions.len(), 2);
    store.finish_restore(sessions[0], false);
    assert_eq!(window(&store)["workspace_ids"], json!([]));
    assert!(
        store
            .snapshot()
            .closed_items
            .iter()
            .any(|item| !item.restoring)
    );
    let retry = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    for effect in retry.effects {
        if let Effect::StartSession { session_id, .. } = effect {
            store.finish_restore(session_id, true);
        }
    }
    assert_eq!(window(&store)["id"], host);
    assert_eq!(window(&store)["workspace_ids"].as_array().unwrap().len(), 1);
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn rejected_first_action_does_not_allocate_an_unhosted_window() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    let before = store.snapshot();
    let next = store.next_identifier();
    assert!(
        store
            .apply(Action::CreateWorkspace {
                name: String::new()
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(store.next_identifier(), next);
    assert_eq!(
        serde_json::to_value(store.snapshot()).unwrap()["windows"],
        json!([])
    );
}

#[test]
fn two_window_creation_is_atomic_no_focus_and_close_preserves_other_sessions() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "first".into(),
        })
        .unwrap();
    let initial = store.snapshot();
    let original = initial.windows[0].clone();
    let original_session = initial.workspaces[0].root.panes()[0].session_id();
    let create: Action =
        serde_json::from_value(json!({"action":"create_window","focus":false})).unwrap();
    let result = store.apply(create).unwrap();
    assert_eq!(result.effects.len(), 1);
    let two = store.snapshot();
    assert_eq!(two.windows.len(), 2);
    assert_eq!(two.focused_window, Some(original.id));
    assert_eq!(two.focused_workspace, original.selected_workspace);
    assert_eq!(
        two.workspaces[0].root.panes()[0].session_id(),
        original_session
    );
    let rejected = store.snapshot();
    let next = store.next_identifier();
    let create: Action =
        serde_json::from_value(json!({"action":"create_window","focus":true})).unwrap();
    assert!(store.apply(create).is_err());
    assert_eq!(store.snapshot(), rejected);
    assert_eq!(store.next_identifier(), next);
    let close: Action =
        serde_json::from_value(json!({"action":"close_window","window_id":two.windows[1].id}))
            .unwrap();
    let closed = store.apply(close).unwrap();
    assert_eq!(closed.effects.len(), 1);
    assert!(matches!(closed.effects[0], Effect::StopSession { .. }));
    assert_eq!(store.snapshot().windows, vec![original.clone()]);
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0].session_id(),
        original_session
    );
    let close: Action =
        serde_json::from_value(json!({"action":"close_window","window_id":original.id})).unwrap();
    let before = store.snapshot();
    assert!(
        store.apply(close).is_err(),
        "last native close must preserve the recoverable model"
    );
    assert_eq!(store.snapshot(), before);
}

#[test]
fn window_scopes_resolve_empty_roots_and_fence_explicit_cross_window_objects() {
    use zmux_workspace_core::targets::{CallerContext, TargetKind, TargetScope};
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "first".into(),
        })
        .unwrap();
    store.apply(Action::CreateWindow { focus: false }).unwrap();
    let model = store.snapshot();
    let first = &model.workspaces[0];
    let second = &model.workspaces[1];
    let destination = model.windows[1].id;
    let scope: TargetScope =
        serde_json::from_value(json!({"window":format!("window:{}",destination.0)})).unwrap();
    assert_eq!(
        scope
            .resolve(&model, &CallerContext::default())
            .unwrap()
            .workspace,
        second.id
    );
    let mismatch: TargetScope = serde_json::from_value(json!({"window":format!("window:{}",destination.0),"surface":first.root.panes()[0].selected_surface.0.to_string()})).unwrap();
    assert!(mismatch.resolve(&model, &CallerContext::default()).is_err());
    let caller: CallerContext = serde_json::from_value(json!({"window":model.object_uuid(TargetKind::Window,model.windows[0].id.0),"surface":model.object_uuid(TargetKind::Surface,second.root.panes()[0].selected_surface.0)})).unwrap();
    assert_eq!(
        TargetScope::default()
            .resolve_window(&model, &caller)
            .unwrap(),
        destination
    );
    store
        .apply(Action::CloseWorkspace {
            workspace_id: second.id,
        })
        .unwrap();
    store
        .apply(Action::FocusWindow {
            window_id: destination,
        })
        .unwrap();
    let empty = store.snapshot();
    assert_eq!(empty.focused_workspace, None);
    assert_eq!(
        scope
            .resolve_window(&empty, &CallerContext::default())
            .unwrap(),
        destination
    );
    assert!(scope.resolve(&empty, &CallerContext::default()).is_err());
    // A focused empty window with a populated background window is a valid v4 desktop.
    let document = store.session_document().unwrap();
    document
        .validate(zmux_workspace_core::RestoreLimits {
            terminals: 8,
            browsers: 4,
            files: 16,
        })
        .unwrap();
}

#[test]
fn workspace_navigation_and_focus_history_remain_within_the_owning_window() {
    use zmux_workspace_core::Navigation;
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace { name: "one".into() })
        .unwrap();
    let first_window = store.snapshot().windows[0].id;
    store
        .apply(Action::CreateWorkspace { name: "two".into() })
        .unwrap();
    let first_members = store.snapshot().windows[0].workspace_ids.clone();
    store.apply(Action::CreateWindow { focus: true }).unwrap();
    let other = store.snapshot().windows[1].selected_workspace.unwrap();
    let transition = store
        .apply(Action::NavigateWorkspace {
            workspace_id: first_members[1],
            direction: Navigation::Next,
        })
        .unwrap();
    assert!(transition.effects.is_empty());
    assert_eq!(store.snapshot().focused_window, Some(first_window));
    assert_eq!(store.snapshot().focused_workspace, Some(first_members[0]));
    store
        .apply(Action::NavigateWorkspace {
            workspace_id: first_members[0],
            direction: Navigation::Last,
        })
        .unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(first_members[1]));
    store
        .apply(Action::NavigateWorkspace {
            workspace_id: other,
            direction: Navigation::Next,
        })
        .unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(other));
}
