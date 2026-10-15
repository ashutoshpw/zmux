use uuid::Uuid;
use zmux_workspace_core::{targets::*, *};

fn create(store: &mut WorkspaceStore, name: &str) -> Workspace {
    store
        .apply(Action::CreateWorkspace { name: name.into() })
        .unwrap();
    store.snapshot().workspaces.last().unwrap().clone()
}

#[test]
fn target_identity_survives_reorder_and_move_but_closed_surface_does_not_retarget() {
    let mut store = WorkspaceStore::with_identity_namespace(Uuid::from_u128(1));
    let first = create(&mut store, "first");
    let pane = first.focused_pane;
    let surface = first.root.panes()[0].selected_surface;
    let uuid = store
        .snapshot()
        .object_uuid(TargetKind::Surface, surface.0)
        .to_string();
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    store
        .apply(Action::ReorderSurface {
            surface_id: surface,
            index: 1,
        })
        .unwrap();
    let second = create(&mut store, "second");
    store
        .apply(Action::MoveSurface {
            surface_id: surface,
            pane_id: second.focused_pane,
            index: 0,
        })
        .unwrap();
    let model = store.snapshot();
    let scope = TargetScope {
        surface: Some(uuid.parse().unwrap()),
        ..Default::default()
    };
    let resolved = scope.resolve(&model, &CallerContext::default()).unwrap();
    assert_eq!(
        (resolved.workspace, resolved.pane, resolved.surface),
        (second.id, second.focused_pane, surface)
    );
    let inherited = CallerContext {
        window: None,
        local_tmux: None,
        workspace: Some(
            model
                .object_uuid(TargetKind::Workspace, first.id.0)
                .to_string()
                .parse()
                .unwrap(),
        ),
        surface: Some(uuid.parse().unwrap()),
    };
    assert_eq!(
        TargetScope::default()
            .resolve(&model, &inherited)
            .unwrap()
            .workspace,
        second.id,
        "the originating surface follows a move even though its inherited workspace is older"
    );
    store
        .apply(Action::CloseSurface {
            surface_id: surface,
        })
        .unwrap();
    assert!(matches!(
        scope.resolve(&store.snapshot(), &CallerContext::default()),
        Err(TargetError::NotFound { .. })
    ));
    store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(
        matches!(
            scope.resolve(&store.snapshot(), &CallerContext::default()),
            Err(TargetError::NotFound { .. })
        ),
        "reopening allocates a fresh identity and cannot resurrect a stale target"
    );
}

#[test]
fn scoped_targets_and_position_indices_reject_wrong_kind_and_cross_owner_without_mutation() {
    let mut store = WorkspaceStore::with_identity_namespace(Uuid::from_u128(2));
    let first = create(&mut store, "first");
    let second = create(&mut store, "second");
    let model = store.snapshot();
    let workspace: TargetRef = format!("workspace:{}", first.id.0).parse().unwrap();
    let pane: TargetRef = format!("pane:{}", first.focused_pane.0).parse().unwrap();
    let caller = CallerContext {
        window: None,
        local_tmux: None,
        workspace: Some(format!("workspace:{}", second.id.0).parse().unwrap()),
        surface: None,
    };
    let scope = TargetScope {
        window: None,
        workspace: Some(workspace.clone()),
        pane: Some("index:1".parse().unwrap()),
        surface: Some("index:1".parse().unwrap()),
    };
    let selected = scope.resolve(&model, &caller).unwrap();
    assert_eq!(selected.workspace, first.id);
    assert_eq!(selected.pane, first.focused_pane);
    let source = TargetScope {
        surface: Some(
            format!("surface:{}", first.root.panes()[0].selected_surface.0)
                .parse()
                .unwrap(),
        ),
        ..Default::default()
    };
    let action = TargetAction::MoveSurface {
        destination: "index:1".parse().unwrap(),
        index: 0,
    }
    .resolve(&model, &source, &CallerContext::default())
    .unwrap();
    assert!(
        matches!(action, Action::MoveSurface { pane_id, .. } if pane_id == first.focused_pane),
        "destination indices must not target the unrelated focused workspace"
    );

    for handle in [
        pane,
        model
            .object_uuid(TargetKind::Pane, first.focused_pane.0)
            .to_string()
            .parse()
            .unwrap(),
    ] {
        let wrong = TargetScope {
            workspace: Some(handle),
            ..Default::default()
        };
        assert!(matches!(
            wrong.resolve(&model, &caller),
            Err(TargetError::WrongKind {
                expected: TargetKind::Workspace,
                actual: TargetKind::Pane
            })
        ));
    }
    let foreign = TargetScope {
        window: None,
        workspace: Some(workspace),
        pane: Some(format!("pane:{}", second.focused_pane.0).parse().unwrap()),
        surface: None,
    };
    assert!(matches!(
        foreign.resolve(&model, &caller),
        Err(TargetError::ScopeMismatch { .. })
    ));
    assert_eq!(
        store.snapshot(),
        model,
        "resolution failures do not modify topology or focus"
    );
    for invalid in [
        "",
        "0",
        "pane:0",
        "index:0",
        "+1",
        "-1",
        "window:abc",
        "pane:abc",
        "1:2:3",
    ] {
        assert!(
            invalid.parse::<TargetRef>().is_err(),
            "accepted malformed handle {invalid}"
        );
    }
}
