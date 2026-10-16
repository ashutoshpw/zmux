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

// Independent wire fixture: binding digest precomputed from the versioned field
// order, not produced by the receipt validator under test. This is history data,
// not a live supervisor authentication fixture.
fn completed_document() -> Value {
    let mut value = document();
    value["version"] = json!(7);
    let receipt: Value = serde_json::from_str(r#"{"version":1,"domain":"zmux.local-zellij.terminal.v1","binding":{"version":1,"owner_uuid":"00000000-0000-0000-0000-000000000014","owner_generation":"00000000-0000-0000-0000-000000000015","namespace":{"root":[1,11],"socket":[1,12]},"completion_socket":[1,13],"server":{"pid":101,"uid":1000,"parent_pid":1,"start":[20,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zellij"},"supervisor":{"pid":102,"uid":1000,"parent_pid":101,"start":[30,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"workload":{"pid":103,"uid":1000,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/bin/sh"}},"binding_digest":"ada9261a1a1bd868e4b920118c71247a61ba3bdc5cabbea46737aad6ce06050e","terminal_sequence":1,"workload_exit":{"kind":"exited","value":7},"child_reaped":true}"#).unwrap();
    value["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"] = json!({
        "kind":"completed_local_zellij_terminal",
        "profile_id":uuid::Uuid::from_u128(10), "owner_uuid":uuid::Uuid::from_u128(20),
        "history":{"receipt":receipt,"viewport":{"contents":"workload exited 7","truncated":false}}
    });
    value
}
#[test]
fn v7_completed_history_restores_without_allocating_a_pty_or_replaying_attach() {
    let wire = completed_document();
    let validated = validate(wire.clone()).expect("v7 completed owner history must validate");
    assert_eq!(
        validated.terminal_count(),
        0,
        "completed history cannot consume launch IDs or physical PTY capacity"
    );
    let mut prepared = WorkspaceStore::prepare_session_restore(validated, &[], None).unwrap();
    assert!(
        prepared.effects.is_empty(),
        "completed history must not launch Attach or ordinary shell"
    );
    let snapshot = serde_json::to_value(prepared.store.snapshot()).unwrap();
    let surface = &snapshot["workspaces"][0]["root"]["pane"]["surfaces"][0];
    assert!(surface["session_id"].is_null());
    let saved = serde_json::to_value(prepared.store.session_document().unwrap()).unwrap();
    assert_eq!(saved["version"], SESSION_DOCUMENT_VERSION);
    assert_eq!(
        saved["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"],
        wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
    );
    let surface_id = prepared.store.snapshot().workspaces[0].root.panes()[0].surfaces[0].id;
    prepared
        .store
        .apply(Action::CloseSurface { surface_id })
        .unwrap();
    let closed_id = prepared.store.snapshot().closed_items[0].id;
    let reopened = prepared
        .store
        .apply(Action::ReopenClosed {
            closed_id: Some(closed_id),
        })
        .unwrap();
    assert!(
        reopened.effects.is_empty(),
        "undo of completed history must remain completed"
    );
}
#[test]
fn v7_completed_history_rejects_corrupt_receipts_owner_mismatch_and_unbounded_viewport() {
    fn content(value: &mut Value) -> &mut Value {
        &mut value["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
    }
    for corruption in ["version", "owner", "digest", "reaped", "exit", "viewport"] {
        let mut value = completed_document();
        match corruption {
            "version" => value["version"] = json!(6),
            "owner" => content(&mut value)["owner_uuid"] = json!(uuid::Uuid::from_u128(25)),
            "digest" => {
                content(&mut value)["history"]["receipt"]["binding_digest"] = json!("0".repeat(64))
            }
            "reaped" => content(&mut value)["history"]["receipt"]["child_reaped"] = json!(false),
            "exit" => {
                content(&mut value)["history"]["receipt"]["workload_exit"]["value"] = json!(256)
            }
            "viewport" => {
                content(&mut value)["history"]["viewport"]["contents"] =
                    json!("x".repeat(16 * 1024 + 1))
            }
            _ => unreachable!(),
        }
        assert!(validate(value).is_err(), "{corruption}");
    }
}

// Independently encoded V2 history fixture; the digest is precomputed from the
// agreed canonical field order. It supplies no live publisher/Run authority.
fn admitted_document() -> Value {
    let mut value = completed_document();
    value["version"] = json!(8);
    value["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]["history"]["receipt"] =
        serde_json::from_str(r#"{"execution_contract":"admitted_lifetime_v2","receipt":{"version":2,"domain":"zmux.local-zellij.terminal.v2","binding":{"version":2,"domain":"zmux.local-zellij.execution.v2","owner_uuid":"00000000-0000-0000-0000-000000000014","owner_generation":"00000000-0000-0000-0000-000000000015","namespace":{"root":[1,11],"socket":[1,12]},"completion_socket":[1,13],"server":{"pid":101,"uid":1000,"parent_pid":1,"start":[20,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zellij"},"supervisor":{"pid":102,"uid":1000,"parent_pid":101,"start":[30,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"workload_birth":{"pid":103,"uid":1000,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111","executable":"/opt/zmux"},"lifetime":{"pid":103,"parent_pid":102,"start":[40,0],"boot":"11111111-1111-4111-8111-111111111111"},"invocation":{"shell":"/bin/sh","invocation_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"admission_id":"00000000-0000-0000-0000-000000000016"},"binding_digest":"20e3c6ee26ec6507888571e2af25149b82e71a0ba0ee38d4602667300acedfa1","terminal_sequence":1,"workload_exit":{"kind":"exited","value":7},"child_reaped":true,"execution_observation":{"kind":"not_observed"}}}"#).unwrap();
    value
}
#[test]
fn v8_admitted_lifetime_history_preserves_classification_without_pty_or_start() {
    for observation in [
        json!({"kind":"not_observed"}),
        json!({"kind":"shell_observed"}),
        json!({"kind":"exec_failed","stage":"configured_shell_exec","errno":2}),
    ] {
        let mut wire = admitted_document();
        wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]["history"]["receipt"]["receipt"]
            ["execution_observation"] = observation;
        let validated =
            validate(wire.clone()).expect("V8 must accept explicitly admitted lifetime history");
        assert_eq!(validated.terminal_count(), 0);
        let mut prepared = WorkspaceStore::prepare_session_restore(validated, &[], None).unwrap();
        assert!(
            prepared.effects.is_empty(),
            "history cannot authorize Attach or Start"
        );
        let snapshot = prepared.store.snapshot();
        let surface = &snapshot.workspaces[0].root.panes()[0].surfaces[0];
        assert!(surface.session_id.is_none());
        let saved = serde_json::to_value(prepared.store.session_document().unwrap()).unwrap();
        assert_eq!(saved["version"], 8);
        assert_eq!(
            saved["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"],
            wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
        );
        let closed = prepared
            .store
            .apply(Action::CloseSurface {
                surface_id: surface.id,
            })
            .unwrap();
        assert!(closed.effects.is_empty());
        let closed_id = prepared.store.snapshot().closed_items[0].id;
        let reopened = prepared
            .store
            .apply(Action::ReopenClosed {
                closed_id: Some(closed_id),
            })
            .unwrap();
        assert!(
            reopened.effects.is_empty(),
            "undo must preserve admitted history without replay"
        );
        let reopened = serde_json::to_value(prepared.store.session_document().unwrap()).unwrap();
        assert_eq!(
            reopened["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"],
            wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"]
        );
    }
}
#[test]
fn v8_history_rejects_contract_downgrade_mixed_identity_and_invalid_observation() {
    for corruption in [
        "v7",
        "unknown_contract",
        "mixed_contract",
        "raw_v2",
        "split_lifetime",
        "owner",
        "errno",
        "stage",
        "viewport",
    ] {
        let mut wire = admitted_document();
        let content = &mut wire["workspaces"][0]["root"]["pane"]["surfaces"][0]["content"];
        let envelope = &mut content["history"]["receipt"];
        match corruption {
            "v7" => wire["version"] = json!(7),
            "unknown_contract" => envelope["execution_contract"] = json!("full_image_v1"),
            "mixed_contract" => envelope["workload_exit"] = json!({"kind":"exited","value":0}),
            "raw_v2" => *envelope = envelope["receipt"].clone(),
            "split_lifetime" => {
                envelope["receipt"]["binding"]["lifetime"]["pid"] = json!(104);
                // Recomputed independently for this exact mismatched tuple, so
                // digest rejection cannot mask missing identity-projection checks.
                envelope["receipt"]["binding_digest"] =
                    json!("4da6a2a9613474189338645721e4ed45fb6f04b515311ee4e03dfc8578365b14");
            }
            "owner" => content["owner_uuid"] = json!(uuid::Uuid::from_u128(25)),
            "errno" => {
                envelope["receipt"]["execution_observation"] =
                    json!({"kind":"exec_failed","stage":"configured_shell_exec","errno":0})
            }
            "stage" => {
                envelope["receipt"]["execution_observation"] =
                    json!({"kind":"exec_failed","stage":"user_command","errno":2})
            }
            "viewport" => {
                content["history"]["viewport"]["contents"] = json!("x".repeat(16 * 1024 + 1))
            }
            _ => unreachable!(),
        }
        assert!(validate(wire).is_err(), "accepted {corruption}");
    }
}
