//! Independent malformed-wire controls for the actual native consumer decoder.
use zmux_remote_runtime::local_zellij::observer_protocol::{FRAME_LIMIT, Frame};

#[test]
fn observer_decoder_refuses_ambiguous_or_extended_lifecycle_messages() {
    // Independent protocol bytes rather than a round trip through Frame's encoder.
    let valid = r#"{"version":1,"context":{"installation_epoch":"11111111-1111-4111-8111-111111111111","profile_id":"22222222-2222-4222-8222-222222222222","operation_id":1,"identity_namespace":"33333333-3333-4333-8333-333333333333","session_id":"44444444-4444-4444-8444-444444444444","surface_id":"55555555-5555-4555-8555-555555555555","launch_generation":1},"nonce":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sequence":1,"event":"owner_reserved","owner_uuid":"66666666-6666-4666-8666-666666666666"}"#;
    Frame::decode(valid.as_bytes()).unwrap();
    for malformed in [
        valid.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        valid.replacen(
            "\"operation_id\":1",
            "\"operation_id\":1,\"operation_id\":1",
            1,
        ),
        valid.replacen(
            "\"event\":\"owner_reserved\"",
            "\"event\":\"owner_reserved\",\"event\":\"owner_reserved\"",
            1,
        ),
        valid.replacen("\"version\":1", "\"extended\":true,\"version\":1", 1),
        valid.replacen(
            "\"operation_id\":1",
            "\"extended\":true,\"operation_id\":1",
            1,
        ),
        valid.replace("owner_reserved", "unknown_event"),
        valid.replace("\"sequence\":1,", ""),
        valid.replace("\"sequence\":1", "\"sequence\":\"1\""),
        "[]".into(),
        String::new(),
        " ".repeat(FRAME_LIMIT + 1),
    ] {
        assert!(
            Frame::decode(malformed.as_bytes()).is_err(),
            "accepted malformed wire: {malformed}"
        );
    }
}
