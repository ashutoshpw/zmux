//! Attach-only descriptor and provisional identity contracts at the core owner.
use serde_json::{Value, json};
use zmux_workspace_core::*;

fn fixture() -> WorkspaceStore {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::from_u128(1));
    store
        .apply(Action::CreateWorkspace {
            name: "persistent".into(),
        })
        .unwrap();
    store
}
fn document() -> Value {
    let mut value = serde_json::to_value(fixture().session_document().unwrap()).unwrap();
    value["version"] = json!(6);
    value["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"] = json!({
        "kind": "local_zellij_terminal",
        "profile_id": uuid::Uuid::from_u128(10),
        "owner_uuid": uuid::Uuid::from_u128(20)
    });
    value
}
fn validate(value: Value) -> Result<ValidatedSessionDocument, String> {
    serde_json::from_value::<SessionDocument>(value)
        .map_err(|e| e.to_string())?
        .validate(RestoreLimits {
            terminals: 8,
            browsers: 8,
        })
        .map_err(|e| e.to_string())
}
#[test]
fn v6_preserves_the_exact_zellij_owner_instead_of_rejecting_or_downgrading_it() {
    let validated = validate(document()).expect("v6 must accept an attach-only Zellij owner");
    let prepared =
        WorkspaceStore::prepare_session_restore(validated, &[SessionId(100)], None).unwrap();
    let saved = serde_json::to_value(prepared.store.session_document().unwrap()).unwrap();
    assert_eq!(
        saved["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"],
        document()["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
    );
}

fn identity() -> LocalZellijIdentity {
    LocalZellijIdentity {
        profile_id: uuid::Uuid::from_u128(10),
        owner_uuid: uuid::Uuid::from_u128(20),
    }
}
#[test]
fn descriptor_admission_rejects_old_versions_and_malformed_or_executable_payloads() {
    for version in 1..=5 {
        let mut wire = document();
        wire["version"] = json!(version);
        if version < 5 {
            wire["workspaces"][0]
                .as_object_mut()
                .unwrap()
                .remove("todos");
        }
        if version < 4 {
            wire.as_object_mut().unwrap().remove("window_layout");
        }
        assert!(
            validate(wire)
                .unwrap_err()
                .contains("require session version 6")
        );
    }
    for field in ["profile_id", "owner_uuid"] {
        for value in [Value::Null, json!(uuid::Uuid::nil()), json!("not-a-uuid")] {
            let mut wire = document();
            wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"][field] = value;
            assert!(validate(wire).is_err());
        }
        let mut wire = document();
        wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(validate(wire).is_err());
    }
    for field in [
        "command",
        "socket",
        "environment",
        "cwd",
        "connection",
        "identity",
    ] {
        let mut wire = document();
        wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"][field] = json!("injected");
        assert!(
            validate(wire).is_err(),
            "unexpected descriptor field {field}"
        );
    }
    let parsed: SessionDocument = serde_json::from_value(document()).unwrap();
    assert!(
        parsed
            .validate(RestoreLimits {
                terminals: 0,
                browsers: 8
            })
            .is_err()
    );
}
#[test]
fn provisional_reservations_burn_ids_without_publishing_or_relaunching_and_prepare_latest_state() {
    let mut store = fixture();
    let initial = store.snapshot();
    let pane = initial.workspaces[0].focused_pane;
    let floor = store.next_identifier();
    let canceled = store.reserve_local_zellij_surface(pane).unwrap();
    assert_eq!(
        store.snapshot(),
        initial,
        "reservation is not a placeholder terminal"
    );
    assert_eq!(store.next_identifier(), floor + 2);
    let burned = canceled.surface_id();
    store.cancel_local_zellij_reservation(canceled).unwrap();
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    assert!(
        store.snapshot().workspaces[0].root.panes()[0]
            .selected_surface
            .0
            > burned.0
    );
    let ticket = store.reserve_local_zellij_surface(pane).unwrap();
    let mut foreign = WorkspaceStore::with_identity_namespace(uuid::Uuid::from_u128(999));
    foreign
        .apply(Action::CreateWorkspace {
            name: "foreign".into(),
        })
        .unwrap();
    assert!(
        foreign
            .prepare_local_zellij_surface(&ticket, identity(), false)
            .is_err()
    );
    let expected = vec![todo::TodoEntry {
        id: uuid::Uuid::from_u128(30),
        text: "latest todo".into(),
        state: todo::TodoState::Completed,
        origin: todo::TodoOrigin::Agent,
    }];
    store
        .replace_workspace_todos(initial.workspaces[0].id, expected.clone())
        .unwrap();
    let latest = store.snapshot();
    let prepared = store
        .prepare_local_zellij_surface(&ticket, identity(), false)
        .unwrap();
    assert_eq!(
        store.snapshot(),
        latest,
        "preparation cannot commit before queue admission"
    );
    let candidate = prepared.store().snapshot();
    assert_eq!(candidate.workspaces[0].todos, expected);
    assert_eq!(
        candidate.workspaces[0].root.panes()[0].selected_surface,
        latest.workspaces[0].root.panes()[0].selected_surface
    );
    let surface = candidate.workspaces[0].root.panes()[0]
        .surfaces
        .last()
        .unwrap();
    assert_eq!(surface.id, ticket.surface_id());
    assert_eq!(surface.session_id, Some(ticket.session_id()));
    assert_eq!(surface.local_zellij, Some(identity()));
    assert!(surface.remote.is_none() && surface.local_tmux.is_none());
    store = prepared.into_store();
    assert!(
        store
            .prepare_local_zellij_surface(&ticket, identity(), true)
            .is_err(),
        "installed ticket cannot be replayed"
    );
    let restored = WorkspaceStore::prepare_session_restore(
        store
            .session_document()
            .unwrap()
            .validate(RestoreLimits {
                terminals: 8,
                browsers: 8,
            })
            .unwrap(),
        &[SessionId(1000), SessionId(1001), SessionId(1002)],
        None,
    )
    .unwrap();
    assert_eq!(restored.store.snapshot().workspaces[0].todos, expected);
    assert_eq!(restored.effects.iter().filter(|effect| matches!(effect, Effect::AttachLocalZellijSession { identity: saved, .. } if saved == &identity())).count(), 1);
}
#[test]
fn reservation_revalidates_capacity_target_and_identity_before_any_install() {
    let mut store = fixture();
    let pane = store.snapshot().workspaces[0].focused_pane;
    let ticket = store.reserve_local_zellij_surface(pane).unwrap();
    let before = store.snapshot();
    assert!(
        store
            .prepare_local_zellij_surface(
                &ticket,
                LocalZellijIdentity {
                    profile_id: uuid::Uuid::nil(),
                    owner_uuid: identity().owner_uuid
                },
                true
            )
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    for _ in 1..32 {
        if store
            .apply(Action::CreateSurface { pane_id: pane })
            .is_err()
        {
            break;
        }
    }
    let full = store.snapshot();
    assert!(
        store
            .prepare_local_zellij_surface(&ticket, identity(), true)
            .is_err()
    );
    assert_eq!(store.snapshot(), full);
    store.apply(Action::ClosePane { pane_id: pane }).unwrap();
    assert!(
        store
            .prepare_local_zellij_surface(&ticket, identity(), true)
            .is_err()
    );
    store.cancel_local_zellij_reservation(ticket).unwrap();
    let mut store = fixture();
    let pane = store.snapshot().workspaces[0].focused_pane;
    let tickets: Vec<_> = (0..MAX_LOCAL_ZELLIJ_RESERVATIONS)
        .map(|_| store.reserve_local_zellij_surface(pane).unwrap())
        .collect();
    let floor = store.next_identifier();
    assert!(store.reserve_local_zellij_surface(pane).is_err());
    assert_eq!(
        store.next_identifier(),
        floor,
        "rejected reservation must not consume IDs"
    );
    for ticket in tickets {
        store.cancel_local_zellij_reservation(ticket).unwrap();
    }
    assert!(store.reserve_local_zellij_surface(pane).is_ok());
}
#[test]
fn workspace_last_tab_and_pane_recovery_attach_exact_owner_and_failed_retry_never_starts_workload()
{
    for close in ["workspace", "tab", "pane"] {
        let mut store = fixture();
        let original = store.snapshot().workspaces[0].clone();
        let pane = original.focused_pane;
        let ticket = store.reserve_local_zellij_surface(pane).unwrap();
        let surface = ticket.surface_id();
        store = store
            .prepare_local_zellij_surface(&ticket, identity(), true)
            .unwrap()
            .into_store();
        store
            .apply(Action::CloseSurface {
                surface_id: original.root.panes()[0].selected_surface,
            })
            .unwrap();
        let todos = vec![todo::TodoEntry {
            id: uuid::Uuid::from_u128(30),
            text: "recover exact todos".into(),
            state: todo::TodoState::InProgress,
            origin: todo::TodoOrigin::Agent,
        }];
        store
            .replace_workspace_todos(original.id, todos.clone())
            .unwrap();
        store
            .apply(match close {
                "workspace" => Action::CloseWorkspace {
                    workspace_id: original.id,
                },
                "tab" => Action::CloseSurface {
                    surface_id: surface,
                },
                _ => Action::ClosePane { pane_id: pane },
            })
            .unwrap();
        let descriptor = store.snapshot().closed_items.last().unwrap().clone();
        for success in [false, true] {
            let reopened = store
                .apply(Action::ReopenClosed {
                    closed_id: Some(descriptor.id),
                })
                .unwrap();
            let [
                Effect::AttachLocalZellijSession {
                    session_id,
                    identity: saved,
                    ..
                },
            ] = reopened.effects.as_slice()
            else {
                panic!("recovery must only Attach: {:?}", reopened.effects)
            };
            assert_eq!(saved, &identity());
            assert_eq!(store.snapshot().workspaces[0].todos, todos);
            store.finish_restore(*session_id, success);
            if !success {
                assert!(store.snapshot().workspaces.is_empty());
                assert_eq!(store.snapshot().closed_items.last(), Some(&descriptor));
            }
        }
        assert_eq!(
            store.snapshot().workspaces[0].root.panes()[0]
                .selected()
                .local_zellij,
            Some(identity())
        );
    }
}
