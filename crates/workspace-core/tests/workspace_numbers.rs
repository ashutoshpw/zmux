use zmux_workspace_core::{Action, WorkspaceStore, workspace_order::WorkspaceOrder};

#[test]
fn workspace_digits_follow_canonical_window_order_and_nine_selects_last() {
    for count in [0, 1, 2, 8, 9, 12] {
        let mut store = WorkspaceStore::default();
        for index in 0..count {
            store
                .apply(Action::CreateWorkspace {
                    name: format!("workspace {index}"),
                })
                .unwrap();
        }
        if count == 0 {
            store
                .apply(Action::CreateWorkspace {
                    name: "empty owner".into(),
                })
                .unwrap();
            let id = store.snapshot().workspaces[0].id;
            store
                .apply(Action::CloseWorkspace { workspace_id: id })
                .unwrap();
        }
        let window = store.snapshot().windows[0].id;
        let mut ids = store.snapshot().windows[0].workspace_ids.clone();
        ids.reverse();
        if !ids.is_empty() {
            store
                .apply(Action::ReorderWorkspaces {
                    window_id: window,
                    order: WorkspaceOrder::Bulk {
                        workspace_ids: ids.clone(),
                    },
                })
                .unwrap();
        }
        // A second window's first row must never leak into a missing index.
        store.apply(Action::CreateWindow { focus: false }).unwrap();
        let model = store.snapshot();
        for digit in 1..=9 {
            let expected = if digit == 9 {
                ids.last().copied()
            } else {
                ids.get(usize::from(digit - 1)).copied()
            };
            assert_eq!(
                model.workspace_number(window, digit).unwrap(),
                expected,
                "count={count}, digit={digit}"
            );
        }
        assert!(model.workspace_number(window, 0).is_err());
        assert!(model.workspace_number(window, 10).is_err());
        assert!(
            model
                .workspace_number(zmux_workspace_core::WindowId(999999), 1)
                .is_err()
        );
    }
}
