//! Actual foreign-call admission: bounded storage, response reservation and owner fencing.
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zmux_native_bridge::*;
fn consume(id: u64) -> Value {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut data = vec![0; length.value as usize];
    // SAFETY: the owned destination is writable for exactly its declared capacity.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, data.as_mut_ptr(), data.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    serde_json::from_slice(&data).unwrap()
}
fn snapshot(desktop: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = zmux_desktop_snapshot(desktop, 0);
        if result.status == 0 && result.value != 0 {
            let value = consume(result.value);
            if predicate(&value) {
                return value;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn download(desktop: u64, command: &Value) -> ZmuxResult {
    let data = serde_json::to_vec(command).unwrap();
    // SAFETY: this immutable encoded request remains valid throughout the call.
    unsafe { zmux_desktop_download(desktop, data.as_ptr(), data.len()) }
}
#[test]
fn foreign_download_capacity_precedes_admission_and_callbacks_require_exact_owner() {
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let initial = snapshot(desktop.value, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let pane = &initial["model"]["workspaces"][0]["focused_pane"];
    let create = serde_json::to_vec(&json!({"version":1,"request_id":1,"payload":{"command":"action","action":{"action":"create_browser","pane_id":pane,"url":"https://example.org","focus":true}}})).unwrap();
    // SAFETY: readable request storage is retained for the duration of dispatch.
    let dispatched = unsafe { zmux_desktop_dispatch(desktop.value, create.as_ptr(), create.len()) };
    assert_eq!(dispatched.status, 0);
    assert_eq!(consume(dispatched.value)["ok"], true);
    let created = snapshot(desktop.value, |v| v["last_request_id"] == 1);
    let pane = &created["model"]["workspaces"][0]["root"]["pane"];
    let surface = pane["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "browser")
        .unwrap();
    let host = Uuid::new_v4();
    let owner = json!({"epoch":created["browser_epoch"],"identity_namespace":created["model"]["identity_namespace"],"surface_id":surface["id"],"host_id":host,"document_generation":1,"model_navigation":surface["navigation"]});
    let event = serde_json::to_vec(&json!({"event":"attach","epoch":created["browser_epoch"],"surface_id":surface["id"],"host_id":host,"document_generation":1,"model_navigation":surface["navigation"]})).unwrap();
    // SAFETY: checked bytes remain readable until acknowledgement.
    assert_eq!(
        unsafe { zmux_desktop_browser_event(desktop.value, event.as_ptr(), event.len()) },
        0
    );
    let token =
        json!({"epoch":created["download_epoch"],"host_id":host,"download_id":Uuid::new_v4()});
    let begin = json!({"operation":"begin","owner":owner,"token":token,"filename":"abi.bin"});
    let malformed = b"{}";
    // SAFETY: null is deliberately rejected before dereference, and the oversized
    // length is rejected before reading from the small live storage.
    assert_eq!(
        unsafe { zmux_desktop_download(desktop.value, std::ptr::null(), 1) }.status,
        -2
    );
    assert_eq!(
        unsafe { zmux_desktop_download(desktop.value, malformed.as_ptr(), 16_385) }.status,
        -2
    );
    assert_eq!(
        unsafe { zmux_desktop_download(desktop.value, malformed.as_ptr(), malformed.len()) }.status,
        -2
    );
    let core = zmux_core_create();
    assert_eq!(core.status, 0);
    let request = br#"{"version":1,"command":{"method":"snapshot"}}"#;
    let mut held = Vec::new();
    for _ in 0..16 {
        // SAFETY: this fixed request's storage is valid through dispatch.
        let result = unsafe { zmux_core_dispatch(core.value, request.as_ptr(), request.len()) };
        assert_eq!(result.status, 0);
        held.push(result.value);
    }
    assert_eq!(download(desktop.value, &begin).status, -5);
    for id in held {
        assert_eq!(zmux_buffer_release(id), 0);
    }
    assert_eq!(zmux_core_destroy(core.value), 0);
    assert!(
        snapshot(desktop.value, |_| true)["downloads"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let admitted = download(desktop.value, &begin);
    assert_eq!(admitted.status, 0);
    assert_eq!(consume(admitted.value)["result"], token);
    let mut forged = token.clone();
    forged["host_id"] = json!(Uuid::new_v4());
    let rejected = download(
        desktop.value,
        &json!({"operation":"cancelled","token":forged,"cleanup_complete":true}),
    );
    assert_eq!(rejected.status, 0);
    assert_eq!(consume(rejected.value)["ok"], false);
    let mut stale = begin.clone();
    stale["owner"]["document_generation"] = json!(2);
    stale["token"]["download_id"] = json!(Uuid::new_v4());
    let rejected = download(desktop.value, &stale);
    assert_eq!(rejected.status, 0);
    assert_eq!(consume(rejected.value)["ok"], false);
    // Reserve final publication before shutdown; this reservation must win
    // against cancellation while a second ordinary download is cancelled.
    for request in [
        json!({"operation":"destination","token":token,"filename":"abi.bin"}),
        json!({"operation":"finalizing","token":token}),
    ] {
        let result = download(desktop.value, &request);
        assert_eq!(result.status, 0);
        assert_eq!(consume(result.value)["ok"], true);
    }
    let mut cancelled_token = token.clone();
    cancelled_token["download_id"] = json!(Uuid::new_v4());
    let mut another = begin.clone();
    another["token"] = cancelled_token.clone();
    let result = download(desktop.value, &another);
    assert_eq!(result.status, 0);
    assert_eq!(consume(result.value)["ok"], true);
    // Real native owners remain charged while the common deadline expires.
    // Trusted cleanup acknowledgments must still execute after normal admission stops.
    let shutdown = zmux_desktop_shutdown_begin(desktop.value, 1, 1);
    assert_eq!(shutdown.status, 0);
    let started = consume(shutdown.value);
    let ticket = started["ticket"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(10));
    let repeated = zmux_desktop_shutdown_begin(desktop.value, 30_000, 0);
    assert_eq!(repeated.status, 0);
    let repeated = consume(repeated.value);
    assert_eq!(repeated["ticket"], ticket);
    assert_eq!(repeated["remaining_ms"], 0);
    assert_eq!(repeated["pending"]["native_cleanup"], true);
    assert_eq!(repeated["physical_cleanup_complete"], false);
    assert_eq!(zmux_desktop_shutdown_release(desktop.value, ticket), -4);
    assert_eq!(zmux_desktop_destroy(desktop.value), -4);
    let no_new_owner = download(desktop.value, &begin);
    assert_eq!(no_new_owner.status, 0);
    assert_eq!(consume(no_new_owner.value)["ok"], false);
    let cancelled = download(
        desktop.value,
        &json!({"operation":"cancelled","token":cancelled_token,"cleanup_complete":true}),
    );
    assert_eq!(cancelled.status, 0);
    assert_eq!(consume(cancelled.value)["ok"], true);
    let saved = download(
        desktop.value,
        &json!({"operation":"saved","token":token,"path":"/abi.bin","bytes":7,"cleanup_complete":true}),
    );
    assert_eq!(saved.status, 0);
    assert_eq!(
        consume(saved.value)["ok"],
        true,
        "shutdown revoked an existing final publication reservation"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = zmux_desktop_shutdown_status(desktop.value, ticket);
        assert_eq!(result.status, 0);
        let report = consume(result.value);
        if report["runtime_cleanup_complete"] == true {
            assert_eq!(report["pending"]["native_downloads"], 0);
            assert_eq!(report["physical_cleanup_complete"], false);
            break;
        }
        assert!(Instant::now() < deadline, "{report}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        zmux_desktop_shutdown_native_complete(desktop.value, ticket + 1),
        -2
    );
    assert_eq!(
        zmux_desktop_shutdown_native_complete(desktop.value, ticket),
        0
    );
    loop {
        let result = zmux_desktop_shutdown_status(desktop.value, ticket);
        assert_eq!(result.status, 0);
        let report = consume(result.value);
        if report["physical_cleanup_complete"] == true {
            assert_eq!(report["phase"], "complete");
            assert_eq!(report["deadline_expired"], true);
            break;
        }
        assert!(Instant::now() < deadline, "{report}");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(zmux_desktop_shutdown_release(desktop.value, ticket), 0);
    assert_eq!(zmux_desktop_snapshot(desktop.value, 0).status, -1);
}
