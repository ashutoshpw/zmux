use zmux_remote_runtime::SshConnectionConfig;
use zmux_workspace_core::*;

fn connection() -> SshConnectionConfig {
    SshConnectionConfig {
        destination: "user@remote-alias".into(),
        port: Some(2222),
        identity: None,
        config_file: None,
        forward_agent: Some(false),
        ssh_options: vec![],
        jump_hosts: vec![],
        command: Some(vec!["run-once".into()]),
    }
}

#[test]
fn remote_tabs_splits_and_reopen_preserve_route_without_replaying_startup_commands() {
    let mut core = WorkspaceStore::default();
    core.apply(Action::CreateWorkspace {
        name: "Local".into(),
    })
    .unwrap();
    let local = core.snapshot().focused_workspace;
    let start = core
        .apply(Action::CreateSshWorkspace {
            name: "Remote".into(),
            connection: connection(),
            focus: false,
        })
        .unwrap();
    assert_eq!(core.snapshot().focused_workspace, local);
    assert!(
        matches!(&start.effects[0], Effect::StartSession { remote: Some(remote), .. } if remote.command.is_some())
    );
    let workspace = core.snapshot().workspaces[1].clone();
    let pane = workspace.root.panes()[0];
    let first = pane.selected().clone();
    core.set_session_cwd(first.session_id.unwrap(), "/path/only/on/remote".into());
    let namespace = core.snapshot().identity_namespace;
    let reconnect = core
        .apply(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id: first.id,
            expected_session_id: first.session_id.unwrap(),
        })
        .unwrap();
    assert_eq!(
        reconnect.effects[0],
        Effect::StopSession {
            session_id: first.session_id.unwrap()
        }
    );
    let replacement = core.snapshot();
    let replaced_workspace = replacement
        .workspaces
        .iter()
        .find(|candidate| candidate.id == workspace.id)
        .unwrap();
    let replaced_pane = replaced_workspace.root.panes()[0];
    let replaced_surface = replaced_pane.selected();
    let replacement_session = replaced_surface.session_id.unwrap();
    assert_ne!(replacement_session, first.session_id.unwrap());
    assert_eq!(replaced_surface.id, first.id);
    assert_eq!(replaced_surface.title, first.title);
    assert_eq!(replaced_surface.cwd, None);
    assert_eq!(
        replaced_surface.remote.as_ref().unwrap().destination,
        "user@remote-alias"
    );
    assert_eq!(replaced_surface.remote.as_ref().unwrap().port, Some(2222));
    assert_eq!(replaced_surface.remote.as_ref().unwrap().command, None);
    assert!(matches!(
        &reconnect.effects[1],
        Effect::StartSession {
            pane_id: started_pane,
            session_id,
            cwd: None,
            remote: Some(remote),
            local_tmux: None,
        } if *started_pane == pane.id && *session_id == replacement_session
            && remote.destination == "user@remote-alias" && remote.command.is_none()
    ));
    assert_eq!(replaced_pane.id, pane.id);
    assert_eq!(replaced_pane.selected_surface, first.id);
    let before_stale = core.snapshot();
    assert!(
        core.apply(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id: first.id,
            expected_session_id: first.session_id.unwrap(),
        })
        .is_err()
    );
    assert_eq!(core.snapshot(), before_stale);

    let local_surface = core.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert!(
        core.apply(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id: local_surface.id,
            expected_session_id: local_surface.session_id.unwrap(),
        })
        .is_err()
    );
    assert_eq!(core.snapshot(), before_stale);

    let tab = core
        .apply(Action::CreateSurface { pane_id: pane.id })
        .unwrap();
    let split = core
        .apply(Action::SplitPane {
            pane_id: pane.id,
            axis: Axis::Horizontal,
        })
        .unwrap();
    for transition in [tab, split] {
        assert!(
            matches!(&transition.effects[0], Effect::StartSession { remote: Some(remote), cwd: None, .. }
            if remote.destination == "user@remote-alias" && remote.port == Some(2222) && remote.command.is_none())
        );
    }
    core.apply(Action::CloseSurface {
        surface_id: first.id,
    })
    .unwrap();
    let reopened = core
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(
        matches!(&reopened.effects[0], Effect::StartSession { remote: Some(remote), .. }
        if remote.destination == "user@remote-alias" && remote.command.is_none())
    );
}

#[test]
fn raw_invalid_remote_actions_fail_before_state_or_id_allocation_changes() {
    let mut core = WorkspaceStore::default();
    let before = core.snapshot();
    let mut invalid = connection();
    invalid.destination = "-oProxyCommand=unexpected".into();
    assert!(
        core.apply(Action::CreateSshWorkspace {
            name: "Invalid".into(),
            connection: invalid,
            focus: true
        })
        .is_err()
    );
    assert_eq!(core.snapshot(), before);
    core.apply(Action::CreateWorkspace {
        name: "First valid".into(),
    })
    .unwrap();
    assert_eq!(core.snapshot().workspaces[0].id, WorkspaceId(1));
}

#[test]
fn schedules_tabs_are_bound_deduplicated_and_restore_without_starting_a_session() {
    let mut core = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    core.apply(Action::CreateSshWorkspace {
        name: "Remote".into(),
        connection: connection(),
        focus: true,
    })
    .unwrap();
    let before = core.snapshot();
    let workspace = before.workspaces[0].id;
    let source = before.workspaces[0].root.panes()[0].selected_surface;
    assert!(
        core.apply(Action::OpenSchedules { surface_id: source })
            .unwrap()
            .effects
            .is_empty()
    );
    let panel = core.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert_eq!(panel.kind, SurfaceKind::Schedules);
    assert_eq!(panel.scheduler_source, Some(source));
    assert!(panel.session_id.is_none());
    core.apply(Action::SelectSurface { surface_id: source })
        .unwrap();
    core.apply(Action::OpenSchedules { surface_id: source })
        .unwrap();
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0].selected_surface,
        panel.id
    );
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0].surfaces.len(),
        2
    );
    core.apply(Action::ReorderSurface {
        surface_id: panel.id,
        index: 0,
        focus: true,
    })
    .unwrap();
    let document = core.session_document().unwrap();
    let validated = document
        .validate(RestoreLimits {
            terminals: 1,
            browsers: 0,
            files: 0,
        })
        .unwrap();
    assert_eq!(validated.terminal_count(), 1);
    let restored =
        WorkspaceStore::prepare_session_restore(validated, &[SessionId(100)], None).unwrap();
    assert_eq!(restored.effects.len(), 1);
    let restored_panel = restored.store.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert_eq!(restored_panel.id, panel.id);
    assert_eq!(restored_panel.scheduler_source, Some(source));
    assert!(
        core.apply(Action::CloseSurface {
            surface_id: panel.id
        })
        .unwrap()
        .effects
        .is_empty()
    );
    assert!(
        core.apply(Action::ReopenClosed { closed_id: None })
            .unwrap()
            .effects
            .is_empty()
    );
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0]
            .selected()
            .scheduler_source,
        Some(source)
    );
    core.apply(Action::CloseSurface { surface_id: source })
        .unwrap();
    let document = core
        .session_document()
        .unwrap()
        .validate(RestoreLimits {
            terminals: 0,
            browsers: 0,
            files: 0,
        })
        .unwrap();
    assert_eq!(
        document.terminal_count(),
        0,
        "orphaned Schedules stays offline and owns no PTY"
    );
    let offline = WorkspaceStore::prepare_session_restore(document, &[], None).unwrap();
    assert!(offline.effects.is_empty());
    let offline_snapshot = offline.store.snapshot();
    let offline_panel = offline_snapshot.workspaces[0].root.panes()[0].selected();
    assert_eq!(offline_panel.kind, SurfaceKind::Schedules);
    assert_eq!(offline_panel.scheduler_source, Some(source));
    assert!(offline_panel.session_id.is_none());
    assert!(
        core.apply(Action::OpenSchedules { surface_id: source })
            .is_err()
    );
    core.apply(Action::CloseWorkspace {
        workspace_id: workspace,
    })
    .unwrap();
}

#[test]
fn schedules_reject_local_sources_and_keep_binding_when_moved_to_another_workspace() {
    let mut core = WorkspaceStore::default();
    core.apply(Action::CreateWorkspace {
        name: "Local".into(),
    })
    .unwrap();
    let local = core.snapshot().workspaces[0].root.panes()[0].selected_surface;
    let destination = core.snapshot().workspaces[0].focused_pane;
    let before = core.snapshot();
    assert!(
        core.apply(Action::OpenSchedules { surface_id: local })
            .is_err()
    );
    assert_eq!(core.snapshot(), before);
    core.apply(Action::CreateSshWorkspace {
        name: "Remote".into(),
        connection: connection(),
        focus: true,
    })
    .unwrap();
    let source = core.snapshot().workspaces[1].root.panes()[0].selected_surface;
    core.apply(Action::OpenSchedules { surface_id: source })
        .unwrap();
    let panel = core.snapshot().workspaces[1].root.panes()[0].selected_surface;
    assert!(
        core.apply(Action::MoveSurface {
            surface_id: panel,
            pane_id: destination,
            index: 1
        })
        .unwrap()
        .effects
        .is_empty()
    );
    core.apply(Action::OpenSchedules { surface_id: source })
        .unwrap();
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0].selected_surface,
        panel
    );
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0]
            .selected()
            .scheduler_source,
        Some(source)
    );
}

#[test]
fn same_host_schedules_follow_exact_source_through_reconnect_but_not_undo_close() {
    let mut core = WorkspaceStore::default();
    core.apply(Action::CreateSshWorkspace {
        name: "Remote".into(),
        connection: connection().for_restore(),
        focus: true,
    })
    .unwrap();
    let initial = core.snapshot();
    let pane = initial.workspaces[0].focused_pane;
    let source = initial.workspaces[0].root.panes()[0].selected().clone();
    core.apply(Action::CreateSurface { pane_id: pane }).unwrap();
    let other_source = core.snapshot().workspaces[0].root.panes()[0].selected_surface;
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0]
            .selected()
            .remote,
        source.remote,
        "both sources deliberately use the same SSH configuration"
    );

    let mut panels = Vec::new();
    for source_id in [source.id, other_source] {
        assert!(
            core.apply(Action::OpenSchedules {
                surface_id: source_id
            })
            .unwrap()
            .effects
            .is_empty()
        );
        let snapshot = core.snapshot();
        let panel = snapshot.workspaces[0].root.panes()[0].selected();
        assert_eq!(panel.kind, SurfaceKind::Schedules);
        assert_eq!(panel.scheduler_source, Some(source_id));
        panels.push(panel.id);
    }
    assert_ne!(panels[0], panels[1], "same host is not the source identity");

    core.apply(Action::ReconnectSsh {
        identity_namespace: initial.identity_namespace,
        surface_id: source.id,
        expected_session_id: source.session_id.unwrap(),
    })
    .unwrap();
    assert!(
        core.apply(Action::OpenSchedules {
            surface_id: source.id
        })
        .unwrap()
        .effects
        .is_empty()
    );
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0].selected_surface,
        panels[0],
        "a new SSH session in the same source retains its panel"
    );

    core.apply(Action::CloseSurface {
        surface_id: source.id,
    })
    .unwrap();
    core.apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    let reopened = core.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert_ne!(reopened.id, source.id);
    assert_eq!(reopened.remote, Some(connection().for_restore()));
    core.apply(Action::OpenSchedules {
        surface_id: reopened.id,
    })
    .unwrap();
    let snapshot = core.snapshot();
    let pane = snapshot.workspaces[0].root.panes()[0];
    let new_panel = pane.selected();
    assert!(!panels.contains(&new_panel.id));
    assert_eq!(new_panel.scheduler_source, Some(reopened.id));
    for (id, source_id) in panels.into_iter().zip([source.id, other_source]) {
        let panel = pane.surfaces.iter().find(|s| s.id == id).unwrap();
        assert_eq!(panel.scheduler_source, Some(source_id));
        assert!(panel.session_id.is_none());
    }
    assert_eq!(
        pane.surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Schedules)
            .count(),
        3
    );
}

#[test]
fn schedules_select_existing_at_tab_capacity_and_reject_new_panel_atomically() {
    let mut core = WorkspaceStore::default();
    core.apply(Action::CreateSshWorkspace {
        name: "Full remote".into(),
        connection: connection(),
        focus: true,
    })
    .unwrap();
    let initial = core.snapshot();
    let pane = initial.workspaces[0].focused_pane;
    let source = initial.workspaces[0].root.panes()[0].selected_surface;
    core.apply(Action::CreateSurface { pane_id: pane }).unwrap();
    let other_source = core.snapshot().workspaces[0].root.panes()[0].selected_surface;
    core.apply(Action::OpenSchedules { surface_id: source })
        .unwrap();
    let panel = core.snapshot().workspaces[0].root.panes()[0].selected_surface;
    for _ in 3..MAX_SURFACES_PER_PANE {
        core.apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.com/".into(),
            focus: true,
            profile_id: None,
        })
        .unwrap();
    }
    let last_browser = core.snapshot().workspaces[0].root.panes()[0].selected_surface;
    assert!(
        core.apply(Action::OpenSchedules { surface_id: source })
            .unwrap()
            .effects
            .is_empty()
    );
    assert_eq!(
        core.snapshot().workspaces[0].root.panes()[0].selected_surface,
        panel
    );
    let full = core.snapshot();
    assert_eq!(
        full.workspaces[0].root.panes()[0].surfaces.len(),
        MAX_SURFACES_PER_PANE
    );
    assert_eq!(
        core.apply(Action::OpenSchedules {
            surface_id: other_source
        }),
        Err(ActionError::Capacity {
            resource: "tabs per pane",
            limit: MAX_SURFACES_PER_PANE
        })
    );
    assert_eq!(
        core.snapshot(),
        full,
        "rejection must preserve selection and topology"
    );
    core.apply(Action::CloseSurface {
        surface_id: last_browser,
    })
    .unwrap();
    assert!(
        core.apply(Action::OpenSchedules {
            surface_id: other_source
        })
        .unwrap()
        .effects
        .is_empty()
    );
    let snapshot = core.snapshot();
    let pane = snapshot.workspaces[0].root.panes()[0];
    assert_eq!(pane.surfaces.len(), MAX_SURFACES_PER_PANE);
    assert_eq!(pane.selected().scheduler_source, Some(other_source));
}
