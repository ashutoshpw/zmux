//! Public shared actions: group topology must preserve the existing live objects.
use serde_json::{Value, json};
use zmux_workspace_core::{Action, WorkspaceStore};

fn group(store: &mut WorkspaceStore, command: Value) -> Result<(), String> {
    let action: Action =
        serde_json::from_value(json!({"action":"workspace_group", "command":command}))
            .map_err(|e| e.to_string())?;
    store
        .apply(action)
        .map(|t| assert!(t.effects.is_empty()))
        .map_err(|e| e.to_string())
}
#[test]
fn existing_members_form_a_stable_contiguous_group_and_anchor_close_promotes_without_restarting_survivors()
 {
    let mut store = WorkspaceStore::default();
    for name in ["A", "B", "C", "D"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let before = store.snapshot();
    let ids = before.windows[0].workspace_ids.clone();
    let next = store.next_identifier();
    group(
        &mut store,
        json!({"operation":"create", "window_id":before.windows[0].id,
        "name":"Project", "workspace_ids":[ids[0],ids[2]], "external_id":"controller-project"}),
    )
    .expect("shared group action must group existing workspaces");
    let snapshot = serde_json::to_value(store.snapshot()).unwrap();
    assert_eq!(
        snapshot["windows"][0]["workspace_ids"],
        json!([ids[0], ids[2], ids[1], ids[3]])
    );
    assert_eq!(
        snapshot["workspace_groups"][0]["workspace_ids"],
        json!([ids[0], ids[2]])
    );
    let id = snapshot["workspace_groups"][0]["id"].clone();
    assert_eq!(id, json!(next));
    assert_eq!(store.snapshot().workspaces, before.workspaces);
    assert_eq!(store.snapshot().focused_workspace, before.focused_workspace);
    group(
        &mut store,
        json!({"operation":"create", "window_id":before.windows[0].id,
        "name":"Repeated", "workspace_ids":[ids[1]], "external_id":"controller-project"}),
    )
    .unwrap();
    assert_eq!(store.next_identifier(), next + 1);
    assert_eq!(
        serde_json::to_value(store.snapshot()).unwrap()["workspace_groups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let saved = store.snapshot();
    let allocator = store.next_identifier();
    let action: Action = serde_json::from_value(json!({"action":"reorder_workspaces","window_id":before.windows[0].id,
        "order":{"operation":"single","workspace_id":ids[1],"placement":{"placement":"index","index":1}}})).unwrap();
    assert!(
        store.apply(action).is_err(),
        "ordinary reordering must not split the group"
    );
    assert_eq!(store.snapshot(), saved);
    assert_eq!(store.next_identifier(), allocator);
    let close = store
        .apply(Action::CloseWorkspace {
            workspace_id: ids[0],
        })
        .unwrap();
    assert_eq!(close.effects.len(), 1);
    let after = store.snapshot();
    assert_eq!(
        after.workspaces.iter().find(|w| w.id == ids[2]),
        before.workspaces.iter().find(|w| w.id == ids[2])
    );
    let groups = serde_json::to_value(&after).unwrap()["workspace_groups"].clone();
    assert_eq!(groups[0]["id"], id);
    assert_eq!(groups[0]["workspace_ids"], json!([ids[2]]));
    group(&mut store, json!({"operation":"ungroup","group_id":id})).unwrap();
    assert!(
        serde_json::to_value(store.snapshot()).unwrap()["workspace_groups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.snapshot().workspaces, after.workspaces);
}
