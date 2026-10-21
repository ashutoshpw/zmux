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
