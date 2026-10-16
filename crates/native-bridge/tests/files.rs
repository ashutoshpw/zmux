//! Foreign-call behaviour of the trusted file channel: frame validation, response reservation
//! before work, owner fencing and a real open/attach/save/reload round trip through the exports.
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{DesktopOptions, files::FilesConfig};
use zmux_native_bridge::*;

/// The C registry uses non-blocking locks (contention reads as BUSY), so tests that share the
/// process-wide registry run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const INVALID_HANDLE: i32 = -1;
const INVALID_ARGUMENT: i32 = -2;
const BUSY: i32 = -4;
const CAPACITY: i32 = -5;

fn bytes_of(id: u64) -> Vec<u8> {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut data = vec![0; length.value as usize];
    // SAFETY: the owned destination is writable for exactly its declared capacity.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, data.as_mut_ptr(), data.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    data
}
fn json_of(id: u64) -> Value {
    serde_json::from_slice(&bytes_of(id)).unwrap()
}
fn snapshot(desktop: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let result = zmux_desktop_snapshot(desktop, 0);
        if result.status == 0 && result.value != 0 {
            let value = json_of(result.value);
            if predicate(&value) {
                return value;
            }
        }
        assert!(Instant::now() < deadline, "snapshot never matched");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn frame(header: &Value, body: &[u8]) -> Vec<u8> {
    let header = serde_json::to_vec(header).unwrap();
    let mut out = (header.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    out
}
fn call_raw(desktop: u64, request: &[u8]) -> ZmuxResult {
    // SAFETY: the request storage is live for the whole call.
    unsafe { zmux_desktop_file(desktop, request.as_ptr(), request.len()) }
}
fn call(desktop: u64, header: &Value, body: &[u8]) -> (Value, Vec<u8>) {
    let result = call_raw(desktop, &frame(header, body));
    assert_eq!(result.status, 0, "{header}");
    split(&bytes_of(result.value))
}
fn split(reply: &[u8]) -> (Value, Vec<u8>) {
    let length = u32::from_be_bytes(reply[..4].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&reply[4..4 + length]).unwrap(),
        reply[4 + length..].to_vec(),
    )
}
fn ready_desktop() -> (u64, Value) {
    let desktop = zmux_desktop_create();
    assert_eq!(desktop.status, 0);
    let initial = snapshot(desktop.value, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    (desktop.value, initial)
}
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("zmux-bridge-files-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn open_and_attach(desktop: u64, path: &std::path::Path) -> (Value, Vec<u8>, Value) {
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":[{"kind":"path","value":path}],"focus":true}),
        &[],
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let item = &opened["result"]["items"][0];
    assert_eq!(item["ok"], true, "{opened}");
    let surface = item["surface"]["surface_id"].clone();
    let state = snapshot(desktop, |v| {
        v["files"]["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    });
    let (attached, body) = call(
        desktop,
        &json!({"op":"attach","epoch":state["files"]["epoch"],"surface_id":surface,"host_id":Uuid::new_v4()}),
        &[],
    );
    assert_eq!(attached["ok"], true, "{attached}");
    (attached.clone(), body, attached["owner"].clone())
}

#[test]
fn frames_are_validated_before_any_work() {
    let _serial = serial();
    let (desktop, _) = ready_desktop();
    let good_header = json!({"op":"detach","owner":{"epoch":Uuid::nil(),"identity_namespace":Uuid::nil(),"surface_id":1,"host_id":Uuid::nil()}});
    // Null, empty, shorter than the length prefix.
    assert_eq!(
        unsafe { zmux_desktop_file(desktop, std::ptr::null(), 8) }.status,
        INVALID_ARGUMENT
    );
    assert_eq!(call_raw(desktop, &[]).status, INVALID_ARGUMENT);
    assert_eq!(call_raw(desktop, &[0, 0, 0]).status, INVALID_ARGUMENT);
    // header_len beyond the supplied bytes, and beyond the 16 KiB header cap.
    assert_eq!(
        call_raw(desktop, &[0, 0, 0, 200, b'{']).status,
        INVALID_ARGUMENT
    );
    let mut huge = (16_385u32).to_be_bytes().to_vec();
    huge.extend(vec![b' '; 16_385]);
    assert_eq!(call_raw(desktop, &huge).status, INVALID_ARGUMENT);
    assert_eq!(
        call_raw(desktop, &(u32::MAX).to_be_bytes()).status,
        INVALID_ARGUMENT
    );
    // Not JSON, unknown op, unknown field, missing field.
    assert_eq!(
        call_raw(desktop, &frame(&json!("nope"), &[])).status,
        INVALID_ARGUMENT
    );
    let mut bad = (3u32).to_be_bytes().to_vec();
    bad.extend_from_slice(b"{x}");
    assert_eq!(call_raw(desktop, &bad).status, INVALID_ARGUMENT);
    assert_eq!(
        call_raw(desktop, &frame(&json!({"op":"format_disk"}), &[])).status,
        INVALID_ARGUMENT
    );
    let mut extra = good_header.clone();
    extra["surprise"] = json!(true);
    assert_eq!(
        call_raw(desktop, &frame(&extra, &[])).status,
        INVALID_ARGUMENT
    );
    assert_eq!(
        call_raw(desktop, &frame(&json!({"op":"attach"}), &[])).status,
        INVALID_ARGUMENT
    );
    // A body is only legal for save; non-save frames are capped at 16384 bytes in total.
    assert_eq!(
        call_raw(desktop, &frame(&good_header, b"body")).status,
        INVALID_ARGUMENT
    );
    let mut oversize = frame(&good_header, &[]);
    oversize.resize(16_385, b' ');
    assert_eq!(call_raw(desktop, &oversize).status, INVALID_ARGUMENT);
    // URL operands are not part of this local-file channel.
    let url = frame(
        &json!({"op":"open","items":[{"kind":"url","value":"https://example.org"}]}),
        &[],
    );
    assert_eq!(call_raw(desktop, &url).status, INVALID_ARGUMENT);
    // The save frame has its own explicit cap: header + 2 MiB body, and not one byte more.
    let save = json!({"op":"save","owner":good_header["owner"],"base_generation":1});
    let header_len = serde_json::to_vec(&save).unwrap().len();
    let too_big = vec![b'a'; 2 * 1024 * 1024 + 1];
    assert_eq!(
        call_raw(desktop, &frame(&save, &too_big)).status,
        INVALID_ARGUMENT
    );
    assert_eq!(MAX_FILE_SAVE_FRAME, 16_384 + 2 * 1024 * 1024);
    assert!(header_len < 16_384);
    // Wrong handle.
    assert_eq!(
        call_raw(desktop + 1000, &frame(&good_header, &[])).status,
        INVALID_HANDLE
    );
    // A well-formed frame is answered, with a typed failure rather than a C status.
    let (reply, body) = call(desktop, &good_header, &[]);
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "stale_owner");
    assert!(body.is_empty());
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}
const MAX_FILE_SAVE_FRAME: usize = zmux_native_bridge::file_channel::MAX_FILE_SAVE_FRAME_BYTES;

#[test]
fn open_attach_save_reload_round_trip_through_the_exports() {
    let _serial = serial();
    let dir = TempDir::new();
    let path = dir.0.join("round-trip.txt");
    std::fs::write(&path, "first\r\nsecond\n").unwrap();
    let (desktop, _) = ready_desktop();
    let (attached, body, owner) = open_and_attach(desktop, &path);
    assert_eq!(body, b"first\r\nsecond\n");
    assert_eq!(attached["mode"], "text");
    assert_eq!(attached["generation"], 1);
    assert_eq!(attached["writable"], true);
    assert_eq!(attached["size"], 14);

    // A 1.5 MiB save is far beyond the ordinary 16 KiB request cap.
    let text = "x".repeat(1_500_000);
    let (saved, _) = call(
        desktop,
        &json!({"op":"save","owner":owner,"base_generation":1,"force":false}),
        text.as_bytes(),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["generation"], 2);
    assert_eq!(saved["size"], 1_500_000);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);

    // A conflicting writer is reported, and force overwrites.
    std::fs::write(&path, "external").unwrap();
    let (conflict, _) = call(
        desktop,
        &json!({"op":"save","owner":owner,"base_generation":2}),
        b"mine",
    );
    assert_eq!(conflict["ok"], false);
    assert_eq!(conflict["error"]["code"], "conflict");
    assert_eq!(conflict["conflict"]["kind"], "modified");
    assert_eq!(conflict["outcome_unknown"], false);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external");
    let (reloaded, body) = call(
        desktop,
        &json!({"op":"reload","owner":owner,"discard":false}),
        &[],
    );
    assert_eq!(reloaded["ok"], true);
    assert_eq!(reloaded["generation"], 3);
    assert_eq!(body, b"external");

    // Dirty observation goes through the trusted event and is visible in the model.
    let (observed, _) = call(
        desktop,
        &json!({"op":"observe","owner":owner,"dirty":true}),
        &[],
    );
    assert_eq!(observed["ok"], true);
    let dirty = snapshot(desktop, |v| {
        v["model"]["workspaces"][0]["root"]["pane"]["surfaces"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["file"]["dirty"] == true))
    });
    assert!(dirty["files"]["entries"][0]["last_op"]["kind"] == "reload");
    let (refused, _) = call(
        desktop,
        &json!({"op":"reload","owner":owner,"discard":false}),
        &[],
    );
    assert_eq!(refused["error"]["code"], "dirty");
    let (detached, _) = call(desktop, &json!({"op":"detach","owner":owner}), &[]);
    assert_eq!(detached["ok"], true);
    let (stale, _) = call(
        desktop,
        &json!({"op":"reload","owner":owner,"discard":true}),
        &[],
    );
    assert_eq!(stale["error"]["code"], "stale_owner");
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn a_forged_owner_changes_nothing_on_disk() {
    let _serial = serial();
    let dir = TempDir::new();
    let path = dir.0.join("guarded.txt");
    std::fs::write(&path, "guarded").unwrap();
    let (desktop, _) = ready_desktop();
    let (_, _, owner) = open_and_attach(desktop, &path);
    for field in ["host_id", "epoch", "identity_namespace"] {
        let mut forged = owner.clone();
        forged[field] = json!(Uuid::new_v4());
        let (reply, _) = call(
            desktop,
            &json!({"op":"save","owner":forged,"base_generation":1,"force":true}),
            b"hijacked",
        );
        assert_eq!(reply["error"]["code"], "stale_owner", "{field}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "guarded");
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn response_capacity_is_claimed_before_any_file_work() {
    let _serial = serial();
    let dir = TempDir::new();
    let path = dir.0.join("capacity.txt");
    std::fs::write(&path, "x").unwrap();
    let (desktop, initial) = ready_desktop();
    let core = zmux_core_create();
    let request = br#"{"version":1,"command":{"method":"snapshot"}}"#;
    let mut held = Vec::new();
    for _ in 0..16 {
        // SAFETY: this fixed request's storage is valid through dispatch.
        let result = unsafe { zmux_core_dispatch(core.value, request.as_ptr(), request.len()) };
        if result.status != 0 {
            break;
        }
        held.push(result.value);
    }
    let open = frame(
        &json!({"op":"open","items":[{"kind":"path","value":path}],"focus":true}),
        &[],
    );
    assert_eq!(call_raw(desktop, &open).status, CAPACITY);
    for id in held {
        assert_eq!(zmux_buffer_release(id), 0);
    }
    assert_eq!(zmux_core_destroy(core.value), 0);
    // Nothing ran: no file tab exists.
    let after = snapshot(desktop, |_| true);
    assert_eq!(
        after["model"]["workspaces"][0]["root"], initial["model"]["workspaces"][0]["root"],
        "capacity rejection must precede the tab creation"
    );
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn observe_returns_busy_not_ok_when_the_command_queue_is_full() {
    let _serial = serial();
    let hold = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let hook_hold = hold.clone();
    let dir = TempDir::new();
    let path = dir.0.join("busy.txt");
    std::fs::write(&path, "x").unwrap();
    let desktop = create_desktop_with_options(DesktopOptions {
        files: Some(FilesConfig {
            executor_tick_hook: Some(Arc::new(move || {
                let (flag, changed) = &*hook_hold;
                let mut state = flag.lock().unwrap();
                while state.0 {
                    state.1 = true;
                    state = changed.wait(state).unwrap();
                }
                state.1 = false;
            })),
            ..FilesConfig::default()
        }),
        ..Default::default()
    });
    assert_eq!(desktop.status, 0);
    let desktop = desktop.value;
    snapshot(desktop, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    let (_, _, owner) = open_and_attach(desktop, &path);

    struct Unhold(Arc<(Mutex<(bool, bool)>, Condvar)>);
    impl Drop for Unhold {
        fn drop(&mut self) {
            self.0.0.lock().unwrap().0 = false;
            self.0.1.notify_all();
        }
    }
    let _unhold = Unhold(hold.clone());
    hold.0.lock().unwrap().0 = true;
    // Do not flood until the executor is provably parked inside the hook.
    let parked = Instant::now() + Duration::from_secs(60);
    while !hold.0.lock().unwrap().1 {
        assert!(
            Instant::now() < parked,
            "the executor never reached the hook"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let command = br#"{"version":1,"request_id":7,"payload":{"command":"attention","action":{"operation":"app_active","active":true}}}"#;
    let mut flooded = false;
    for _ in 0..200 {
        // SAFETY: the fixed request storage is valid through the call.
        let result = unsafe { zmux_desktop_dispatch(desktop, command.as_ptr(), command.len()) };
        if result.status == BUSY {
            flooded = true;
            break;
        }
        assert_eq!(result.status, 0);
        assert_eq!(zmux_buffer_release(result.value), 0);
    }
    assert!(flooded, "the bounded command queue never filled");

    let (reply, _) = call(
        desktop,
        &json!({"op":"observe","owner":owner,"dirty":true}),
        &[],
    );
    assert_eq!(
        reply["ok"], false,
        "a refused observe must never read as ok"
    );
    assert_eq!(reply["error"]["code"], "busy");
    let held_snapshot = snapshot(desktop, |_| true);
    assert!(
        held_snapshot["model"]["workspaces"][0]["root"]["pane"]["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["file"]["dirty"] != true)
    );

    // Once the executor drains, the same idempotent write is retried and sticks.
    hold.0.lock().unwrap().0 = false;
    hold.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (retry, _) = call(
            desktop,
            &json!({"op":"observe","owner":owner,"dirty":true}),
            &[],
        );
        if retry["ok"] == true {
            break;
        }
        assert_eq!(retry["error"]["code"], "busy");
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    snapshot(desktop, |v| {
        v["model"]["workspaces"][0]["root"]["pane"]["surfaces"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["file"]["dirty"] == true))
    });
    let (clean, _) = call(
        desktop,
        &json!({"op":"observe","owner":owner,"dirty":false}),
        &[],
    );
    assert_eq!(clean["ok"], true);
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

/// A desktop whose file waits are far longer than any plausible scheduling delay, so a test of
/// maximum-size payloads depends on the data and the caps, never on how fast this machine is.
fn patient_desktop() -> u64 {
    let desktop = create_desktop_with_options(DesktopOptions {
        files: Some(FilesConfig {
            probe_timeout: Duration::from_secs(60),
            read_timeout: Duration::from_secs(60),
            save_timeout: Duration::from_secs(60),
            command_timeout: Duration::from_secs(60),
            stall_after: Duration::from_secs(120),
            ..FilesConfig::default()
        }),
        ..Default::default()
    });
    assert_eq!(desktop.status, 0);
    snapshot(desktop.value, |v| {
        !v["model"]["workspaces"].as_array().unwrap().is_empty()
    });
    desktop.value
}

#[test]
fn the_largest_legal_preview_attaches_through_the_bridge_and_one_more_byte_does_not_open() {
    let _serial = serial();
    const MAX_PREVIEW: usize = 7_340_032;
    let dir = TempDir::new();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    // A recognisable, non-repeating body so a truncated or reordered reply cannot compare equal.
    png.extend((0..MAX_PREVIEW - 8).map(|index| (index % 251) as u8));
    assert_eq!(png.len(), MAX_PREVIEW);
    let path = dir.0.join("largest.png");
    std::fs::write(&path, &png).unwrap();

    let desktop = patient_desktop();
    let (attached, body, owner) = open_and_attach(desktop, &path);
    assert_eq!(attached["mode"], "image");
    assert_eq!(attached["size"], MAX_PREVIEW);
    assert_eq!(attached["writable"], false);
    assert_eq!(
        body.len(),
        MAX_PREVIEW,
        "the whole 7 MiB preview is the raw reply body"
    );
    assert!(body == png, "the preview bytes arrive unchanged");
    // The reply (4 + header + 7 MiB) fit the shared buffer cap; a preview is never saved.
    let (refused, _) = call(
        desktop,
        &json!({"op":"save","owner":owner,"base_generation":1,"force":true}),
        b"text",
    );
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["error"]["code"], "invalid_request");
    assert_eq!(std::fs::read(&path).unwrap(), png);

    // One byte over the cap is refused when it is opened: no tab, a per-item too_large.
    png.push(0);
    let over = dir.0.join("over.png");
    std::fs::write(&over, &png).unwrap();
    let (opened, _) = call(
        desktop,
        &json!({"op":"open","items":[{"kind":"path","value":over}],"focus":true}),
        &[],
    );
    assert_eq!(opened["result"]["items"][0]["ok"], false, "{opened}");
    assert_eq!(opened["result"]["items"][0]["error"]["code"], "too_large");
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}

#[test]
fn a_two_mib_save_in_a_frame_exactly_at_the_cap_is_written_and_one_byte_more_is_refused() {
    let _serial = serial();
    const BODY: usize = 2_097_152;
    let dir = TempDir::new();
    let path = dir.0.join("cap.txt");
    std::fs::write(&path, "before").unwrap();
    let desktop = patient_desktop();
    let (_, _, owner) = open_and_attach(desktop, &path);

    // Pad the header with JSON whitespace so 4 + header + body is exactly the frame cap.
    let header = json!({"op":"save","owner":owner,"base_generation":1,"force":false});
    let build = |extra: usize| {
        let mut bytes = serde_json::to_vec(&header).unwrap();
        bytes.resize(MAX_FILE_SAVE_FRAME - BODY - 4 + extra, b' ');
        let mut out = (bytes.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&bytes);
        out
    };
    let body: Vec<u8> = (0..BODY).map(|i| b'a' + (i % 26) as u8).collect();

    // One byte past the cap (header 16381): refused as a malformed frame, nothing written.
    let mut oversize = build(1);
    oversize.extend_from_slice(&body);
    assert_eq!(oversize.len(), MAX_FILE_SAVE_FRAME + 1);
    assert_eq!(call_raw(desktop, &oversize).status, INVALID_ARGUMENT);
    assert_eq!(std::fs::read(&path).unwrap(), b"before");

    // Exactly at the cap (header 16380, body exactly 2 MiB): accepted and written whole.
    let mut exact = build(0);
    exact.extend_from_slice(&body);
    assert_eq!(exact.len(), MAX_FILE_SAVE_FRAME);
    let result = call_raw(desktop, &exact);
    assert_eq!(result.status, 0);
    let (saved, _) = split(&bytes_of(result.value));
    assert_eq!(saved["ok"], true, "{saved}");
    assert_eq!(saved["size"], BODY);
    assert_eq!(saved["generation"], 2);
    assert!(std::fs::read(&path).unwrap() == body);

    // The file just written is the largest one that can be read back, and it can be saved again.
    let (reloaded, again) = call(
        desktop,
        &json!({"op":"reload","owner":owner,"discard":false}),
        &[],
    );
    assert_eq!(reloaded["ok"], true, "{reloaded}");
    assert!(again == body);
    assert_eq!(zmux_desktop_destroy(desktop), 0);
}
