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

#[test]
fn group_membership_anchor_and_block_moves_preserve_window_focus_and_reject_cross_window_add_atomically()
 {
    let mut store = WorkspaceStore::default();
    for name in ["A", "B", "C", "D", "E"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let before = store.snapshot();
    let window = before.windows[0].id;
    let ids = before.windows[0].workspace_ids.clone();
    let g = store.next_identifier();
    group(&mut store,json!({"operation":"create","window_id":window,"name":"G","workspace_ids":[ids[0],ids[1],ids[2]]})).unwrap();
    let h = store.next_identifier();
    group(
        &mut store,
        json!({"operation":"create","window_id":window,"name":"H","workspace_ids":[ids[3],ids[4]]}),
    )
    .unwrap();
    for command in [
        json!({"operation":"add","group_id":h,"workspace_id":ids[1]}),
        json!({"operation":"set_anchor","group_id":h,"workspace_id":ids[4]}),
        json!({"operation":"set_collapsed","group_id":h,"collapsed":true}),
        json!({"operation":"rename","group_id":h,"name":"Renamed"}),
        json!({"operation":"move","group_id":g,"index":99}),
        json!({"operation":"remove","workspace_id":ids[4]}),
    ] {
        group(&mut store, command).unwrap();
    }
    assert_eq!(
        store.snapshot().windows[0].workspace_ids,
        vec![ids[3], ids[1], ids[4], ids[0], ids[2]]
    );
    assert_eq!(store.snapshot().focused_workspace, before.focused_workspace);
    assert_eq!(store.snapshot().workspaces, before.workspaces);
    store.apply(Action::CreateWindow { focus: false }).unwrap();
    let other = store.snapshot().windows[1].clone();
    let snapshot = store.snapshot();
    let next = store.next_identifier();
    for command in [
        json!({"operation":"add","group_id":g,"workspace_id":other.workspace_ids[0]}),
        json!({"operation":"set_anchor","group_id":g,"workspace_id":ids[4]}),
        json!({"operation":"create","window_id":window,"name":"Empty","workspace_ids":[]}),
        json!({"operation":"create","window_id":window,"name":"Duplicate","workspace_ids":[ids[4],ids[4]]}),
        json!({"operation":"move","group_id":g,"index":1}),
    ] {
        assert!(group(&mut store, command).is_err());
        assert_eq!(store.snapshot(), snapshot);
        assert_eq!(store.next_identifier(), next);
    }
    let moved = store
        .apply(Action::MoveWorkspaceToWindow {
            workspace_id: ids[3],
            destination_window_id: other.id,
            index: None,
            focus: false,
        })
        .unwrap();
    assert!(moved.effects.is_empty());
    let after = serde_json::to_value(store.snapshot()).unwrap();
    assert_eq!(
        after["workspace_groups"][1]["workspace_ids"],
        json!([ids[1]])
    );
    assert_eq!(store.snapshot().focused_workspace, before.focused_workspace);
    store
        .apply(Action::CloseWorkspace {
            workspace_id: ids[1],
        })
        .unwrap();
    assert_eq!(store.snapshot().workspace_groups.len(), 1);
    assert_eq!(store.snapshot().workspace_groups[0].id.0, g);
}

#[test]
fn version_nine_groups_round_trip_stable_logical_identity_and_reject_malformed_or_downgraded_membership()
 {
    use zmux_workspace_core::{RestoreLimits, SessionDocument, SessionId};
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::from_u128(77));
    for name in ["A", "B", "C", "D"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let before = store.snapshot();
    let ids = before.windows[0].workspace_ids.clone();
    let window = before.windows[0].id;
    let first = store.next_identifier();
    group(&mut store,json!({"operation":"create","window_id":window,"name":"G","workspace_ids":[ids[0],ids[2]],"external_id":"one"})).unwrap();
    group(
        &mut store,
        json!({"operation":"set_collapsed","group_id":first,"collapsed":true}),
    )
    .unwrap();
    group(&mut store,json!({"operation":"create","window_id":window,"name":"G","workspace_ids":[ids[1],ids[3]],"external_id":"two"})).unwrap();
    let wire = serde_json::to_value(store.session_document().unwrap()).unwrap();
    assert_eq!(wire["version"], 9);
    let limits = RestoreLimits {
        terminals: 8,
        browsers: 4,
    };
    let validate = |value: Value| {
        serde_json::from_value::<SessionDocument>(value)
            .map_err(|e| e.to_string())
            .and_then(|d| d.validate(limits).map_err(|e| e.to_string()))
    };
    let prepared = WorkspaceStore::prepare_session_restore(
        validate(wire.clone()).unwrap(),
        &[
            SessionId(100),
            SessionId(101),
            SessionId(102),
            SessionId(103),
        ],
        None,
    )
    .unwrap();
    assert_eq!(
        prepared.store.snapshot().workspace_groups,
        store.snapshot().workspace_groups
    );
    assert_eq!(prepared.store.snapshot().windows, store.snapshot().windows);
    assert_eq!(
        prepared.store.snapshot().identity_namespace,
        before.identity_namespace
    );
    assert!(
        prepared
            .effects
            .iter()
            .all(|e| matches!(e, zmux_workspace_core::Effect::StartSession { .. }))
    );
    for mutation in [
        "missing",
        "null",
        "empty_group",
        "duplicate_member",
        "foreign_window",
        "split_block",
        "cross_kind_id",
        "duplicate_external",
        "downgrade",
    ] {
        let mut bad = wire.clone();
        match mutation {
            "missing" => {
                bad.as_object_mut().unwrap().remove("workspace_groups");
            }
            "null" => bad["workspace_groups"] = Value::Null,
            "empty_group" => bad["workspace_groups"][0]["workspace_ids"] = json!([]),
            "duplicate_member" => {
                bad["workspace_groups"][0]["workspace_ids"] = json!([ids[0], ids[0]])
            }
            "foreign_window" => bad["workspace_groups"][0]["window_id"] = json!(999),
            "split_block" => bad["window_layout"]["windows"][0]["workspace_ids"] = json!(ids),
            "cross_kind_id" => bad["workspace_groups"][0]["id"] = json!(ids[0]),
            "duplicate_external" => bad["workspace_groups"][1]["external_id"] = json!("one"),
            "downgrade" => bad["version"] = json!(8),
            _ => unreachable!(),
        }
        assert!(validate(bad).is_err(), "malformed group ingress {mutation}");
    }
    let mut legacy = wire;
    legacy["version"] = json!(8);
    legacy.as_object_mut().unwrap().remove("workspace_groups");
    let migrated = WorkspaceStore::prepare_session_restore(
        validate(legacy).unwrap(),
        &[
            SessionId(200),
            SessionId(201),
            SessionId(202),
            SessionId(203),
        ],
        None,
    )
    .unwrap();
    assert!(migrated.store.snapshot().workspace_groups.is_empty());
    assert_eq!(migrated.store.session_document().unwrap().version, 9);
}

#[test]
fn failed_provisional_restore_prunes_group_membership_and_promotes_the_surviving_anchor() {
    use zmux_workspace_core::Effect;
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::from_u128(78));
    for name in ["A", "B"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let original = store.snapshot();
    let a = original.workspaces[0].id;
    let b = original.workspaces[1].id;
    store
        .apply(Action::CloseWorkspace { workspace_id: a })
        .unwrap();
    let reopening = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    let session = reopening
        .effects
        .iter()
        .find_map(|e| {
            if let Effect::StartSession { session_id, .. } = e {
                Some(*session_id)
            } else {
                None
            }
        })
        .unwrap();
    let restored = store
        .snapshot()
        .workspaces
        .iter()
        .find(|w| w.name == "A")
        .unwrap()
        .id;
    group(&mut store,json!({"operation":"create","window_id":original.windows[0].id,"name":"Provisional","workspace_ids":[restored,b]})).unwrap();
    store.finish_restore(session, false);
    assert_eq!(store.snapshot().workspace_groups[0].workspace_ids, vec![b]);
    assert!(
        store.session_document().is_ok(),
        "rollback must leave exportable canonical group membership"
    );
}
