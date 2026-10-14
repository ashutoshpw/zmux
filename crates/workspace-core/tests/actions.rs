use zmux_workspace_core::*;

fn create(store: &mut WorkspaceStore, name: &str) -> (WorkspaceId, PaneId, SessionId) {
    store
        .apply(Action::CreateWorkspace { name: name.into() })
        .unwrap();
    let snapshot = store.snapshot();
    let workspace = snapshot.workspaces.last().unwrap();
    let pane = workspace.root.panes()[0];
    (workspace.id, pane.id, pane.session_id().unwrap())
}

#[test]
fn split_and_close_preserve_surviving_identity_and_stop_only_closed_session() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, first_id, first_session) = create(&mut store, "work");
    let result = store
        .apply(Action::SplitPane {
            pane_id: first_id,
            axis: Axis::Vertical,
        })
        .unwrap();
    let (second_id, second_session) = match result.effects.as_slice() {
        [
            Effect::StartSession {
                pane_id,
                session_id,
                ..
            },
        ] => (*pane_id, *session_id),
        _ => panic!("split must request exactly one session"),
    };
    assert_ne!(first_id, second_id);
    assert_ne!(first_session, second_session);
    let split = store.snapshot();
    assert_eq!(split.workspaces[0].focused_pane, second_id);
    assert_eq!(split.workspaces[0].root.panes().len(), 2);
    let closed = store
        .apply(Action::ClosePane { pane_id: second_id })
        .unwrap();
    assert_eq!(
        closed.effects,
        vec![Effect::StopSession {
            session_id: second_session
        }]
    );
    let snapshot = store.snapshot();
    assert_eq!(snapshot.focused_workspace, Some(workspace_id));
    assert_eq!(snapshot.workspaces[0].focused_pane, first_id);
    assert!(
        matches!(&snapshot.workspaces[0].root, PaneTree::Leaf { pane } if pane.id == first_id && pane.session_id().unwrap() == first_session)
    );
}

#[test]
fn focus_moves_between_workspaces_and_final_close_leaves_valid_empty_state() {
    let mut store = WorkspaceStore::default();
    let (first_workspace, first, _) = create(&mut store, "first");
    let (second_workspace, second, _) = create(&mut store, "second");
    store.apply(Action::FocusPane { pane_id: first }).unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(first_workspace));
    store.apply(Action::ClosePane { pane_id: first }).unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(second_workspace));
    store.apply(Action::ClosePane { pane_id: second }).unwrap();
    assert!(store.snapshot().workspaces.is_empty());
    assert_eq!(store.snapshot().focused_workspace, None);
    let (_, replacement, _) = create(&mut store, "replacement");
    assert_ne!(replacement, first);
    assert_ne!(replacement, second);
}

#[test]
fn rejected_actions_leave_state_and_revision_unchanged() {
    let mut store = WorkspaceStore::default();
    let (workspace, pane, _) = create(&mut store, "original");
    let before = store.snapshot();
    for action in [
        Action::CreateWorkspace { name: " ".into() },
        Action::RenameWorkspace {
            workspace_id: workspace,
            name: "bad\nname".into(),
        },
        Action::RenameWorkspace {
            workspace_id: WorkspaceId(999),
            name: "valid".into(),
        },
        Action::FocusPane {
            pane_id: PaneId(999),
        },
        Action::SplitPane {
            pane_id: PaneId(999),
            axis: Axis::Horizontal,
        },
        Action::ClosePane {
            pane_id: PaneId(999),
        },
    ] {
        assert!(store.apply(action).is_err());
        assert_eq!(store.snapshot(), before);
    }
    store
        .apply(Action::RenameWorkspace {
            workspace_id: workspace,
            name: "  renamed  ".into(),
        })
        .unwrap();
    assert_eq!(store.snapshot().workspaces[0].name, "renamed");
    assert_eq!(store.snapshot().workspaces[0].focused_pane, pane);
}

#[test]
fn nested_close_collapses_tree_without_disturbing_another_workspace() {
    let mut store = WorkspaceStore::default();
    let (first_workspace, first, _) = create(&mut store, "first");
    store
        .apply(Action::SplitPane {
            pane_id: first,
            axis: Axis::Horizontal,
        })
        .unwrap();
    let sibling = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::SplitPane {
            pane_id: sibling,
            axis: Axis::Vertical,
        })
        .unwrap();
    let nested = store.snapshot().workspaces[0].focused_pane;
    let (other_workspace, other, _) = create(&mut store, "other");
    store.apply(Action::ClosePane { pane_id: sibling }).unwrap();
    let snapshot = store.snapshot();
    assert_eq!(snapshot.focused_workspace, Some(other_workspace));
    assert_eq!(snapshot.workspaces[1].focused_pane, other);
    let remaining: Vec<_> = snapshot.workspaces[0]
        .root
        .panes()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(remaining, [first, nested]);
    store.apply(Action::FocusPane { pane_id: nested }).unwrap();
    store.apply(Action::ClosePane { pane_id: nested }).unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(first_workspace));
    assert_eq!(store.snapshot().workspaces[0].focused_pane, first);
}

#[test]
fn capacity_and_depth_rejection_are_atomic() {
    let mut store = WorkspaceStore::default();
    let (_, mut pane, _) = create(&mut store, "depth");
    for _ in 0..MAX_SPLIT_DEPTH {
        store
            .apply(Action::SplitPane {
                pane_id: pane,
                axis: Axis::Horizontal,
            })
            .unwrap();
        pane = store.snapshot().workspaces[0].focused_pane;
    }
    let before = store.snapshot();
    assert!(matches!(
        store.apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Vertical
        }),
        Err(ActionError::Capacity {
            resource: "split depth",
            ..
        })
    ));
    assert_eq!(store.snapshot(), before);
    for i in 1..MAX_WORKSPACES {
        create(&mut store, &format!("workspace {i}"));
    }
    let before = store.snapshot();
    assert!(matches!(
        store.apply(Action::CreateWorkspace {
            name: "overflow".into()
        }),
        Err(ActionError::Capacity {
            resource: "workspaces",
            ..
        })
    ));
    assert_eq!(store.snapshot(), before);
}

#[test]
fn pane_capacity_is_independent_of_depth_limit() {
    let mut store = WorkspaceStore::default();
    create(&mut store, "balanced");
    while store.snapshot().workspaces[0].root.panes().len() < MAX_PANES_PER_WORKSPACE {
        // Split leaves breadth-first so no branch approaches the depth limit.
        let snapshot = store.snapshot();
        let ids: Vec<_> = snapshot.workspaces[0]
            .root
            .panes()
            .into_iter()
            .map(|p| p.id)
            .collect();
        for id in ids {
            if store.snapshot().workspaces[0].root.panes().len() == MAX_PANES_PER_WORKSPACE {
                break;
            }
            store
                .apply(Action::SplitPane {
                    pane_id: id,
                    axis: Axis::Horizontal,
                })
                .unwrap();
        }
    }
    let before = store.snapshot();
    let pane = before.workspaces[0].focused_pane;
    assert!(matches!(
        store.apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Horizontal
        }),
        Err(ActionError::Capacity {
            resource: "panes",
            ..
        })
    ));
    assert_eq!(store.snapshot(), before);
}
