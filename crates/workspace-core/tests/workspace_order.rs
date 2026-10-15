use serde_json::{Value, json};
use zmux_workspace_core::{Action, WorkspaceStore};

fn fixture() -> WorkspaceStore {
    let mut store = WorkspaceStore::default();
    for name in ["A", "B", "C", "D"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    store
}
fn reorder(
    store: &mut WorkspaceStore,
    order: Value,
) -> Result<zmux_workspace_core::Transition, String> {
    let action: Action = serde_json::from_value(json!({"action":"reorder_workspaces", "window_id":store.snapshot().windows[0].id, "order":order})).map_err(|e| e.to_string())?;
    store.apply(action).map_err(|e| e.to_string())
}
#[test]
fn single_placement_uses_final_zero_based_indices_without_changing_focus_or_objects() {
    // Independent behavioral examples; after-self intentionally follows the frozen reference.
    for (source, placement, expected) in [
        (
            1,
            json!({"placement":"index", "index":-50}),
            vec![1, 0, 2, 3],
        ),
        (
            1,
            json!({"placement":"index", "index":50}),
            vec![0, 2, 3, 1],
        ),
        (
            0,
            json!({"placement":"before", "workspace_id":"C"}),
            vec![1, 0, 2, 3],
        ),
        (
            0,
            json!({"placement":"after", "workspace_id":"C"}),
            vec![1, 2, 0, 3],
        ),
        (
            2,
            json!({"placement":"before", "workspace_id":"A"}),
            vec![2, 0, 1, 3],
        ),
        (
            1,
            json!({"placement":"before", "workspace_id":"B"}),
            vec![0, 1, 2, 3],
        ),
        (
            1,
            json!({"placement":"after", "workspace_id":"B"}),
            vec![0, 2, 1, 3],
        ),
        (
            3,
            json!({"placement":"after", "workspace_id":"D"}),
            vec![0, 1, 2, 3],
        ),
    ] {
        let mut store = fixture();
        let before = store.snapshot();
        let ids = before.windows[0].workspace_ids.clone();
        let next = store.next_identifier();
        let mut placement = placement;
        if let Some(name) = placement["workspace_id"].as_str() {
            let position = ["A", "B", "C", "D"]
                .iter()
                .position(|n| *n == name)
                .unwrap();
            placement["workspace_id"] = json!(ids[position]);
        }
        let transition = reorder(
            &mut store,
            json!({"operation":"single", "workspace_id":ids[source], "placement":placement}),
        )
        .expect("shared reorder action");
        assert!(transition.effects.is_empty());
        let mut after = store.snapshot();
        assert_eq!(
            after.windows[0].workspace_ids,
            expected.iter().map(|i| ids[*i]).collect::<Vec<_>>()
        );
        assert_eq!(store.next_identifier(), next);
        after.windows[0].workspace_ids = ids;
        after.revision = before.revision;
        assert_eq!(
            after, before,
            "only canonical order and revision may change"
        );
    }
}
#[test]
fn bulk_prefix_preserves_unlisted_order_and_rejects_invalid_input_atomically() {
    let mut store = fixture();
    let before = store.snapshot();
    let ids = before.windows[0].workspace_ids.clone();
    reorder(
        &mut store,
        json!({"operation":"bulk", "workspace_ids":[ids[2],ids[0]]}),
    )
    .unwrap();
    assert_eq!(
        store.snapshot().windows[0].workspace_ids,
        vec![ids[2], ids[0], ids[1], ids[3]]
    );
    store.apply(Action::CreateWindow { focus: false }).unwrap();
    let foreign = store.snapshot().windows[1].workspace_ids[0];
    for requested in [
        json!([]),
        json!([ids[0], ids[0]]),
        json!([ids[1], 999999]),
        json!([foreign]),
    ] {
        let before = store.snapshot();
        let next = store.next_identifier();
        assert!(
            reorder(
                &mut store,
                json!({"operation":"bulk", "workspace_ids":requested})
            )
            .is_err()
        );
        assert_eq!(store.snapshot(), before);
        assert_eq!(store.next_identifier(), next);
    }
}
