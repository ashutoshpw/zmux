//! The foreign envelope may grow only for one strict, bounded semantic paste.
use serde_json::{Value, json};
use zmux_native_bridge::*;

fn consume(id: u64) -> Value {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut data = vec![0; length.value as usize];
    // SAFETY: the owned destination is writable throughout this bounded copy.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, data.as_mut_ptr(), data.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    serde_json::from_slice(&data).unwrap()
}
fn dispatch(id: u64, payload: Value) -> ZmuxResult {
    let data = serde_json::to_vec(&json!({"version":1,"request_id":1,"payload":payload})).unwrap();
    // SAFETY: immutable encoded storage is readable until the call returns.
    unsafe { zmux_desktop_dispatch(id, data.as_ptr(), data.len()) }
}

#[test]
fn configured_foreign_desktop_bounds_whole_paste_and_rejects_envelope_bypasses() {
    let configuration = br#"{"terminal_engine":{"engine":"vt100"}}"#;
    // SAFETY: fixed configuration remains readable for its declared length.
    let desktop =
        unsafe { zmux_desktop_create_configured(configuration.as_ptr(), configuration.len()) };
    assert_eq!(desktop.status, 0);
    // A stale target is intentional: this boundary proves admission, not physical
    // delivery. Public Session tests independently inspect actual child bytes.
    let admitted = dispatch(
        desktop.value,
        json!({"command":"paste","session_id":u64::MAX,"text":"\0".repeat(16384)}),
    );
    assert_eq!(admitted.status, 0);
    assert_eq!(consume(admitted.value)["ok"], true);
    for payload in [
        json!({"command":"paste","session_id":u64::MAX,"text":"\0".repeat(16385)}),
        json!({"command":"paste","session_id":u64::MAX,"text":"x","unused":"x".repeat(17000)}),
    ] {
        let refused = dispatch(desktop.value, payload);
        assert_eq!(refused.status, 0);
        assert_eq!(consume(refused.value)["ok"], false);
    }
    let other = dispatch(
        desktop.value,
        json!({"command":"input","session_id":u64::MAX,"text":"x".repeat(16384)}),
    );
    assert_eq!(other.status, -2);
    // SAFETY: deliberately oversized length must be refused before dereferencing
    // this short storage, just as for the existing ordinary request cap.
    assert_eq!(
        unsafe { zmux_desktop_dispatch(desktop.value, b"{}".as_ptr(), 99329) }.status,
        -2
    );
    assert_eq!(zmux_desktop_destroy(desktop.value), 0);
    let invalid = br#"{"terminal_engine":{"engine":"vt100","unused":true}}"#;
    // SAFETY: fixed invalid JSON is readable; no desktop may be created.
    assert_eq!(
        unsafe { zmux_desktop_create_configured(invalid.as_ptr(), invalid.len()) }.status,
        -2
    );
}
