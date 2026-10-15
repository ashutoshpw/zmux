use zmux_terminal_adapter::{Engine, EngineError, EngineKind, Size};

fn size() -> Size {
    Size { rows: 3, cols: 8 }
}

#[test]
fn default_backend_stages_zero_reply_frames_and_abort_preserves_last_publication() {
    let mut engine = Engine::new(size(), EngineKind::default()).unwrap();
    let batch = engine.stage_process(b"first").unwrap();
    assert!(batch.replies().is_empty());
    assert_eq!(engine.viewport().contents, "");
    assert!(matches!(
        engine.stage_process(b"later"),
        Err(EngineError::Pending)
    ));
    assert!(engine.commit(batch.generation() + 1).is_err());
    engine.commit(batch.generation()).unwrap();
    assert_eq!(engine.viewport().contents, "first");
    let batch = engine.stage_process(b"\rsecond").unwrap();
    engine.abort();
    engine.abort();
    assert_eq!(engine.viewport().contents, "first");
    assert!(engine.commit(batch.generation()).is_err());
    assert!(engine.stage_scroll(1).is_err());
}

#[cfg(feature = "ghostty")]
fn ghostty() -> Engine {
    use zmux_terminal_adapter::GhosttyLimits;
    // Fixture policy only, deliberately no product default or RSS claim.
    let bytes = 16 * 1024 * 1024;
    Engine::new(
        size(),
        EngineKind::Ghostty(GhosttyLimits {
            engine_bytes: bytes,
            general_bytes: bytes,
            page_bytes: bytes,
        }),
    )
    .unwrap()
}

#[cfg(feature = "ghostty")]
#[test]
fn linked_engine_publishes_cells_modes_metadata_and_reply_as_one_exact_generation() {
    let mut engine = ghostty();
    let batch = engine.stage_process("界e\u{301}\x1b]0;title;rest\x07\x1b]7;file://host/a%20b;c\x07\x1b]777;notify;hello;body;rest\x07\x1b[?1h\x1b[?2004h\x1b[6n".as_bytes()).unwrap();
    assert_eq!(batch.replies(), b"\x1b[1;4R");
    assert_eq!(engine.viewport().contents, "");
    assert_eq!(engine.viewport().title, "");
    assert!(engine.viewport().notices.is_empty());
    assert!(matches!(
        engine.stage_resize(Size { rows: 2, cols: 6 }),
        Err(EngineError::Pending)
    ));
    assert!(engine.commit(batch.generation() + 1).is_err());
    // This direct adapter test stands in for an already completed physical write;
    // worker PTY ordering/backpressure acceptance remains a separate integration.
    engine.commit(batch.generation()).unwrap();
    let retained = engine.viewport().clone();
    assert_eq!(retained.contents, "界e\u{301}");
    assert_eq!(retained.lines[0][0].width, 2);
    assert_eq!(retained.title, "title;rest");
    assert_eq!(retained.working_directory.as_deref(), Some("/a b;c"));
    assert_eq!(retained.notices.len(), 1);
    assert_eq!(retained.notices[0].body, "body;rest");
    assert!(retained.application_cursor && retained.bracketed_paste);
    let batch = engine.stage_resize(Size { rows: 2, cols: 6 }).unwrap();
    assert_eq!(engine.viewport().size, size());
    engine.commit(batch.generation()).unwrap();
    assert_eq!(engine.viewport().size, Size { rows: 2, cols: 6 });
    drop(engine);
    assert_eq!(retained.contents, "界e\u{301}");
    assert_eq!(retained.title, "title;rest");
}

#[cfg(feature = "ghostty")]
#[test]
fn linked_engine_failure_and_partial_reply_cancellation_keep_published_metadata() {
    let mut engine = ghostty();
    let first = engine
        .stage_process(b"good\x1b]2;published\x07\x07")
        .unwrap();
    engine.commit(first.generation()).unwrap();
    let pending = engine
        .stage_process(b"\x1b]2;unpublished\x07\x07\x1b[6n")
        .unwrap();
    assert!(!pending.replies().is_empty());
    // A caller observing only a partial PTY reply must abort, never commit/replay.
    engine.abort();
    assert_eq!(engine.viewport().title, "published");
    assert_eq!(engine.viewport().notices.len(), 1);
    assert!(engine.commit(pending.generation()).is_err());
    assert!(engine.stage_process(b"later").is_err());

    let mut engine = ghostty();
    let first = engine.stage_process(b"good").unwrap();
    engine.commit(first.generation()).unwrap();
    // Public Viewport cannot represent strikethrough: fail visibly, no silent loss.
    assert!(engine.stage_process(b"\x1b[9munsupported").is_err());
    assert_eq!(engine.viewport().contents, "good");
    assert!(engine.stage_process(b"\x1b[0mrecovery").is_err());
}

#[cfg(feature = "ghostty")]
#[test]
fn linked_engine_exact_history_scroll_and_alternate_remain_bounded() {
    let mut engine = ghostty();
    for _ in 0..1100 {
        let batch = engine.stage_process(b"line\r\n").unwrap();
        engine.commit(batch.generation()).unwrap();
    }
    assert_eq!(engine.viewport().scrollback_lines, 1000);
    let batch = engine.stage_scroll(5).unwrap();
    engine.commit(batch.generation()).unwrap();
    assert_eq!(engine.viewport().scrollback_offset, 5);
    let batch = engine.stage_process(b"\x1b[?1049hnew").unwrap();
    engine.commit(batch.generation()).unwrap();
    assert!(engine.viewport().alternate_screen);
    assert_eq!(engine.viewport().scrollback_lines, 0);
    let batch = engine.stage_process(b"\x1b[?1049l").unwrap();
    engine.commit(batch.generation()).unwrap();
    assert!(!engine.viewport().alternate_screen);
    assert_eq!(engine.viewport().scrollback_lines, 1000);
}
