use serde_json::{Value, json};
use zmux_workspace_core::*;

fn fixture() -> WorkspaceStore {
    let mut store = WorkspaceStore::with_identity_namespace(
        uuid::Uuid::parse_str("98765432-1234-5678-9012-123456789012").unwrap(),
    );
    store
        .apply(Action::CreateWorkspace {
            name: "local".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    store
        .apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Vertical,
        })
        .unwrap();
    let last = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::CreateBrowser {
            profile_id: None,
            focus: true,
            pane_id: last,
            url: "https://example.org/docs?token=secret#private".into(),
        })
        .unwrap();
    store
}

fn limits() -> RestoreLimits {
    RestoreLimits {
        terminals: 8,
        browsers: 4,
    }
}

#[test]
fn document_restores_stable_topology_with_new_shells_and_private_browser_urls() {
    let mut store = fixture();
    let before = store.snapshot();
    let session = before.workspaces[0].root.panes()[0].surfaces[0]
        .session_id
        .unwrap();
    store.set_session_cwd(session, "/tmp/project".into());
    let document = store.session_document().unwrap();
    let wire = serde_json::to_string(&document).unwrap();
    assert!(!wire.contains("session_id"));
    assert!(!wire.contains("secret"));
    assert!(!wire.contains("private"));
    let validated = serde_json::from_str::<SessionDocument>(&wire)
        .unwrap()
        .validate(limits())
        .unwrap();
    assert_eq!(validated.terminal_count(), 3);
    assert_eq!(validated.browser_count(), 1);
    let prepared = WorkspaceStore::prepare_session_restore(
        validated,
        &[SessionId(1000), SessionId(1001), SessionId(1002)],
        None,
    )
    .unwrap();
    let after = prepared.store.snapshot();
    assert_eq!(after.identity_namespace, before.identity_namespace);
    assert_eq!(after.focused_workspace, before.focused_workspace);
    assert_eq!(after.windows, before.windows);
    assert_eq!(after.focused_window, before.focused_window);
    assert_eq!(after.workspaces[0].id, before.workspaces[0].id);
    assert_eq!(after.workspaces[0].name, "local");
    assert!(matches!(
        after.workspaces[0].root,
        PaneTree::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
    assert_eq!(
        after.workspaces[0].focused_pane,
        before.workspaces[0].focused_pane
    );
    for (old, new) in before.workspaces[0]
        .root
        .panes()
        .iter()
        .zip(after.workspaces[0].root.panes())
    {
        assert_eq!(old.id, new.id);
        assert_eq!(old.selected_surface, new.selected_surface);
        for (old, new) in old.surfaces.iter().zip(&new.surfaces) {
            assert_eq!(old.id, new.id);
            assert_eq!(
                before.object_uuid(targets::TargetKind::Surface, old.id.0),
                after.object_uuid(targets::TargetKind::Surface, new.id.0)
            );
            if old.kind == SurfaceKind::Terminal {
                assert_ne!(old.session_id, new.session_id);
            } else {
                assert_eq!(new.url.as_deref(), Some("https://example.org/docs"));
            }
        }
    }
    assert!(prepared.effects.iter().any(|effect| matches!(effect, Effect::StartSession { cwd: Some(cwd), .. } if cwd == "/tmp/project")));
    assert_eq!(prepared.effects.len(), 3);
    assert!(after.closed_items.is_empty());
    assert!(prepared.store.session_document().unwrap().next_id > 1002);
}

#[test]
fn version_four_requires_one_checked_window_and_canonical_ownership() {
    let wire = serde_json::to_value(fixture().session_document().unwrap()).unwrap();
    let workspace = wire["workspaces"][0]["id"].clone();
    let mut cases = Vec::new();
    let mut missing = wire.clone();
    missing.as_object_mut().unwrap().remove("window_layout");
    cases.push(missing);
    let mut null = wire.clone();
    null["window_layout"] = Value::Null;
    cases.push(null);
    let mut absent = wire.clone();
    absent["workspaces"] = json!([]);
    absent["focused_workspace"] = Value::Null;
    absent["window_layout"] = json!({"windows": [], "focused_window": null});
    cases.push(absent);
    let mut multiple = wire.clone();
    multiple["window_layout"]["windows"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": 99, "workspace_ids": [], "selected_workspace": null
        }));
    multiple["next_id"] = json!(100);
    cases.push(multiple);
    for (membership, selection) in [
        (json!([]), Value::Null),
        (json!([workspace.clone(), workspace.clone()]), workspace),
        (json!([999]), json!(999)),
    ] {
        let mut bad = wire.clone();
        bad["window_layout"]["windows"][0]["workspace_ids"] = membership;
        bad["window_layout"]["windows"][0]["selected_workspace"] = selection;
        cases.push(bad);
    }
    for (field, value) in [
        ("id", wire["workspaces"][0]["id"].clone()),
        ("selected_workspace", Value::Null),
        ("selected_workspace", json!(999)),
    ] {
        let mut bad = wire.clone();
        bad["window_layout"]["windows"][0][field] = value;
        cases.push(bad);
    }
    for focus in [Value::Null, json!(999)] {
        let mut bad = wire.clone();
        bad["window_layout"]["focused_window"] = focus;
        cases.push(bad);
    }
    for (index, bad) in cases.into_iter().enumerate() {
        assert!(
            serde_json::from_value::<SessionDocument>(bad)
                .and_then(|doc| doc.validate(limits()).map_err(serde::de::Error::custom))
                .is_err(),
            "malformed ownership case {index} accepted"
        );
    }
    let validated = serde_json::from_value::<SessionDocument>(wire)
        .unwrap()
        .validate(limits())
        .unwrap();
    assert!(
        WorkspaceStore::prepare_session_restore(
            validated,
            &[SessionId(1000), SessionId(1001), SessionId(1002)],
            Some(WindowId(2000))
        )
        .is_err()
    );
}

#[test]
fn legacy_versions_require_fresh_reserved_window_identity_and_keep_logical_ids() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "legacy".into(),
        })
        .unwrap();
    let before = store.snapshot();
    let wire = serde_json::to_value(store.session_document().unwrap()).unwrap();
    for version in [1, 2, 3] {
        let mut legacy = wire.clone();
        legacy["version"] = json!(version);
        // Layouts are versioned data, never silently discarded by an older reader.
        assert!(
            serde_json::from_value::<SessionDocument>(legacy.clone())
                .unwrap()
                .validate(limits())
                .is_err()
        );
        legacy.as_object_mut().unwrap().remove("window_layout");
        let validated = serde_json::from_value::<SessionDocument>(legacy)
            .unwrap()
            .validate(limits())
            .unwrap();
        assert!(validated.requires_window_id());
        for id in [
            None,
            Some(WindowId(1)),
            Some(WindowId(1000)),
            Some(WindowId(MAX_NEXT_IDENTIFIER)),
        ] {
            assert!(
                WorkspaceStore::prepare_session_restore(validated.clone(), &[SessionId(1000)], id)
                    .is_err()
            );
        }
        let prepared = WorkspaceStore::prepare_session_restore(
            validated,
            &[SessionId(1000)],
            Some(WindowId(1001)),
        )
        .unwrap();
        let after = prepared.store.snapshot();
        assert_eq!(
            after.windows,
            vec![Window {
                id: WindowId(1001),
                workspace_ids: vec![before.workspaces[0].id],
                selected_workspace: before.focused_workspace
            }]
        );
        assert_eq!(after.identity_namespace, before.identity_namespace);
        assert_eq!(after.workspaces[0].id, before.workspaces[0].id);
        assert_eq!(
            after.workspaces[0].focused_pane,
            before.workspaces[0].focused_pane
        );
        assert_eq!(
            after.workspaces[0].root.panes()[0].selected_surface,
            before.workspaces[0].root.panes()[0].selected_surface
        );
        assert_eq!(
            after.workspaces[0].root.panes()[0].selected().session_id,
            Some(SessionId(1000))
        );
        assert_eq!(prepared.store.next_identifier(), 1002);
        assert_eq!(prepared.store.session_document().unwrap().version, 4);
    }
}

#[test]
fn saved_window_membership_order_drives_the_singleton_native_catalog() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    for name in ["first", "second"] {
        store
            .apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let mut document = store.session_document().unwrap();
    let window = &mut document.window_layout.as_mut().unwrap().windows[0];
    window.workspace_ids.reverse();
    window.selected_workspace = Some(window.workspace_ids[1]);
    document.focused_workspace = window.selected_workspace;
    let order = window.workspace_ids.clone();
    let mut conflicting = document.clone();
    conflicting.focused_workspace = Some(order[0]);
    assert!(conflicting.validate(limits()).is_err());
    let prepared = WorkspaceStore::prepare_session_restore(
        document.validate(limits()).unwrap(),
        &[SessionId(1000), SessionId(1001)],
        None,
    )
    .unwrap();
    assert_eq!(
        prepared
            .store
            .snapshot()
            .workspaces
            .iter()
            .map(|w| w.id)
            .collect::<Vec<_>>(),
        order
    );
    assert_eq!(prepared.store.snapshot().focused_workspace, Some(order[1]));
}

#[test]
fn ssh_export_cannot_replay_startup_commands() {
    let mut store = fixture();
    store
        .apply(Action::CreateSshWorkspace {
            name: "remote".into(),
            focus: true,
            connection: zmux_remote_runtime::SshConnectionConfig {
                destination: "alice@example.org".into(),
                config_file: None,
                port: Some(2222),
                identity: Some("/tmp/key".into()),
                forward_agent: Some(false),
                ssh_options: vec!["BatchMode=yes".into()],
                jump_hosts: vec![],
                command: Some(vec!["echo".into(), "startup-secret".into()]),
            },
        })
        .unwrap();
    let document = store.session_document().unwrap();
    let wire = serde_json::to_string(&document).unwrap();
    assert!(!wire.contains("command"));
    assert!(!wire.contains("startup-secret"));
    let validated = document.validate(limits()).unwrap();
    let prepared = WorkspaceStore::prepare_session_restore(
        validated,
        &[
            SessionId(1000),
            SessionId(1001),
            SessionId(1002),
            SessionId(1003),
        ],
        None,
    )
    .unwrap();
    let remote = prepared
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::StartSession {
                remote: Some(remote),
                ..
            } => Some(remote),
            _ => None,
        })
        .unwrap();
    assert_eq!(remote.destination, "alice@example.org");
    assert_eq!(remote.port, Some(2222));
    assert!(remote.command.is_none());
}

#[test]
fn malformed_documents_and_stale_runtime_ids_are_rejected_before_restore() {
    let wire = serde_json::to_value(fixture().session_document().unwrap()).unwrap();
    let mut cases: Vec<Value> = Vec::new();
    for (key, value) in [
        ("format", json!("foreign.session")),
        ("version", json!(99)),
        ("identity_namespace", json!(uuid::Uuid::nil())),
        ("focused_workspace", json!(9999)),
        ("next_id", json!(1)),
        ("next_id", json!(u64::MAX)),
    ] {
        let mut bad = wire.clone();
        bad[key] = value;
        cases.push(bad);
    }
    let mut duplicate = wire.clone();
    duplicate["workspaces"][0]["root"]["second"]["pane"]["id"] =
        duplicate["workspaces"][0]["root"]["first"]["pane"]["id"].clone();
    cases.push(duplicate);
    let mut selected = wire.clone();
    selected["workspaces"][0]["root"]["first"]["pane"]["selected_surface"] = json!(999);
    cases.push(selected);
    let mut cwd = wire.clone();
    cwd["workspaces"][0]["root"]["first"]["pane"]["surfaces"][0]["content"]["cwd"] =
        json!("relative/path");
    cases.push(cwd);
    let mut focus = wire.clone();
    focus["workspaces"][0]["focused_pane"] = json!(999);
    cases.push(focus);
    let mut empty = wire.clone();
    empty["workspaces"][0]["root"]["first"]["pane"]["surfaces"] = json!([]);
    cases.push(empty);
    let mut name = wire.clone();
    name["workspaces"][0]["name"] = json!("invalid\nname");
    cases.push(name);
    for bad in cases {
        assert!(
            serde_json::from_value::<SessionDocument>(bad)
                .unwrap()
                .validate(limits())
                .is_err()
        );
    }
    let mut unknown = wire;
    unknown["workspaces"][0]["root"]["first"]["pane"]["surfaces"][0]["content"]["command"] =
        json!(["danger"]);
    assert!(serde_json::from_value::<SessionDocument>(unknown).is_err());
    let document = fixture().session_document().unwrap();
    assert!(
        document
            .clone()
            .validate(RestoreLimits {
                terminals: 2,
                browsers: 4
            })
            .is_err()
    );
    assert!(
        document
            .clone()
            .validate(RestoreLimits {
                terminals: 8,
                browsers: 0
            })
            .is_err()
    );
    let validated = document.validate(limits()).unwrap();
    for ids in [
        vec![SessionId(1000)],
        vec![SessionId(1000), SessionId(1000), SessionId(1002)],
        vec![SessionId(1), SessionId(1001), SessionId(1002)],
        vec![SessionId(u64::MAX), SessionId(1001), SessionId(1002)],
    ] {
        assert!(WorkspaceStore::prepare_session_restore(validated.clone(), &ids, None).is_err());
    }
}

#[test]
fn structural_limits_reject_oversized_topology_and_accept_an_empty_desktop() {
    let document = fixture().session_document().unwrap();
    let SavedTree::Split { first, .. } = &document.workspaces[0].root else {
        panic!("split fixture")
    };
    let mut deep = document.clone();
    let mut tree = first.as_ref().clone();
    for _ in 0..=MAX_SPLIT_DEPTH {
        tree = SavedTree::Split {
            axis: Axis::Horizontal,
            first: Box::new(tree),
            second: first.clone(),
        };
    }
    deep.workspaces[0].root = tree;
    assert_eq!(
        deep.validate(limits()).unwrap_err().0,
        "split depth limit exceeded"
    );

    let mut surfaces = document.clone();
    let SavedTree::Split { first, .. } = &mut surfaces.workspaces[0].root else {
        unreachable!()
    };
    let SavedTree::Leaf { pane } = first.as_mut() else {
        unreachable!()
    };
    pane.surfaces
        .resize(MAX_SURFACES_PER_PANE + 1, pane.surfaces[0].clone());
    assert!(
        surfaces
            .validate(limits())
            .unwrap_err()
            .0
            .contains("bounded nonempty")
    );

    let mut workspaces = document.clone();
    workspaces
        .workspaces
        .resize(MAX_WORKSPACES + 1, document.workspaces[0].clone());
    assert_eq!(
        workspaces.validate(limits()).unwrap_err().0,
        "workspace limit exceeded"
    );

    let mut empty = document;
    empty.workspaces.clear();
    empty.focused_workspace = None;
    for window in &mut empty.window_layout.as_mut().unwrap().windows {
        window.workspace_ids.clear();
        window.selected_workspace = None;
    }
    let prepared =
        WorkspaceStore::prepare_session_restore(empty.validate(limits()).unwrap(), &[], None)
            .unwrap();
    assert!(prepared.effects.is_empty());
    assert!(prepared.store.snapshot().workspaces.is_empty());
    assert!(prepared.store.snapshot().focused_workspace.is_none());
}

#[test]
fn reserved_allocator_floors_and_restore_revisions_only_advance_and_fail_closed_on_overflow() {
    let mut store = fixture();
    let initial = store.snapshot();
    let next = store.next_identifier();
    store.advance_identifier_floor(4096).unwrap();
    store.advance_identifier_floor(next).unwrap();
    assert_eq!(store.next_identifier(), 4096);
    store
        .apply(Action::CreateSurface {
            pane_id: initial.workspaces[0].focused_pane,
        })
        .unwrap();
    let allocated = store.snapshot();
    assert!(
        allocated.workspaces[0]
            .root
            .panes()
            .iter()
            .flat_map(|p| &p.surfaces)
            .any(|surface| surface.id.0 >= 4096)
    );
    assert!(store.advance_identifier_floor(u64::MAX).is_err());
    assert_eq!(store.snapshot(), allocated);
    store
        .advance_restore_revision(allocated.revision + 100)
        .unwrap();
    let rebased = store.snapshot();
    assert_eq!(rebased.revision, allocated.revision + 101);
    assert!(store.advance_restore_revision(u64::MAX).is_err());
    assert_eq!(store.snapshot(), rebased);
}

#[test]
fn browser_profile_session_versions_never_drop_or_invent_storage_identity() {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "profiles".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    let profile = uuid::Uuid::new_v4();
    store
        .apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.org/?secret=1".into(),
            focus: true,
            profile_id: Some(profile),
        })
        .unwrap();
    let wire = serde_json::to_value(store.session_document().unwrap()).unwrap();
    assert_eq!(wire["version"], 4);
    assert_eq!(
        wire["workspaces"][0]["root"]["pane"]["surfaces"][1]["content"]["profile_id"],
        profile.to_string()
    );
    let restore = |wire: Value| {
        let validated = serde_json::from_value::<SessionDocument>(wire)?
            .validate(limits())
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        let window = validated.requires_window_id().then_some(WindowId(2000));
        WorkspaceStore::prepare_session_restore(validated, &[SessionId(1000)], window)
            .map_err(|e| serde::de::Error::custom(e.to_string()))
    };
    let restored: Result<_, serde_json::Error> = restore(wire.clone());
    let restored = restored.unwrap().store.snapshot();
    assert_eq!(
        restored.workspaces[0].root.panes()[0].surfaces[1].browser_profile_id,
        Some(profile)
    );
    let mut legacy = wire.clone();
    legacy["version"] = json!(1);
    legacy.as_object_mut().unwrap().remove("window_layout");
    // Even explicit null is a v2 field and must not be accepted then forgotten by v1.
    legacy["workspaces"][0]["root"]["pane"]["surfaces"][1]["content"]["profile_id"] = Value::Null;
    assert!(restore(legacy.clone()).is_err());
    legacy["workspaces"][0]["root"]["pane"]["surfaces"][1]["content"]
        .as_object_mut()
        .unwrap()
        .remove("profile_id");
    let restored = restore(legacy.clone()).unwrap().store.snapshot();
    assert_eq!(
        restored.workspaces[0].root.panes()[0].surfaces[1].browser_profile_id,
        None
    );
    legacy["version"] = json!(2);
    assert!(restore(legacy).is_err());
    store
        .apply(Action::CloseSurface {
            surface_id: store.snapshot().workspaces[0].root.panes()[0].selected_surface,
        })
        .unwrap();
    store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0]
            .selected()
            .browser_profile_id,
        Some(profile)
    );
}

#[test]
fn tmux_session_v3_preserves_only_immutable_identity_and_rejects_older_versions_or_commands() {
    let mut store = fixture();
    let pane = store.snapshot().workspaces[0].root.panes()[0].id;
    let identity = zmux_remote_runtime::local_multiplexer::LocalTmuxIdentity {
        id: uuid::Uuid::new_v4(),
        server_incarnation: uuid::Uuid::new_v4(),
        tmux_session_id: "$7".into(),
        created_at: 42,
    };
    store
        .apply(Action::CreateLocalTmuxSurface {
            pane_id: pane,
            identity: identity.clone(),
            focus: false,
        })
        .unwrap();
    let wire = serde_json::to_value(store.session_document().unwrap()).unwrap();
    let content = &wire["workspaces"][0]["root"]["first"]["pane"]["surfaces"][2]["content"];
    assert_eq!(content["kind"], "local_tmux_terminal");
    assert_eq!(
        content["identity"],
        serde_json::to_value(&identity).unwrap()
    );
    assert_eq!(content.as_object().unwrap().len(), 2);
    let validated = serde_json::from_value::<SessionDocument>(wire.clone())
        .unwrap()
        .validate(limits())
        .unwrap();
    let restored = WorkspaceStore::prepare_session_restore(
        validated,
        &[
            SessionId(1000),
            SessionId(1001),
            SessionId(1002),
            SessionId(1003),
        ],
        None,
    )
    .unwrap();
    assert!(
        restored.store.snapshot().workspaces[0].root.panes()[0]
            .surfaces
            .iter()
            .any(|s| s.local_tmux.as_ref() == Some(&identity))
    );
    assert!(restored.effects.iter().any(|effect|matches!(effect,Effect::StartSession {local_tmux:Some(saved),..} if saved.as_ref()==&identity)));
    for version in [1, 2] {
        let mut old = wire.clone();
        old["version"] = json!(version);
        old.as_object_mut().unwrap().remove("window_layout");
        assert!(
            serde_json::from_value::<SessionDocument>(old)
                .unwrap()
                .validate(limits())
                .is_err()
        );
    }
    let mut injected = wire.clone();
    injected["workspaces"][0]["root"]["first"]["pane"]["surfaces"][2]["content"]["command"] =
        json!("do not replay");
    assert!(serde_json::from_value::<SessionDocument>(injected).is_err());
    let mut malformed = wire;
    malformed["workspaces"][0]["root"]["first"]["pane"]["surfaces"][2]["content"]["identity"]["tmux_session_id"] =
        json!("-unsafe");
    assert!(
        serde_json::from_value::<SessionDocument>(malformed)
            .unwrap()
            .validate(limits())
            .is_err()
    );
}
