use serde_json::json;
use zmux_workspace_core::{Action, WorkspaceStore};

#[test]
fn background_surface_reorder_defaults_to_preserving_every_selection_and_live_identity() {
    let mut store = WorkspaceStore::default();
    store
        .apply(Action::CreateWorkspace {
            name: "background".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    for _ in 0..2 {
        store
            .apply(Action::CreateSurface { pane_id: pane })
            .unwrap();
    }
    let background = store.snapshot().workspaces[0].clone();
    let ids: Vec<_> = background.root.panes()[0]
        .surfaces
        .iter()
        .map(|s| s.id)
        .collect();
    store
        .apply(Action::SelectSurface { surface_id: ids[0] })
        .unwrap();
    store
        .apply(Action::CreateWorkspace {
            name: "foreground".into(),
        })
        .unwrap();
    let before = store.snapshot();
    let next = store.next_identifier();
    // Omitted focus must mean no focus on the shared action transport too.
    let action: Action =
        serde_json::from_value(json!({"action":"reorder_surface","surface_id":ids[2],"index":0}))
            .unwrap();
    assert!(store.apply(action).unwrap().effects.is_empty());
    let after = store.snapshot();
    assert_eq!(after.focused_workspace, before.focused_workspace);
    assert_eq!(after.workspaces[0].root.panes()[0].selected_surface, ids[0]);
    assert_eq!(
        after.workspaces[0].root.panes()[0]
            .surfaces
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![ids[2], ids[0], ids[1]]
    );
    assert_eq!(store.next_identifier(), next);
    let mut expected = before.clone();
    let zmux_workspace_core::PaneTree::Leaf {
        pane: expected_pane,
    } = &mut expected.workspaces[0].root
    else {
        panic!("fixture must contain one pane")
    };
    let tabs = &mut expected_pane.surfaces;
    let moved = tabs.remove(2);
    tabs.insert(0, moved);
    expected.revision = after.revision;
    assert_eq!(after, expected);
}

#[test]
fn final_positions_relative_anchors_and_native_offsets_have_one_same_pane_contract() {
    use zmux_workspace_core::surface_order::Placement;
    let mut store = WorkspaceStore::default();
    store
        .apply(Action::CreateWorkspace {
            name: "tabs".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    for _ in 0..3 {
        store
            .apply(Action::CreateSurface { pane_id: pane })
            .unwrap();
    }
    let model = store.snapshot();
    let ids: Vec<_> = model.workspaces[0].root.panes()[0]
        .surfaces
        .iter()
        .map(|s| s.id)
        .collect();
    let cases = [
        (0, Placement::Index { index: 1 }, 1),
        (2, Placement::Index { index: i64::MIN }, 0),
        (0, Placement::Index { index: i64::MAX }, 3),
        (0, Placement::Before { surface_id: ids[2] }, 1),
        (0, Placement::After { surface_id: ids[2] }, 2),
        (3, Placement::Before { surface_id: ids[1] }, 1),
        (3, Placement::After { surface_id: ids[1] }, 2),
        (1, Placement::Before { surface_id: ids[1] }, 1),
        (1, Placement::After { surface_id: ids[1] }, 1),
        (0, Placement::Offset { offset: -1 }, 0),
        (3, Placement::Offset { offset: 1 }, 3),
        (1, Placement::Offset { offset: i64::MAX }, 3),
        (2, Placement::Offset { offset: i64::MIN }, 0),
    ];
    for (source, placement, expected) in cases {
        let plan = model.plan_surface_order(ids[source], &placement).unwrap();
        assert_eq!(
            (plan.pane_id, plan.from_index, plan.to_index),
            (pane, source, expected),
            "{placement:?}"
        );
    }
    store
        .apply(Action::CreateWorkspace {
            name: "other".into(),
        })
        .unwrap();
    let foreign = store.snapshot().workspaces[1].root.panes()[0].selected_surface;
    for anchor in [foreign, zmux_workspace_core::SurfaceId(u64::MAX)] {
        assert!(
            store
                .snapshot()
                .plan_surface_order(ids[0], &Placement::Before { surface_id: anchor })
                .is_err()
        );
    }
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::ReorderSurface {
                surface_id: ids[0],
                index: 4,
                focus: true
            })
            .is_err()
    );
    assert_eq!(
        store.snapshot(),
        before,
        "strict raw-action bounds reject atomically"
    );
    assert!(
        store
            .apply(Action::ReorderSurface {
                surface_id: ids[0],
                index: 1,
                focus: true
            })
            .unwrap()
            .effects
            .is_empty()
    );
    let after = store.snapshot();
    assert_eq!(after.focused_workspace, Some(before.workspaces[0].id));
    assert_eq!(after.workspaces[0].root.panes()[0].selected_surface, ids[0]);
    assert_eq!(
        after.workspaces[0].root.panes()[0]
            .surfaces
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![ids[1], ids[0], ids[2], ids[3]]
    );
}
