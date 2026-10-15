//! The native window export reserves response storage before host registration.
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_native_bridge::*;
fn consume(id: u64) -> Value {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut bytes = vec![0; length.value as usize];
    // SAFETY: the owned byte vector is writable for its exact declared length.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, bytes.as_mut_ptr(), bytes.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    serde_json::from_slice(&bytes).unwrap()
}
fn snapshot(id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = zmux_desktop_snapshot(id, 0);
        if result.status == 0 && result.value != 0 {
            let value = consume(result.value);
            if value["window_requests"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
            {
                return value;
            }
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn window_abi_fences_storage_capacity_and_host_generation_before_observation() {
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let initial = snapshot(desktop.value);
    let request = &initial["window_requests"][0];
    let token = json!({"epoch":request["epoch"],"identity_namespace":request["identity_namespace"],"window_id":request["window_id"],"host_id":Uuid::new_v4()});
    let bytes = serde_json::to_vec(
        &json!({"event":"complete","token":token,"operation_id":request["operation_id"]}),
    )
    .unwrap();
    // SAFETY: null and oversized storage are deliberately rejected before access.
    assert_eq!(
        unsafe { zmux_desktop_window_event(desktop.value, std::ptr::null(), 1) }.status,
        -2
    );
    assert_eq!(
        unsafe { zmux_desktop_window_event(desktop.value, bytes.as_ptr(), 16_385) }.status,
        -2
    );
    let core = zmux_core_create();
    let query = br#"{"version":1,"command":{"method":"snapshot"}}"#;
    let mut held = Vec::new();
    for _ in 0..16 {
        // SAFETY: the immutable query remains readable throughout dispatch.
        let result = unsafe { zmux_core_dispatch(core.value, query.as_ptr(), query.len()) };
        assert_eq!(result.status, 0);
        held.push(result.value);
    }
    // SAFETY: the encoded native event is retained through the call.
    assert_eq!(
        unsafe { zmux_desktop_window_event(desktop.value, bytes.as_ptr(), bytes.len()) }.status,
        -5
    );
    for id in held {
        assert_eq!(zmux_buffer_release(id), 0);
    }
    assert_eq!(zmux_core_destroy(core.value), 0);
    assert!(
        snapshot(desktop.value)["window_hosts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // SAFETY: the immutable event storage remains live until acknowledgement.
    let result = unsafe { zmux_desktop_window_event(desktop.value, bytes.as_ptr(), bytes.len()) };
    assert_eq!(result.status, 0);
    assert_eq!(consume(result.value)["ok"], true);
    let mut forged = token;
    forged["host_id"] = json!(Uuid::new_v4());
    let key = serde_json::to_vec(&json!({"event":"key","token":forged,"key":true})).unwrap();
    // SAFETY: the encoded event is readable for the exact declared length.
    let result = unsafe { zmux_desktop_window_event(desktop.value, key.as_ptr(), key.len()) };
    assert_eq!(result.status, 0);
    assert_eq!(consume(result.value)["ok"], false);
    assert_eq!(zmux_desktop_destroy(desktop.value), 0);
}
