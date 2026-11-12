#[cfg(feature = "ghostty")]
use zmux_program_status::SessionEpoch;
use zmux_program_status::{FEATURE_DETECTION_REPLY, Kind, State, TransitionCause};
use zmux_terminal_adapter::{Engine, EngineError, EngineKind, Size, Terminal, TerminalError};

fn size() -> Size {
    Size { rows: 4, cols: 40 }
}

fn state(viewport: &zmux_terminal_adapter::Viewport, id: &str) -> Option<State> {
    viewport
        .program_status
        .records
        .iter()
        .find(|record| record.id.as_ref() == id)
        .map(|record| record.state)
}

#[test]
fn reports_survive_byte_fragmentation_and_keep_visible_text_and_session_identity() {
    let mut terminal = Terminal::new(size()).unwrap();
    let report =
        b"before\x1b]7501;state=working:app=zed:id=build:progress=38:msg=SGVsbG8=\x1b\\after";
    for byte in report {
        terminal.process(std::slice::from_ref(byte)).unwrap();
    }

    let viewport = terminal.viewport();
    let record = viewport
        .program_status
        .records
        .iter()
        .find(|record| record.id.as_ref() == "build")
        .unwrap();
    assert_eq!(record.state, State::Working);
    assert_eq!(record.progress, Some(38));
    assert_eq!(record.app.as_deref(), Some("zed"));
    assert_eq!(record.msg.as_deref(), Some("Hello"));
    assert_eq!(viewport.contents, "beforeafter");
    assert_eq!(viewport.program_status.session_epoch.0, 0);
    let encoded = serde_json::to_value(&viewport).unwrap();
    assert!(encoded.get("program_status").is_none());
}

#[test]
fn prompt_ris_and_decstr_are_applied_in_wire_order() {
    let mut terminal = Terminal::new(size()).unwrap();
    terminal
        .process(
            b"\x1b]7501;state=working:id=first\x07\x1b]7501;state=done:id=kept\x1b\\\x1b]133;A\x07\x1b]7501;state=blocked:kind=permission:id=second\x07",
        )
        .unwrap();
    let prompted = terminal.viewport();
    assert_eq!(state(&prompted, "first"), None);
    assert_eq!(state(&prompted, "kept"), Some(State::Done));
    assert_eq!(state(&prompted, "second"), Some(State::Blocked));
    assert_eq!(
        prompted
            .program_status
            .records
            .iter()
            .find(|record| record.id.as_ref() == "second")
            .unwrap()
            .kind,
        Some(Kind::Permission)
    );
    assert!(
        prompted
            .program_status
            .transitions
            .iter()
            .any(|event| event.cause == TransitionCause::Prompt && event.id.as_ref() == "first")
    );

    terminal.process(b"\x1b[!p").unwrap();
    assert_eq!(state(&terminal.viewport(), "kept"), Some(State::Done));
    assert_eq!(state(&terminal.viewport(), "second"), Some(State::Blocked));

    terminal.process(b"\x1bc").unwrap();
    assert!(terminal.viewport().program_status.records.is_empty());
}

#[test]
fn engine_holds_staged_status_and_orders_query_reply_before_da() {
    let mut engine = Engine::new(size(), EngineKind::default()).unwrap();
    let report = engine
        .stage_process(b"\x1b]7501;state=working:id=compile\x07")
        .unwrap();
    assert!(report.replies().is_empty());
    assert!(engine.viewport().program_status.records.is_empty());
    engine.commit(report.generation()).unwrap();
    assert_eq!(state(engine.viewport(), "compile"), Some(State::Working));

    let query = engine.stage_process(b"\x1b]7501;?\x1b\\\x1b[c").unwrap();
    let mut expected = FEATURE_DETECTION_REPLY.to_vec();
    expected.extend_from_slice(b"\x1b[?62c");
    assert_eq!(query.replies(), expected);
    engine.abort();
    assert_eq!(state(engine.viewport(), "compile"), Some(State::Working));
}

#[test]
fn deferred_default_backend_exports_once_at_publication() {
    let mut engine = Engine::new(size(), EngineKind::default()).unwrap();
    let batch = engine
        .stage_process_deferred(b"\x1b]7501;state=working:id=compile\x07")
        .unwrap();
    engine.commit(batch.generation()).unwrap();
    assert!(engine.viewport().program_status.records.is_empty());
    engine.refresh_viewport().unwrap();
    assert_eq!(state(engine.viewport(), "compile"), Some(State::Working));
}

#[test]
fn causal_reply_overload_keeps_previous_publication_and_requires_recovery() {
    const MAX_READ_BYTES: usize = 4096;
    const QUERY: &[u8] = b"\x1b]7501;?\x07";

    let mut engine = Engine::new(size(), EngineKind::default()).unwrap();
    let initial = engine
        .stage_process(b"\x1b]7501;state=working:id=retained\x07")
        .unwrap();
    engine.commit(initial.generation()).unwrap();
    assert_eq!(state(engine.viewport(), "retained"), Some(State::Working));

    let update = b"\x1b]7501;state=done:id=retained\x07";
    let query_count = (MAX_READ_BYTES - update.len()) / QUERY.len();
    assert!(FEATURE_DETECTION_REPLY.len() * query_count > 8 * 1024);
    let mut input = Vec::with_capacity(update.len() + query_count * QUERY.len());
    input.extend_from_slice(update);
    for _ in 0..query_count {
        input.extend_from_slice(QUERY);
    }
    assert!(input.len() <= MAX_READ_BYTES);

    assert!(matches!(
        engine.stage_process_deferred(&input),
        Err(EngineError::Terminal(TerminalError::CausalReplyOverflow))
    ));
    assert_eq!(state(engine.viewport(), "retained"), Some(State::Working));
    assert!(matches!(
        engine.stage_process_deferred(b"\x1b]7501;state=done:id=retained\x07"),
        Err(EngineError::Failed)
    ));
}

#[test]
fn overlong_report_fragmented_across_worker_reads_is_rejected_atomically() {
    let mut terminal = Terminal::new(size()).unwrap();
    terminal
        .process(b"\x1b]7501;state=working:id=compile\x07")
        .unwrap();
    let mut oversized = b"\x1b]7501;state=done:id=compile:unknown=".to_vec();
    oversized.extend(std::iter::repeat_n(b'x', 4_100));
    oversized.push(0x07);
    terminal.process(&oversized[..4096]).unwrap();
    terminal.process(&oversized[4096..]).unwrap();
    assert_eq!(state(&terminal.viewport(), "compile"), Some(State::Working));
}

#[cfg(feature = "ghostty")]
#[test]
fn linked_engine_stages_raw_reports_query_prompt_ris_child_exit_and_close() {
    use zmux_terminal_adapter::GhosttyLimits;

    let bytes = 16 * 1024 * 1024;
    let mut engine = Engine::new_with_session_epoch(
        size(),
        EngineKind::Ghostty(GhosttyLimits {
            engine_bytes: bytes,
            general_bytes: bytes,
            page_bytes: bytes,
        }),
        None,
        SessionEpoch(73),
    )
    .unwrap();

    let report = engine
        .stage_process(
            b"\x1b]7501;state=working:id=fast\x1b]7501;state=done:id=fast\x1b\\\x1b]7501;?\x1b\\\x1b[c",
        )
        .unwrap();
    let mut expected_replies = FEATURE_DETECTION_REPLY.to_vec();
    expected_replies.extend_from_slice(b"\x1b[?62;22c");
    assert_eq!(report.replies(), expected_replies);
    assert!(engine.viewport().program_status.records.is_empty());
    engine.commit(report.generation()).unwrap();
    let committed = &engine.viewport().program_status;
    assert_eq!(committed.session_epoch, SessionEpoch(73));
    assert_eq!(state(engine.viewport(), "fast"), Some(State::Done));
    assert_eq!(committed.transitions.len(), 2);
    assert_eq!(
        committed.transitions[0]
            .after_record
            .as_ref()
            .unwrap()
            .state,
        State::Working
    );
    assert_eq!(
        committed.transitions[1]
            .after_record
            .as_ref()
            .unwrap()
            .state,
        State::Done
    );

    let lifetime = engine
        .stage_process(
            b"\x1b]7501;state=working:id=active\x1b]7501;state=blocked:kind=question:id=waiting\x1b\\\x1b]133;A\x07",
        )
        .unwrap();
    engine.commit(lifetime.generation()).unwrap();
    assert_eq!(state(engine.viewport(), "active"), None);
    assert_eq!(state(engine.viewport(), "waiting"), None);
    assert_eq!(state(engine.viewport(), "fast"), Some(State::Done));
    assert_eq!(engine.viewport().program_status.command_epoch.0, 1);

    let reset = engine
        .stage_process(b"\x1b]7501;state=working:id=reset\x1b[!p")
        .unwrap();
    engine.commit(reset.generation()).unwrap();
    assert_eq!(state(engine.viewport(), "reset"), Some(State::Working));

    let exit = engine.stage_attached_child_exited().unwrap();
    engine.commit(exit.generation()).unwrap();
    assert_eq!(state(engine.viewport(), "reset"), None);
    assert_eq!(state(engine.viewport(), "fast"), Some(State::Done));

    let ris = engine.stage_process(b"\x1bc").unwrap();
    engine.commit(ris.generation()).unwrap();
    assert!(engine.viewport().program_status.records.is_empty());
    assert!(
        engine
            .viewport()
            .program_status
            .transitions
            .iter()
            .any(|transition| transition.cause == TransitionCause::Ris)
    );

    let final_report = engine
        .stage_process(b"\x1b]7501;state=done:id=closed")
        .unwrap();
    engine.commit(final_report.generation()).unwrap();
    let closed = engine.terminal_closed_viewport().unwrap();
    assert!(closed.program_status.records.is_empty());
    assert!(
        closed
            .program_status
            .transitions
            .iter()
            .any(|transition| transition.cause == TransitionCause::TerminalClosed)
    );
}
