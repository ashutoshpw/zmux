//! Independent consumer of the native delta wire contract through the real ABI.
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};
use zmux_native_bridge::*;

struct Desktop(u64);
impl Drop for Desktop {
    fn drop(&mut self) {
        assert_eq!(zmux_desktop_destroy(self.0), 0);
    }
}
fn take(token: u64) -> Value {
    let size = zmux_buffer_length(token);
    assert_eq!(size.status, 0);
    let mut bytes = vec![0; size.value as usize];
    // SAFETY: the owned Vec has exactly the capacity requested by the ABI.
    assert_eq!(
        unsafe { zmux_buffer_copy(token, bytes.as_mut_ptr(), bytes.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(token), 0);
    serde_json::from_slice(&bytes).unwrap()
}
fn poll(id: u64, revision: u64) -> Option<Value> {
    let result = zmux_desktop_poll_frames(id, revision);
    if result.status == -4 || (result.status == 0 && result.value == 0) {
        return None;
    }
    assert_eq!(result.status, 0);
    Some(take(result.value))
}
fn wait(id: u64, revision: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(value) = poll(id, revision)
            && predicate(&value)
        {
            return value;
        }
        assert!(Instant::now() < end, "native frame contract did not settle");
        thread::sleep(Duration::from_millis(5));
    }
}
fn command(id: u64, number: u64, payload: Value) {
    let data =
        serde_json::to_vec(&json!({"version":1,"request_id":number,"payload":payload})).unwrap();
    // SAFETY: the immutable JSON bytes remain readable through dispatch.
    let result = unsafe { zmux_desktop_dispatch(id, data.as_ptr(), data.len()) };
    assert_eq!(result.status, 0);
    assert_eq!(take(result.value)["ok"], true);
}
fn packet(value: &Value, session: u64) -> &Value {
    value["frame_packets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["frame"]["session_id"] == session)
        .unwrap()
}

#[test]
fn native_poll_patches_only_changed_rows_preserves_unicode_and_recovers_unknown_baselines() {
    let created = zmux_desktop_create();
    assert_eq!(created.status, 0);
    let desktop = Desktop(created.value);
    let first = wait(desktop.0, u64::MAX, |v| {
        v["frame_packets"]
            .as_array()
            .is_some_and(|f| f.len() == 1 && f[0]["frame"]["snapshot"]["state"] == "running")
    });
    let pane = first["snapshot"]["model"]["workspaces"][0]["root"]["pane"]["id"].clone();
    command(
        desktop.0,
        1,
        json!({"command":"action","action":{"action":"split_pane","pane_id":pane,"axis":"horizontal"}}),
    );
    let split = wait(desktop.0, u64::MAX, |v| {
        v["frame_packets"].as_array().is_some_and(|f| {
            f.len() == 2
                && f.iter()
                    .all(|p| p["frame"]["snapshot"]["state"] == "running")
        })
    });
    let ids: Vec<u64> = split["frame_packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["frame"]["session_id"].as_u64().unwrap())
        .collect();
    let script = "stty -echo; printf '\\033[2J\\033[Hλ🙂BASE'; IFS= read -r first; printf '\\033[1;1Hμ🙂NEXT'; IFS= read -r second\n";
    command(
        desktop.0,
        2,
        json!({"command":"input","session_id":ids[0],"text":script}),
    );
    command(
        desktop.0,
        3,
        json!({"command":"input","session_id":ids[1],"text":"stty -echo; printf '\\033[2J\\033[HOTHER_READY'; IFS= read -r hold\n"}),
    );
    let baseline = wait(desktop.0, u64::MAX, |v| {
        packet(v, ids[0])["frame"]["snapshot"]["viewport"]["contents"]
            .as_str()
            .is_some_and(|s| s.starts_with("λ🙂BASE"))
            && packet(v, ids[1])["frame"]["snapshot"]["viewport"]["contents"]
                .as_str()
                .is_some_and(|s| s.starts_with("OTHER_READY"))
    });
    assert_eq!(baseline["schema_version"], 1);
    assert!(baseline["base_revision"].is_null());
    assert_eq!(baseline["snapshot"]["frames"], json!([]));
    let revision = baseline["snapshot"]["revision"].as_u64().unwrap();
    command(
        desktop.0,
        4,
        json!({"command":"input","session_id":ids[0],"text":"go\n"}),
    );
    let delta = wait(desktop.0, revision, |v| {
        packet(v, ids[0])["changed_rows"]
            .as_array()
            .is_some_and(|r| !r.is_empty())
    });
    assert_eq!(delta["base_revision"], revision);
    let stable = packet(&delta, ids[1]);
    assert_eq!(stable["unchanged"], true);
    assert!(
        stable["frame"]["snapshot"].is_null(),
        "unchanged pane retransmitted its grid"
    );
    let changed = packet(&delta, ids[0]);
    assert_eq!(changed["changed_rows"], json!([0]));
    let old = &packet(&baseline, ids[0])["frame"]["snapshot"]["viewport"];
    let patch = &changed["frame"]["snapshot"]["viewport"];
    let mut lines = old["lines"].as_array().unwrap().clone();
    for row in changed["changed_rows"].as_array().unwrap() {
        let row = row.as_u64().unwrap() as usize;
        lines[row] = patch["lines"][row].clone();
    }
    assert!(
        patch["lines"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .all(|r| r.as_array().unwrap().is_empty())
    );
    let mut contents = old["contents"].as_str().unwrap().to_string();
    let start = changed["contents_start"].as_u64().unwrap() as usize;
    let end = changed["contents_end"].as_u64().unwrap() as usize;
    assert!(contents.is_char_boundary(start) && contents.is_char_boundary(end));
    contents.replace_range(start..end, patch["contents"].as_str().unwrap());
    assert!(contents.starts_with("μ🙂NEXT"), "{contents}");
    // A consumer with an expired/unknown revision gets a full coherent baseline.
    let reset = wait(desktop.0, u64::MAX, |_| true);
    assert!(reset["base_revision"].is_null());
    let reset_frame = packet(&reset, ids[0]);
    assert_eq!(reset_frame["unchanged"], false);
    assert!(reset_frame["changed_rows"].is_null());
    assert_eq!(
        reset_frame["frame"]["snapshot"]["viewport"]["contents"],
        contents
    );
    assert_eq!(
        reset_frame["frame"]["snapshot"]["viewport"]["lines"],
        json!(lines)
    );
    // Full API readers retain their original independent snapshot contract.
    let full = zmux_desktop_snapshot(desktop.0, u64::MAX);
    assert_eq!(full.status, 0);
    let full = take(full.value);
    assert_eq!(full["frames"].as_array().unwrap().len(), 2);
    assert_eq!(
        full["frames"][0]["snapshot"]["viewport"]["contents"],
        contents
    );
}
