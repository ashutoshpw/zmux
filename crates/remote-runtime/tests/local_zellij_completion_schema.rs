//! Structured terminal status is a strict bounded protocol, never a command.
use zmux_remote_runtime::local_zellij::completion::WorkloadExit;
#[test]
fn terminal_exit_wire_rejects_unknown_or_ambiguous_fields() {
    for valid in [
        r#"{"kind":"exited","value":0}"#,
        r#"{"kind":"exited","value":7}"#,
        r#"{"kind":"signaled","value":15}"#,
    ] {
        serde_json::from_str::<WorkloadExit>(valid)
            .unwrap()
            .validate()
            .unwrap();
    }
    for malformed in [
        r#"{"kind":"exited","value":0,"command":"must never become authority"}"#,
        r#"{"kind":"signaled","value":15,"context":{}}"#,
        r#"{"kind":"exited","value":0,"value":7}"#,
        r#"{"kind":"exited","kind":"signaled","value":15}"#,
        r#"{"kind":"continued"}"#,
        r#"{"kind":"exited","value":256}"#,
        r#"{"kind":"signaled","value":0}"#,
    ] {
        assert!(
            serde_json::from_str::<WorkloadExit>(malformed)
                .and_then(|exit| exit.validate().map_err(serde_json::Error::io))
                .is_err(),
            "accepted malformed terminal exit: {malformed}"
        );
    }
}
