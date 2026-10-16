//! Native hosts must explicitly configure the same retained local owner as Desktop.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    thread,
    time::{Duration, Instant},
};
use zmux_native_bridge::*;

fn consume(id: u64) -> Value {
    let length = zmux_buffer_length(id);
    assert_eq!(length.status, 0);
    let mut data = vec![0; length.value as usize];
    // SAFETY: the owned destination remains writable for its declared length.
    assert_eq!(
        unsafe { zmux_buffer_copy(id, data.as_mut_ptr(), data.len()) },
        0
    );
    assert_eq!(zmux_buffer_release(id), 0);
    serde_json::from_slice(&data).unwrap()
}
fn snapshot(desktop: u64, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = zmux_desktop_snapshot(desktop, 0);
        assert!(result.status == 0 || result.status == -4);
        if result.status == 0 && result.value != 0 {
            let value = consume(result.value);
            if ready(&value) {
                return value;
            }
        }
        assert!(
            Instant::now() < deadline,
            "native local owner snapshot deadline"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn configured(value: Value) -> ZmuxResult {
    let data = serde_json::to_vec(&value).unwrap();
    // SAFETY: immutable JSON storage remains readable throughout the call.
    unsafe { zmux_desktop_create_configured(data.as_ptr(), data.len()) }
}
fn start(desktop: u64, pane: &Value, profile: uuid::Uuid, cwd: &Path) -> Value {
    let data = serde_json::to_vec(&json!({"version":1,"request_id":1,"payload":{
        "command":"local_zellij","request":{"operation":"start","profile_id":profile,
        "pane_id":pane,"name":"native-configured","cwd":cwd,"command":"exit 99","focus":true}}}))
    .unwrap();
    // SAFETY: immutable JSON storage remains readable throughout the call.
    let result = unsafe { zmux_desktop_dispatch(desktop, data.as_ptr(), data.len()) };
    assert_eq!(result.status, 0);
    consume(result.value)
}

#[test]
fn configured_foreign_owner_is_explicit_and_refuses_a_noncapable_dependency_before_reservation() {
    let root = std::env::temp_dir().join(format!("zmux-native-ready-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let state = root.join("state");
    let profile_root = root.join("profile");
    let probe = root.join("unsupported-probe");
    let queried = root.join("capability-queried");
    let forbidden = root.join("backend-must-not-launch");
    // A capability-only negative fixture is never an owner or Ready producer.
    fs::write(&probe, format!("#!/bin/sh\ncase \"$1\" in\n --version) printf 'zellij 0.43.0\\n';;\n --zmux-input-readiness-protocol) printf x > '{}'; exit 2;;\n *) printf x > '{}'; exit 99;;\nesac\n",queried.display(),forbidden.display())).unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o700)).unwrap();
    let profile = uuid::Uuid::new_v4();
    let local = json!({"installation":{"epoch":uuid::Uuid::new_v4(),"cli":"/bin/true","zellij":probe},
                       "profiles":[{"id":profile,"root":profile_root}]});
    let desktop = configured(json!({"state_directory":state,"local_zellij":local}));
    assert_eq!(
        desktop.status, 0,
        "explicit typed configuration must reach the real native owner"
    );
    let initial = snapshot(desktop.value, |s| {
        s["persistence"]["startup_pending"] == false
            && s["pending_launches"] == 0
            && s["model"]["workspaces"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
    });
    assert_eq!(initial["local_zellij_operations"], json!([]));
    let pane = &initial["model"]["workspaces"][0]["focused_pane"];
    assert_eq!(start(desktop.value, pane, profile, &root)["ok"], true);
    let failed = snapshot(desktop.value, |s| {
        s["local_zellij_operations"]
            .as_array()
            .and_then(|v| v.last())
            .is_some_and(|row| row["session_joined"] == true)
    });
    let receipt = failed["local_zellij_operations"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(receipt["profile_id"], profile.to_string());
    assert_eq!(receipt["input_ready"], false);
    assert!(receipt["owner_uuid"].is_null() && receipt["checkpoint_revision"].is_null());
    assert!(receipt["error"].is_string());
    assert_eq!(receipt["monitor_joined"], true);
    assert_eq!(receipt["namespace_cleaned"], true);
    assert_eq!(fs::read(&queried).unwrap(), b"x");
    assert!(!forbidden.exists());
    assert_eq!(zmux_desktop_destroy(desktop.value), 0);

    let ordinary = configured(json!({"state_directory":root.join("ordinary-state")}));
    assert_eq!(ordinary.status, 0);
    let initial = snapshot(ordinary.value, |s| {
        s["persistence"]["startup_pending"] == false
            && s["model"]["workspaces"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
    });
    let pane = &initial["model"]["workspaces"][0]["focused_pane"];
    // The foreign boundary acknowledges queue admission; the serialized owner
    // reports semantic refusal asynchronously in its production snapshot.
    assert_eq!(start(ordinary.value, pane, profile, &root)["ok"], true);
    let refused = snapshot(ordinary.value, |s| s["last_request_id"] == 1);
    assert!(
        refused["last_error"]
            .as_str()
            .unwrap()
            .contains("configured")
    );
    assert_eq!(refused["local_zellij_operations"], json!([]));
    assert_eq!(zmux_desktop_destroy(ordinary.value), 0);
    fs::remove_dir_all(root).unwrap();
}
