use serde_json::{Value, json};
use zmux_workspace_core::{Action, Effect, SessionId, WorkspaceStore};

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
