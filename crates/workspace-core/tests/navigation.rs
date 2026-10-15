use zmux_workspace_core::{targets::*, *};

fn create(store: &mut WorkspaceStore, name: &str) -> Workspace {
    store
        .apply(Action::CreateWorkspace { name: name.into() })
        .unwrap();
    store.snapshot().workspaces.last().unwrap().clone()
}
fn selected(store: &WorkspaceStore) -> SurfaceId {
    let model = store.snapshot();
    let workspace = model
        .workspaces
        .iter()
        .find(|w| Some(w.id) == model.focused_workspace)
        .unwrap();
    workspace
        .root
        .panes()
        .into_iter()
        .find(|p| p.id == workspace.focused_pane)
        .unwrap()
        .selected_surface
}
fn navigation(store: &mut WorkspaceStore, action: Action) {
    assert!(
        store.apply(action).unwrap().effects.is_empty(),
        "selection must not restart sessions"
    );
}
fn split(store: &mut WorkspaceStore, pane: PaneId, direction: Direction, focus: bool) -> PaneId {
    match store
        .apply(Action::SplitPaneDirectional {
            pane_id: pane,
            direction,
            focus,
        })
        .unwrap()
        .effects
        .as_slice()
    {
        [Effect::StartSession { pane_id, .. }] => *pane_id,
        effects => panic!("split must start exactly one new session, got {effects:?}"),
    }
}

#[test]
fn directional_split_orders_new_panes_and_preserves_background_focus_and_sessions() {
    for (direction, axis, before) in [
        (Direction::Left, Axis::Horizontal, true),
        (Direction::Right, Axis::Horizontal, false),
        (Direction::Up, Axis::Vertical, true),
        (Direction::Down, Axis::Vertical, false),
    ] {
        let mut store = WorkspaceStore::default();
        let source = create(&mut store, "source");
        let original = source.root.panes()[0].clone();
        let background = create(&mut store, "active");
        let prior_selected = selected(&store);
        let added = split(&mut store, original.id, direction, false);
        let model = store.snapshot();
        assert_eq!(model.focused_workspace, Some(background.id));
        assert_eq!(model.workspaces[0].focused_pane, original.id);
        assert_eq!(selected(&store), prior_selected);
        match &model.workspaces[0].root {
            PaneTree::Split {
                axis: actual,
                first,
                second,
            } => {
                assert_eq!(*actual, axis);
                let (new, old) = if before {
                    (first, second)
                } else {
                    (second, first)
                };
                assert_eq!(new.panes()[0].id, added);
                assert_eq!(old.panes()[0], &original);
            }
            _ => panic!("directional split must retain both panes"),
        }
        navigation(
            &mut store,
            Action::FocusPane {
                pane_id: original.id,
            },
        );
        let focused_added = split(&mut store, original.id, direction, true);
        assert_eq!(store.snapshot().focused_workspace, Some(source.id));
        assert_eq!(store.snapshot().workspaces[0].focused_pane, focused_added);
    }
}

#[test]
fn directional_focus_uses_adjacent_nested_panes_and_boundary_failure_is_atomic() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "grid").focused_pane;
    let right = split(&mut store, first, Direction::Right, true);
    let lower_left = split(&mut store, first, Direction::Down, true);
    let lower_right = split(&mut store, right, Direction::Down, true);
    for (source, direction, expected) in [
        (first, Direction::Right, right),
        (first, Direction::Down, lower_left),
        (lower_left, Direction::Right, lower_right),
        (lower_right, Direction::Up, right),
        (right, Direction::Left, first),
        (lower_right, Direction::Left, lower_left),
    ] {
        navigation(
            &mut store,
            Action::FocusPaneDirection {
                pane_id: source,
                direction,
            },
        );
        assert_eq!(store.snapshot().workspaces[0].focused_pane, expected);
    }
    for (source, direction) in [
        (first, Direction::Up),
        (first, Direction::Left),
        (lower_right, Direction::Down),
        (lower_right, Direction::Right),
    ] {
        let before = store.snapshot();
        assert!(
            store
                .apply(Action::FocusPaneDirection {
                    pane_id: source,
                    direction
                })
                .is_err()
        );
        assert_eq!(store.snapshot(), before);
    }
    navigation(
        &mut store,
        Action::NavigatePane {
            pane_id: lower_right,
            direction: Navigation::Next,
        },
    );
    assert_eq!(store.snapshot().workspaces[0].focused_pane, first);
    navigation(
        &mut store,
        Action::NavigatePane {
            pane_id: first,
            direction: Navigation::Last,
        },
    );
    assert_eq!(store.snapshot().workspaces[0].focused_pane, lower_left);
}

#[test]
fn workspace_and_tab_navigation_wrap_and_last_remains_within_its_owner() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let pane = first.focused_pane;
    let first_tab = first.root.panes()[0].selected_surface;
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    let second_tab = selected(&store);
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    let third_tab = selected(&store);
    navigation(
        &mut store,
        Action::NavigateSurface {
            surface_id: third_tab,
            direction: Navigation::Next,
        },
    );
    assert_eq!(selected(&store), first_tab);
    navigation(
        &mut store,
        Action::NavigateSurface {
            surface_id: first_tab,
            direction: Navigation::Previous,
        },
    );
    assert_eq!(selected(&store), third_tab);
    navigation(
        &mut store,
        Action::NavigateSurface {
            surface_id: third_tab,
            direction: Navigation::Last,
        },
    );
    assert_eq!(selected(&store), first_tab);
    navigation(
        &mut store,
        Action::SelectSurface {
            surface_id: second_tab,
        },
    );
    let second = create(&mut store, "second");
    let third = create(&mut store, "third");
    navigation(
        &mut store,
        Action::NavigateWorkspace {
            workspace_id: third.id,
            direction: Navigation::Next,
        },
    );
    assert_eq!(store.snapshot().focused_workspace, Some(first.id));
    assert_eq!(
        selected(&store),
        second_tab,
        "workspace selection keeps its last selected tab"
    );
    navigation(
        &mut store,
        Action::NavigateWorkspace {
            workspace_id: first.id,
            direction: Navigation::Previous,
        },
    );
    assert_eq!(store.snapshot().focused_workspace, Some(third.id));
    navigation(
        &mut store,
        Action::NavigateWorkspace {
            workspace_id: third.id,
            direction: Navigation::Last,
        },
    );
    assert_eq!(store.snapshot().focused_workspace, Some(first.id));
    navigation(
        &mut store,
        Action::NavigateSurface {
            surface_id: second_tab,
            direction: Navigation::Last,
        },
    );
    assert_eq!(
        selected(&store),
        first_tab,
        "last tab cannot select another workspace's tab"
    );
    navigation(
        &mut store,
        Action::SelectWorkspace {
            workspace_id: second.id,
        },
    );
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::NavigateSurface {
                surface_id: selected(&store),
                direction: Navigation::Last
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
}

#[test]
fn focus_history_restores_tabs_across_workspaces_and_only_focus_changes_clear_forward() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let original = selected(&store);
    store
        .apply(Action::CreateSurface {
            pane_id: first.focused_pane,
        })
        .unwrap();
    let tab = selected(&store);
    let second = create(&mut store, "second");
    let other = selected(&store);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Back,
        },
    );
    assert_eq!(selected(&store), tab);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Back,
        },
    );
    assert_eq!(selected(&store), original);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Forward,
        },
    );
    assert_eq!(selected(&store), tab);
    store
        .apply(Action::RenameWorkspace {
            workspace_id: first.id,
            name: "renamed".into(),
        })
        .unwrap();
    navigation(&mut store, Action::SelectSurface { surface_id: tab });
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::FocusPane {
                pane_id: PaneId(u64::MAX)
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Forward,
        },
    );
    assert_eq!(selected(&store), other);
    assert_eq!(store.snapshot().focused_workspace, Some(second.id));
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Back,
        },
    );
    navigation(
        &mut store,
        Action::SelectSurface {
            surface_id: original,
        },
    );
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::NavigateFocus {
                direction: FocusHistoryDirection::Forward
            })
            .is_err()
    );
    assert_eq!(
        store.snapshot(),
        before,
        "a new focus branch clears forward history atomically"
    );
}

#[test]
fn history_skips_closed_targets_and_tracks_moved_live_surfaces() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let original = selected(&store);
    store
        .apply(Action::CreateSurface {
            pane_id: first.focused_pane,
        })
        .unwrap();
    let moved = selected(&store);
    let second = create(&mut store, "second");
    let other = selected(&store);
    store
        .apply(Action::MoveSurface {
            surface_id: moved,
            pane_id: second.focused_pane,
            index: 1,
        })
        .unwrap();
    store
        .apply(Action::CloseSurface { surface_id: other })
        .unwrap();
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Back,
        },
    );
    assert_eq!(selected(&store), original);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Forward,
        },
    );
    assert_eq!(selected(&store), moved);
    assert_eq!(store.snapshot().focused_workspace, Some(second.id));
    store
        .apply(Action::CloseWorkspace {
            workspace_id: first.id,
        })
        .unwrap();
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::NavigateFocus {
                direction: FocusHistoryDirection::Back
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
}

#[test]
fn failed_restore_does_not_leave_provisional_focus_targets_in_history() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let original = selected(&store);
    let second = create(&mut store, "second");
    store
        .apply(Action::CloseWorkspace {
            workspace_id: second.id,
        })
        .unwrap();
    let restore = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    let session = match restore.effects.as_slice() {
        [Effect::StartSession { session_id, .. }] => *session_id,
        _ => panic!("one session restored"),
    };
    let provisional = selected(&store);
    assert_ne!(provisional, original);
    store.finish_restore(session, false);
    assert_eq!(store.snapshot().focused_workspace, Some(first.id));
    assert_eq!(selected(&store), original);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::NavigateFocus {
                direction: FocusHistoryDirection::Back
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    assert!(!store.snapshot().closed_items.last().unwrap().restoring);
}

#[test]
fn focus_history_has_a_bounded_reachable_past() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let a = selected(&store);
    store
        .apply(Action::CreateSurface {
            pane_id: first.focused_pane,
        })
        .unwrap();
    let b = selected(&store);
    for index in 0..MAX_FOCUS_HISTORY + 20 {
        navigation(
            &mut store,
            Action::SelectSurface {
                surface_id: if index % 2 == 0 { a } else { b },
            },
        );
    }
    for _ in 0..MAX_FOCUS_HISTORY {
        navigation(
            &mut store,
            Action::NavigateFocus {
                direction: FocusHistoryDirection::Back,
            },
        );
    }
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::NavigateFocus {
                direction: FocusHistoryDirection::Back
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    navigation(
        &mut store,
        Action::NavigateFocus {
            direction: FocusHistoryDirection::Forward,
        },
    );
}

#[test]
fn targeted_navigation_uses_caller_or_explicit_scope_and_rejects_foreign_owners() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let second = create(&mut store, "second");
    let caller = CallerContext {
        window: None,
        local_tmux: None,
        workspace: Some(format!("workspace:{}", first.id.0).parse().unwrap()),
        surface: None,
    };
    let action = TargetAction::SplitPaneDirectional {
        direction: Direction::Left,
        focus: false,
    }
    .resolve(&store.snapshot(), &TargetScope::default(), &caller)
    .unwrap();
    let added = match store.apply(action).unwrap().effects.as_slice() {
        [Effect::StartSession { pane_id, .. }] => *pane_id,
        _ => panic!("one new pane"),
    };
    assert_eq!(store.snapshot().focused_workspace, Some(second.id));
    let scope = TargetScope {
        window: None,
        workspace: Some(format!("workspace:{}", first.id.0).parse().unwrap()),
        pane: Some("index:1".parse().unwrap()),
        surface: None,
    };
    let action = TargetAction::FocusPaneDirection {
        direction: Direction::Right,
    }
    .resolve(&store.snapshot(), &scope, &CallerContext::default())
    .unwrap();
    navigation(&mut store, action);
    assert_eq!(
        store.snapshot().workspaces[0].focused_pane,
        first.focused_pane
    );
    let last = TargetAction::NavigatePane {
        direction: Navigation::Previous,
    }
    .resolve(&store.snapshot(), &scope, &CallerContext::default())
    .unwrap();
    navigation(&mut store, last);
    assert_eq!(
        store.snapshot().workspaces[0].focused_pane,
        first.focused_pane
    );
    let foreign = TargetScope {
        window: None,
        workspace: scope.workspace.clone(),
        pane: Some(format!("pane:{}", second.focused_pane.0).parse().unwrap()),
        surface: None,
    };
    let before = store.snapshot();
    assert!(matches!(
        TargetAction::FocusPaneDirection {
            direction: Direction::Left
        }
        .resolve(&before, &foreign, &caller),
        Err(TargetError::ScopeMismatch {
            kind: TargetKind::Pane
        })
    ));
    assert_eq!(store.snapshot(), before);
    let action = TargetAction::SelectWorkspace
        .resolve(
            &before,
            &TargetScope {
                workspace: Some(format!("workspace:{}", second.id.0).parse().unwrap()),
                ..Default::default()
            },
            &caller,
        )
        .unwrap();
    navigation(&mut store, action);
    assert_eq!(store.snapshot().focused_workspace, Some(second.id));
    assert_ne!(added, first.focused_pane);
}

#[test]
fn moving_a_surface_does_not_rewrite_previously_focused_workspace_history() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store, "first");
    let original = selected(&store);
    store
        .apply(Action::CreateSurface {
            pane_id: first.focused_pane,
        })
        .unwrap();
    let second = create(&mut store, "second");
    create(&mut store, "third");
    navigation(
        &mut store,
        Action::SelectSurface {
            surface_id: original,
        },
    );
    store
        .apply(Action::MoveSurface {
            surface_id: original,
            pane_id: second.focused_pane,
            index: 1,
        })
        .unwrap();
    assert_eq!(store.snapshot().focused_workspace, Some(second.id));
    navigation(
        &mut store,
        Action::NavigateWorkspace {
            workspace_id: second.id,
            direction: Navigation::Last,
        },
    );
    assert_eq!(
        store.snapshot().focused_workspace,
        Some(first.id),
        "workspace history records which workspace was focused, independent of later tab ownership"
    );
}
