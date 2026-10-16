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
