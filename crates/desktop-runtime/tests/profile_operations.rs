//! Shared production owner with real persistence and logical shutdown. Separate
//! executable keeps its PTY fork from inheriting neighboring lease-test guards.
#[path = "support/profile_fixture.rs"]
mod profile_fixture;
use profile_fixture::Fixture;
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::profile_leases::{
    LeaseService, MutationRequest, NativeMutationOutcome,
    journal::{MutationKind, MutationPhase, QUARANTINE_SENTINEL},
};
#[test]
fn shared_runtime_receipt_fences_new_opens_and_waits_for_native_physical_catalog_chain() {
    let _process_isolation = profile_fixture::process_fixture_isolation();
    use std::sync::Arc;
    use zmux_desktop_runtime::{
        Desktop, DesktopOptions, Shell, profile_native::NativeProfileOperations,
        profiles::ProfileCommand,
    };
    let fixture = Fixture::new();
    let service = Arc::new(LeaseService::start(fixture.lease_root()).unwrap());
    let controller = Uuid::new_v4();
    let native = Arc::new(
        NativeProfileOperations::new(controller, fixture.state(), service.clone()).unwrap(),
    );
    let desktop = Arc::new(
        Desktop::with_options(
            Shell::default(),
            DesktopOptions {
                state_directory: Some(fixture.state()),
                profile_native: Some(native.clone()),
                ..Default::default()
            },
        )
        .unwrap(),
    );
    let snapshot = |deadline: Instant| loop {
        match desktop.snapshot() {
            Ok(snapshot)
                if !snapshot.browser_profiles.loading && !snapshot.persistence.startup_pending =>
            {
                break snapshot;
            }
            Ok(_) | Err(zmux_desktop_runtime::DesktopError::Busy) => (),
            Err(e) => panic!("snapshot: {e}"),
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    };
    snapshot(Instant::now() + Duration::from_secs(5));
    let receipt = desktop
        .native_profile_command(ProfileCommand::Delete {
            profile: fixture.profiles[0].to_string(),
        })
        .unwrap();
    let intent = receipt.intent.unwrap();
    assert_eq!(receipt.state, "pending");
    assert!(
        desktop
            .native_profile_command(ProfileCommand::Clear {
                profile: fixture.profiles[0].to_string()
            })
            .is_err()
    );
    let socket = fixture.root.join("control.sock");
    let server =
        zmux_desktop_runtime::control::ControlServer::start(&socket, desktop.clone()).unwrap();
    let wire = |command: serde_json::Value| {
        use std::{
            io::{Read, Write},
            os::unix::net::UnixStream,
        };
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let data = serde_json::to_vec(
            &serde_json::json!({"version": 2, "request_id": 17, "command": command}),
        )
        .unwrap();
        stream
            .write_all(&(data.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&data).unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut response = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut response).unwrap();
        serde_json::from_slice::<serde_json::Value>(&response).unwrap()
    };
    let status = wire(
        serde_json::json!({"method":"browser_profile_intent_status", "profile":fixture.profiles[0], "intent":intent}),
    );
    assert_eq!(status["result"]["phase"], "native_preflight_pending");
    let pane = snapshot(Instant::now() + Duration::from_secs(2))
        .model
        .workspaces[0]
        .focused_pane;
    for (profile, allowed) in [(fixture.profiles[0], false), (fixture.profiles[1], true)] {
        let action = zmux_workspace_core::Action::CreateBrowser {
            profile_id: Some(profile),
            pane_id: pane,
            url: "about:blank".into(),
            focus: false,
        };
        let reply = wire(serde_json::json!({"method":"action", "action":action}));
        assert_eq!(reply["ok"], allowed, "{reply}");
    }
    let forged = wire(
        serde_json::json!({"method":"browser_profile_native", "operation":"finish", "intent":intent}),
    );
    assert_eq!(forged["error"]["code"], "invalid_request");
    server
        .set_policy(
            zmux_desktop_runtime::control_policy::ControlPolicy::same_user(
                zmux_desktop_runtime::control_policy::ControlAccess::ReadOnly,
            ),
        )
        .unwrap();
    let denied = wire(
        serde_json::json!({"method":"browser_profile_reconcile", "profile":fixture.profiles[0], "intent":intent}),
    );
    assert_eq!(denied["error"]["code"], "policy_denied");
    assert!(native.drain_reconciliations().unwrap().is_empty());
    server
        .set_policy(zmux_desktop_runtime::control_policy::ControlPolicy::default())
        .unwrap();
    let recovery = wire(
        serde_json::json!({"method":"browser_profile_reconcile", "profile":fixture.profiles[0], "intent":intent}),
    );
    let recovery_id: Uuid = recovery["result"]["id"].as_str().unwrap().parse().unwrap();
    let recovery_status =
        wire(serde_json::json!({"method":"browser_profile_reconcile_status", "id":recovery_id}));
    assert_eq!(recovery_status["result"]["state"], "pending");
    drop(server);
    let requests = native.drain().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].intent, intent);
    assert!(native.drain().unwrap().is_empty());
    assert!(native.complete(intent, Ok(())).is_err());
    let token = service
        .mutation_start(MutationRequest {
            catalog: fixture.state(),
            controller,
            intent,
            profile: fixture.profiles[0],
            kind: MutationKind::Delete,
            local_stores: vec![],
        })
        .unwrap();
    service
        .mutation_advance(token, MutationPhase::ClearDispatched)
        .unwrap();
    assert!(
        native
            .complete(intent, Err("generic timeout after native dispatch".into()))
            .is_err()
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
    let recovery = native.drain_reconciliations().unwrap();
    assert_eq!(recovery.len(), 1);
    assert_eq!(recovery[0].id, recovery_id);
    native
        .finish_reconciliation(
            recovery_id,
            Some("original callback remains unknown; no native work replayed".into()),
        )
        .unwrap();
    assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
    let report = desktop
        .begin_shutdown(Duration::from_secs(5), false)
        .unwrap();
    assert!(!report.physical_cleanup_complete);
    for phase in [
        MutationPhase::ClearCompleted,
        MutationPhase::RemoveDispatched,
        MutationPhase::RemoveCompleted,
    ] {
        service.mutation_advance(token, phase).unwrap();
    }
    service
        .mutation_finish(token, NativeMutationOutcome::Completed)
        .unwrap();
    // Logical Desktop shutdown did not remove the original service/callback owner.
    native.complete(intent, Ok(())).unwrap();
    let report = desktop
        .wait_shutdown(report.ticket, Duration::from_secs(5))
        .unwrap();
    assert!(report.physical_cleanup_complete, "{report:?}");
    let status = native.status(fixture.profiles[0], intent).unwrap();
    assert_eq!(status.record.phase, MutationPhase::Completed);
    assert!(!status.physical_cleanup_pending);
    let catalog: zmux_desktop_runtime::profiles::Catalog = serde_json::from_slice(
        &fs::read(
            fixture
                .state()
                .join(zmux_desktop_runtime::profiles::CATALOG_NAME),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!catalog.profiles.iter().any(|v| v.id == fixture.profiles[0]));
    assert!(catalog.profiles.iter().any(|v| v.id == fixture.profiles[1]));
}

#[test]
fn terminal_sentinel_status_is_truthful_and_cleanup_needs_exact_original_intent() {
    let _process_isolation = profile_fixture::process_fixture_isolation();
    use std::io::Write;
    let fixture = Fixture::new();
    let service = LeaseService::start(fixture.lease_root()).unwrap();
    let token = service
        .mutation_start(MutationRequest {
            catalog: fixture.state(),
            controller: Uuid::new_v4(),
            intent: Uuid::new_v4(),
            profile: fixture.profiles[0],
            kind: MutationKind::Clear,
            local_stores: vec![],
        })
        .unwrap();
    for phase in [
        MutationPhase::ClearDispatched,
        MutationPhase::ClearCompleted,
        MutationPhase::RemoveDispatched,
        MutationPhase::RemoveCompleted,
    ] {
        service.mutation_advance(token, phase).unwrap();
    }
    service
        .mutation_finish(token, NativeMutationOutcome::Completed)
        .unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(fixture.path())
        .unwrap();
    file.write_all(QUARANTINE_SENTINEL).unwrap();
    file.sync_all().unwrap();
    let status = service.intent_status(token.profile, token.intent).unwrap();
    assert_eq!(status.record.phase, MutationPhase::Completed);
    assert!(status.physical_cleanup_pending && !status.outcome_unknown);
    assert!(
        service
            .recover_terminal(token.profile, Uuid::new_v4())
            .is_err()
    );
    assert_eq!(fs::read(fixture.path()).unwrap(), QUARANTINE_SENTINEL);
    let status = service
        .recover_terminal(token.profile, token.intent)
        .unwrap();
    assert!(!status.physical_cleanup_pending);
    assert!(fs::read(fixture.path()).unwrap().is_empty());
}

#[test]
fn known_native_failure_reconciliation_never_reports_destructive_success() {
    let _process_isolation = profile_fixture::process_fixture_isolation();
    use std::sync::Arc;
    use zmux_desktop_runtime::profile_native::NativeProfileOperations;
    for partial in [false, true] {
        let fixture = Fixture::new();
        let service = Arc::new(LeaseService::start(fixture.lease_root()).unwrap());
        let controller = Uuid::new_v4();
        let native =
            NativeProfileOperations::new(controller, fixture.state(), service.clone()).unwrap();
        let token = service
            .mutation_start(MutationRequest {
                catalog: fixture.state(),
                controller,
                intent: Uuid::new_v4(),
                profile: fixture.profiles[0],
                kind: MutationKind::Delete,
                local_stores: vec![],
            })
            .unwrap();
        let outcome = if partial {
            for phase in [
                MutationPhase::ClearDispatched,
                MutationPhase::ClearCompleted,
                MutationPhase::RemoveDispatched,
            ] {
                service.mutation_advance(token, phase).unwrap();
            }
            NativeMutationOutcome::RemovalFailed {
                message: "original public removal callback failed".into(),
            }
        } else {
            NativeMutationOutcome::AbortedBeforeClear {
                message: "known pre-dispatch refusal".into(),
            }
        };
        service.mutation_finish(token, outcome).unwrap();
        // Optional diagnostic text must not turn a known failure phase into success.
        let path = fixture
            .lease_root()
            .join(format!("browser-profile-{}.intent.json", token.profile));
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["error"] = serde_json::Value::Null;
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        let receipt = native
            .request_reconcile(token.profile, token.intent)
            .unwrap();
        let drained = native.drain_reconciliations().unwrap();
        assert_eq!(drained.len(), 1);
        native.finish_reconciliation(receipt.id, None).unwrap();
        let result = native.reconciliation_status(receipt.id).unwrap();
        assert_eq!(result.state, "failed");
        assert_eq!(
            result.original_phase,
            Some(if partial {
                MutationPhase::FailedPartial
            } else {
                MutationPhase::FailedBeforeClear
            })
        );
        assert!(result.error.unwrap().contains("original operation failed_"));
        let successor = native
            .request_reconcile(token.profile, token.intent)
            .unwrap();
        assert_ne!(successor.id, receipt.id);
        assert!(native.finish_reconciliation(receipt.id, None).is_err());
        assert_eq!(
            native.reconciliation_status(successor.id).unwrap().state,
            "pending"
        );
        assert!(fs::read(fixture.path()).unwrap().is_empty());
    }
}
