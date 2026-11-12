use std::collections::HashSet;

use data_encoding::{BASE64, BASE64_NOPAD};
use zmux_program_status::{
    CommandEpoch, FEATURE_DETECTION_REPLY, IgnoreReason, IngestOutcome, Kind, MAX_APP_BYTES,
    MAX_ID_BYTES, MAX_ID_DEPTH, MAX_ID_SEGMENT_BYTES, MAX_KEY_BYTES, MAX_MSG_DECODED_BYTES,
    MAX_MSG_ENCODED_BYTES, MAX_RECORDS, MAX_SEQUENCE_BYTES, MAX_TITLE_DECODED_BYTES,
    MAX_TITLE_ENCODED_BYTES, MAX_TRANSITIONS, ProgramStatusStore, RejectionReason, SessionEpoch,
    State, StoreCopyError, TransitionCause, TransitionCursor,
};

fn new_store() -> ProgramStatusStore {
    ProgramStatusStore::try_new(SessionEpoch(7)).expect("bounded owner allocation")
}

fn sequence_len(args: &[u8]) -> usize {
    // ESC ] 7501 ; is 7 bytes and ST is 2 bytes.
    args.len() + 9
}

fn ingest(store: &mut ProgramStatusStore, args: &str) -> IngestOutcome {
    ingest_bytes(store, args.as_bytes())
}

fn ingest_bytes(store: &mut ProgramStatusStore, args: &[u8]) -> IngestOutcome {
    store.ingest(args, sequence_len(args))
}

fn record<'a>(
    snapshot: &'a zmux_program_status::ProgramStatusSnapshot,
    id: &str,
) -> &'a zmux_program_status::Record {
    snapshot
        .records
        .iter()
        .find(|record| record.id.as_ref() == id)
        .unwrap_or_else(|| panic!("missing record {id:?}"))
}

fn encode_standard(value: &[u8]) -> String {
    BASE64.encode(value)
}

#[test]
fn p01_states_clear_query_free_reports_and_applicable_fields() {
    let mut store = new_store();

    let states = [
        ("idle", State::Idle),
        ("working", State::Working),
        ("done", State::Done),
        ("blocked", State::Blocked),
        ("error", State::Error),
    ];
    for (wire, expected) in states {
        assert!(matches!(
            ingest(&mut store, &format!("state={wire}")),
            IngestOutcome::Applied { .. }
        ));
        assert_eq!(record(&store.snapshot(None).unwrap(), "").state, expected);
    }

    assert!(matches!(
        ingest(&mut store, "state=blocked:kind=question:progress=0"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(record(&snapshot, "").kind, Some(Kind::Question));
    assert_eq!(record(&snapshot, "").progress, Some(0));

    assert!(matches!(
        ingest(&mut store, "state=working:kind=auth:progress=100"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(record(&snapshot, "").kind, None);
    assert_eq!(record(&snapshot, "").progress, Some(100));

    assert!(matches!(
        ingest(&mut store, "state=blocked:kind=future:progress=101"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(record(&snapshot, "").kind, None);
    assert_eq!(record(&snapshot, "").progress, None);

    assert_eq!(
        ingest(&mut store, "?"),
        IngestOutcome::Ignored(IgnoreReason::FeatureQuery)
    );
    assert_eq!(
        FEATURE_DETECTION_REPLY,
        b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\"
    );

    assert!(matches!(
        ingest(&mut store, "state=clear"),
        IngestOutcome::Applied {
            records_changed: true
        }
    ));
    assert!(store.snapshot(None).unwrap().records.is_empty());
}

#[test]
fn p02_pair_grammar_duplicates_and_optional_base64_padding() {
    let mut store = new_store();
    assert!(matches!(
        ingest(
            &mut store,
            " state = blocked : kind = permission : app = terraform : msg = YQ== : future=value : missing-equals "
        ),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    let root = record(&snapshot, "");
    assert_eq!(root.state, State::Blocked);
    assert_eq!(root.kind, Some(Kind::Permission));
    assert_eq!(root.app.as_deref(), Some("terraform"));
    assert_eq!(root.msg.as_deref(), Some("a"));

    assert!(matches!(
        ingest(&mut store, "state=working:msg=YQ"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "").msg.as_deref(),
        Some("a")
    );

    assert!(matches!(
        ingest(&mut store, "state=working:state=done"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "").state,
        State::Done
    );

    // The later syntactically valid app wins; app syntax validation then makes
    // its slash-containing identifier absent instead of retaining terraform.
    assert!(matches!(
        ingest(&mut store, "state=working:app=terraform:app=bad/name"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(record(&store.snapshot(None).unwrap(), "").app, None);

    // Likewise, an invalid last progress value is indeterminate, not 40.
    assert!(matches!(
        ingest(&mut store, "state=working:progress=40:progress=101"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(record(&store.snapshot(None).unwrap(), "").progress, None);

    let before = store.snapshot(None).unwrap();
    assert_eq!(
        ingest(&mut store, "state=waiting"),
        IngestOutcome::Ignored(IgnoreReason::MissingOrUnknownState)
    );
    assert_eq!(
        ingest(&mut store, "app=terraform"),
        IngestOutcome::Ignored(IgnoreReason::MissingOrUnknownState)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);
}

#[test]
fn p03_rejection_is_atomic_and_every_duplicate_text_value_is_validated() {
    let mut store = new_store();
    assert!(matches!(
        ingest(
            &mut store,
            "state=done:app=keep:msg=SGVsbG8=:title=UmVzdWx0"
        ),
        IngestOutcome::Applied { .. }
    ));
    let before = store.snapshot(None).unwrap();

    let invalid_text_reports = [
        (
            "state=working:app=other:msg=A:msg=YQ==",
            RejectionReason::InvalidBase64,
        ),
        (
            "state=working:app=other:msg=//4=:msg=YQ==",
            RejectionReason::InvalidUtf8,
        ),
        (
            "state=working:app=other:msg=Cg==:msg=YQ==",
            RejectionReason::ControlCharacter,
        ),
        (
            "state=working:app=other:title=CQ==:title=Yg==",
            RejectionReason::ControlCharacter,
        ),
        ("state=working:msg=fw==", RejectionReason::ControlCharacter),
        ("state=working:msg=woU=", RejectionReason::ControlCharacter),
        ("state=working:msg=YR==", RejectionReason::InvalidBase64),
    ];
    for (wire, expected) in invalid_text_reports {
        assert_eq!(
            ingest(&mut store, wire),
            IngestOutcome::Rejected(expected),
            "wire report {wire:?}"
        );
        assert_eq!(
            store.snapshot(None).unwrap(),
            before,
            "wire report {wire:?}"
        );
    }

    let too_many_encoded_bytes = BASE64.encode(&vec![b'x'; MAX_MSG_DECODED_BYTES + 2]);
    assert!(too_many_encoded_bytes.len() > MAX_MSG_ENCODED_BYTES);
    assert_eq!(
        ingest(
            &mut store,
            &format!("state=working:msg={too_many_encoded_bytes}")
        ),
        IngestOutcome::Rejected(RejectionReason::MsgEncodedTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    let app_too_long = "x".repeat(MAX_APP_BYTES + 1);
    assert_eq!(
        ingest(
            &mut store,
            &format!("state=working:app={app_too_long}:app=valid")
        ),
        IngestOutcome::Rejected(RejectionReason::AppTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    let too_many_message_bytes = BASE64_NOPAD.encode(&vec![b'x'; MAX_MSG_DECODED_BYTES + 1]);
    assert_eq!(too_many_message_bytes.len(), MAX_MSG_ENCODED_BYTES);
    assert_eq!(
        ingest(
            &mut store,
            &format!("state=working:msg={too_many_message_bytes}:msg=YQ==")
        ),
        IngestOutcome::Rejected(RejectionReason::MsgDecodedTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    let title_too_long = BASE64.encode(&[b'x'; MAX_TITLE_DECODED_BYTES + 1]);
    assert_eq!(
        ingest(
            &mut store,
            &format!("state=working:title={title_too_long}:title=YQ==")
        ),
        IngestOutcome::Rejected(RejectionReason::TitleEncodedTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    let long_key = "k".repeat(MAX_KEY_BYTES + 1);
    assert_eq!(
        ingest(&mut store, &format!("state=working:{long_key}=value")),
        IngestOutcome::Rejected(RejectionReason::KeyTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    let max_msg = encode_standard(&vec![b'x'; MAX_MSG_DECODED_BYTES]);
    assert_eq!(max_msg.len(), MAX_MSG_ENCODED_BYTES);
    assert!(matches!(
        ingest(&mut store, &format!("state=working:msg={max_msg}")),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "")
            .msg
            .as_ref()
            .unwrap()
            .len(),
        MAX_MSG_DECODED_BYTES
    );

    let max_title = encode_standard(&[b't'; MAX_TITLE_DECODED_BYTES]);
    assert_eq!(max_title.len(), MAX_TITLE_ENCODED_BYTES);
    assert!(matches!(
        ingest(&mut store, &format!("state=done:title={max_title}")),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "")
            .title
            .as_ref()
            .unwrap()
            .len(),
        MAX_TITLE_DECODED_BYTES
    );

    let max_app = "a".repeat(MAX_APP_BYTES);
    assert!(matches!(
        ingest(&mut store, &format!("state=idle:app={max_app}")),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "").app.as_deref(),
        Some(max_app.as_str())
    );
}

#[test]
fn p03_sequence_and_path_limits_are_hard_and_atomic() {
    let mut store = new_store();
    assert!(matches!(
        ingest(&mut store, "state=done:app=keep"),
        IngestOutcome::Applied { .. }
    ));
    let before = store.snapshot(None).unwrap();

    let mut exact_sequence = b"state=working:x=".to_vec();
    exact_sequence.resize(MAX_SEQUENCE_BYTES - 9, b'a');
    assert_eq!(
        store.ingest(&exact_sequence, MAX_SEQUENCE_BYTES),
        IngestOutcome::Applied {
            records_changed: true
        }
    );

    let too_long = vec![b'a'; MAX_SEQUENCE_BYTES - 8];
    assert_eq!(
        store.ingest(&too_long, MAX_SEQUENCE_BYTES + 1),
        IngestOutcome::Rejected(RejectionReason::SequenceTooLong)
    );
    assert_eq!(
        store.ingest(b"state=working", b"state=working".len() + 10),
        IngestOutcome::Rejected(RejectionReason::InvalidSequenceLength)
    );
    assert!(matches!(
        ingest(&mut store, "state=working:abcdefghijklmnop=value"),
        IngestOutcome::Applied { .. }
    ));

    let over_limit_total = [
        "a".repeat(32),
        "b".repeat(32),
        "c".repeat(32),
        "d".repeat(32),
    ]
    .join("/");
    assert!(over_limit_total.len() > MAX_ID_BYTES);
    let before_over_limit_id = store.snapshot(None).unwrap();
    assert_eq!(
        ingest(
            &mut store,
            &format!("state=working:id={over_limit_total}:id=valid")
        ),
        IngestOutcome::Rejected(RejectionReason::IdTooLong)
    );
    assert_eq!(store.snapshot(None).unwrap(), before_over_limit_id);

    let long_segment = "a".repeat(MAX_ID_SEGMENT_BYTES + 1);
    assert_eq!(
        ingest(&mut store, &format!("state=working:id={long_segment}")),
        IngestOutcome::Rejected(RejectionReason::IdSegmentTooLong)
    );

    let deep_id = std::iter::repeat_n("a", MAX_ID_DEPTH + 1)
        .collect::<Vec<_>>()
        .join("/");
    assert_eq!(
        ingest(&mut store, &format!("state=working:id={deep_id}")),
        IngestOutcome::Rejected(RejectionReason::IdTooDeep)
    );

    let max_id = [
        "a".repeat(25),
        "b".repeat(25),
        "c".repeat(25),
        "d".repeat(25),
        "e".repeat(24),
    ]
    .join("/");
    assert_eq!(max_id.len(), MAX_ID_BYTES);
    assert!(matches!(
        ingest(&mut store, &format!("state=idle:id={max_id}")),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(store.snapshot(None).unwrap().records.len(), 2);

    let max_segment = "z".repeat(MAX_ID_SEGMENT_BYTES);
    assert!(matches!(
        ingest(&mut store, &format!("state=idle:id={max_segment}")),
        IngestOutcome::Applied { .. }
    ));
    let max_depth = std::iter::repeat_n("d", MAX_ID_DEPTH)
        .collect::<Vec<_>>()
        .join("/");
    assert!(matches!(
        ingest(&mut store, &format!("state=idle:id={max_depth}")),
        IngestOutcome::Applied { .. }
    ));

    // The pre-existing root record stayed intact through all rejected inputs.
    assert_eq!(record(&before, "").state, State::Done);
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "").state,
        State::Working
    );
}

#[test]
fn p04_replacement_and_nearest_ancestor_app_are_dynamic() {
    let mut store = new_store();
    assert!(matches!(
        ingest(&mut store, "state=idle:app=root-app"),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=working:id=build:app=builder"),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(
            &mut store,
            "state=blocked:id=build/test:kind=auth:progress=40"
        ),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=working:id=orphan/child"),
        IngestOutcome::Applied { .. }
    ));

    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(
        record(&snapshot, "build/test").effective_app.as_deref(),
        Some("builder")
    );
    assert_eq!(
        record(&snapshot, "orphan/child").effective_app.as_deref(),
        Some("root-app")
    );

    // Replacement removes omitted fields and removes the parent's direct app.
    assert!(matches!(
        ingest(&mut store, "state=done:id=build"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(record(&snapshot, "build").app, None);
    assert_eq!(record(&snapshot, "build/test").kind, Some(Kind::Auth));
    assert_eq!(record(&snapshot, "build/test").progress, Some(40));
    assert_eq!(
        record(&snapshot, "build/test").effective_app.as_deref(),
        Some("root-app")
    );

    assert!(matches!(
        ingest(&mut store, "state=working"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "build/test").effective_app,
        None
    );

    // An ancestor eviction changes inherited projection without copying the
    // old app into the child record.
    let mut store = new_store();
    assert!(matches!(
        ingest(&mut store, "state=working:id=group:app=runner"),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=working:id=group/child"),
        IngestOutcome::Applied { .. }
    ));
    for index in 0..(MAX_RECORDS - 2) {
        assert!(matches!(
            ingest(&mut store, &format!("state=idle:id=filler/{index}")),
            IngestOutcome::Applied { .. }
        ));
    }
    assert_eq!(store.snapshot(None).unwrap().records.len(), MAX_RECORDS);
    assert!(matches!(
        ingest(&mut store, "state=done:id=new"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert!(
        snapshot
            .records
            .iter()
            .all(|record| record.id.as_ref() != "group")
    );
    assert_eq!(record(&snapshot, "group/child").effective_app, None);
}

#[test]
fn p05_clear_is_segment_aware_and_invalid_paths_do_not_fall_back_to_root() {
    let mut store = new_store();
    assert!(matches!(
        ingest(&mut store, "state=done:app=root"),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=working:id=build/test"),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=blocked:id=builder"),
        IngestOutcome::Applied { .. }
    ));

    assert!(matches!(
        ingest(&mut store, "state=clear:id=build"),
        IngestOutcome::Applied {
            records_changed: true
        }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert!(
        snapshot
            .records
            .iter()
            .all(|record| record.id.as_ref() != "build/test")
    );
    assert_eq!(record(&snapshot, "builder").state, State::Blocked);
    assert_eq!(record(&snapshot, "").state, State::Done);

    let before = snapshot.clone();
    assert_eq!(
        ingest(&mut store, "state=error:id=build/test:id=build//test"),
        IngestOutcome::Ignored(IgnoreReason::InvalidId)
    );
    assert_eq!(
        ingest(&mut store, "state=error:id="),
        IngestOutcome::Ignored(IgnoreReason::InvalidId)
    );
    assert_eq!(store.snapshot(None).unwrap(), before);

    // Malformed pairs are skipped generically: an earlier valid id remains the
    // selected path when a later id pair contains a forbidden value byte.
    assert!(matches!(
        ingest(&mut store, "state=working:id=build/test:id=bad@path"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&store.snapshot(None).unwrap(), "build/test").state,
        State::Working
    );

    // A malformed duplicate is skipped before applying id-specific limits,
    // including when its invalid value also exceeds a byte, segment or depth cap.
    for (invalid, state, expected) in [
        (format!("{}@", "a".repeat(128)), "blocked", State::Blocked),
        (format!("{}@", "a".repeat(32)), "error", State::Error),
        ("a/a/a/a/a/a/a/a/@".into(), "idle", State::Idle),
    ] {
        assert!(matches!(
            ingest(
                &mut store,
                &format!("state={state}:id=build/test:id={invalid}")
            ),
            IngestOutcome::Applied { .. }
        ));
        let snapshot = store.snapshot(None).unwrap();
        assert_eq!(record(&snapshot, "build/test").state, expected);
        assert_eq!(record(&snapshot, "").state, State::Done);
    }

    // Dot segments are legal literal protocol names. They are not normalized as
    // filesystem paths and do not alias the root record.
    assert!(matches!(
        ingest(&mut store, "state=idle:id=../worker"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(record(&snapshot, "../worker").state, State::Idle);
    assert_eq!(record(&snapshot, "").state, State::Done);

    assert!(matches!(
        ingest(&mut store, "state=clear"),
        IngestOutcome::Applied {
            records_changed: true
        }
    ));
    assert!(store.snapshot(None).unwrap().records.is_empty());
}

#[test]
fn p06_record_capacity_and_lru_eviction_are_bounded_and_deterministic() {
    // Protocol 0.4 requires support for at least 64 simultaneous records. Keep
    // this literal independent of the configured MAX_RECORDS constant so a
    // reduced implementation cap fails the public contract.
    let mut maximal = new_store();
    let max_title = BASE64.encode(&[b't'; MAX_TITLE_DECODED_BYTES]);
    let max_msg = BASE64.encode(&vec![b'm'; MAX_MSG_DECODED_BYTES]);
    assert_eq!(max_title.len(), MAX_TITLE_ENCODED_BYTES);
    assert_eq!(max_msg.len(), MAX_MSG_ENCODED_BYTES);
    for index in 0..64 {
        let first = format!("r{index:02}{}", "a".repeat(22));
        let id = [
            first,
            "b".repeat(25),
            "c".repeat(25),
            "d".repeat(25),
            "e".repeat(24),
        ]
        .join("/");
        let app = format!("app-{index:02}{}", "x".repeat(26));
        assert_eq!(id.len(), MAX_ID_BYTES);
        assert_eq!(app.len(), MAX_APP_BYTES);

        for state in ["working", "done"] {
            let wire = format!("state={state}:id={id}:app={app}:title={max_title}:msg={max_msg}");
            assert!(sequence_len(wire.as_bytes()) <= MAX_SEQUENCE_BYTES);
            assert!(matches!(
                ingest(&mut maximal, &wire),
                IngestOutcome::Applied { .. }
            ));
        }
    }
    let maximal_snapshot = maximal.snapshot(None).unwrap();
    assert_eq!(maximal_snapshot.records.len(), 64);
    assert_eq!(maximal_snapshot.transitions.len(), MAX_TRANSITIONS);
    let unique_ids = maximal_snapshot
        .records
        .iter()
        .map(|record| record.id.as_ref())
        .collect::<HashSet<_>>();
    assert_eq!(unique_ids.len(), 64);
    for record in &maximal_snapshot.records {
        assert_eq!(record.id.len(), MAX_ID_BYTES);
        assert_eq!(record.app.as_ref().unwrap().len(), MAX_APP_BYTES);
        assert_eq!(
            record.title.as_ref().unwrap().len(),
            MAX_TITLE_DECODED_BYTES
        );
        assert_eq!(record.msg.as_ref().unwrap().len(), MAX_MSG_DECODED_BYTES);
        assert_eq!(record.state, State::Done);
    }
    for transition in &maximal_snapshot.transitions {
        let record = transition.after_record.as_ref().unwrap();
        assert_eq!(record.id.len(), MAX_ID_BYTES);
        assert_eq!(record.app.as_ref().unwrap().len(), MAX_APP_BYTES);
        assert_eq!(
            record.title.as_ref().unwrap().len(),
            MAX_TITLE_DECODED_BYTES
        );
        assert_eq!(record.msg.as_ref().unwrap().len(), MAX_MSG_DECODED_BYTES);
    }

    let mut store = new_store();
    for index in 0..MAX_RECORDS {
        assert!(matches!(
            ingest(&mut store, &format!("state=working:id=job/{index}")),
            IngestOutcome::Applied { .. }
        ));
    }
    assert_eq!(store.snapshot(None).unwrap().records.len(), MAX_RECORDS);

    // An identical report is still a most-recently-updated record, but it does
    // not produce an attention transition.
    let before = store.snapshot(None).unwrap();
    assert_eq!(
        ingest(&mut store, "state=working:id=job/0"),
        IngestOutcome::Applied {
            records_changed: false
        }
    );
    assert_eq!(
        store.snapshot(None).unwrap().transitions,
        before.transitions
    );

    assert!(matches!(
        ingest(&mut store, "state=done:id=job/new"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(snapshot.records.len(), MAX_RECORDS);
    assert!(
        snapshot
            .records
            .iter()
            .any(|record| record.id.as_ref() == "job/0")
    );
    assert!(
        snapshot
            .records
            .iter()
            .all(|record| record.id.as_ref() != "job/1")
    );
    assert_eq!(
        snapshot.transitions.last().unwrap().cause,
        TransitionCause::Report
    );
    assert!(snapshot.transitions.iter().any(|transition| {
        transition.cause == TransitionCause::Eviction && transition.id.as_ref() == "job/1"
    }));
}

#[test]
fn p07_prompt_exit_ris_and_close_apply_distinct_lifetimes() {
    let mut store = new_store();
    for (id, state) in [
        ("run", "working"),
        ("wait", "blocked"),
        ("idle", "idle"),
        ("done", "done"),
        ("error", "error"),
    ] {
        assert!(matches!(
            ingest(&mut store, &format!("state={state}:id={id}")),
            IngestOutcome::Applied { .. }
        ));
    }

    assert_eq!(store.prompt_boundary().unwrap(), CommandEpoch(1));
    let snapshot = store.snapshot(None).unwrap();
    assert_eq!(snapshot.command_epoch, CommandEpoch(1));
    assert!(
        snapshot
            .records
            .iter()
            .all(|record| !matches!(record.id.as_ref(), "run" | "wait"))
    );
    assert_eq!(record(&snapshot, "idle").state, State::Idle);
    assert_eq!(record(&snapshot, "done").state, State::Done);
    assert_eq!(record(&snapshot, "error").state, State::Error);
    assert!(
        snapshot
            .transitions
            .iter()
            .filter(|event| event.cause == TransitionCause::Prompt)
            .all(|event| event.command_epoch == CommandEpoch(0))
    );

    assert!(matches!(
        ingest(&mut store, "state=working:id=child"),
        IngestOutcome::Applied { .. }
    ));
    store.attached_child_exited().unwrap();
    let snapshot = store.snapshot(None).unwrap();
    assert!(
        snapshot
            .records
            .iter()
            .all(|record| record.id.as_ref() != "child")
    );
    assert_eq!(record(&snapshot, "done").state, State::Done);
    assert_eq!(record(&snapshot, "error").state, State::Error);

    assert!(matches!(
        ingest(&mut store, "state=blocked:id=reset-target"),
        IngestOutcome::Applied { .. }
    ));
    // There is deliberately no timer/expiry method; snapshots taken without a
    // lifecycle input remain identical.
    let before = store.snapshot(None).unwrap();
    assert_eq!(store.snapshot(None).unwrap(), before);
    store.ris().unwrap();
    assert!(store.snapshot(None).unwrap().records.is_empty());

    assert!(matches!(
        ingest(&mut store, "state=done"),
        IngestOutcome::Applied { .. }
    ));
    store.terminal_closed().unwrap();
    assert!(store.snapshot(None).unwrap().records.is_empty());
    assert_eq!(
        ingest(&mut store, "state=working"),
        IngestOutcome::Ignored(IgnoreReason::TerminalClosed)
    );
}

#[test]
fn p08_transitions_keep_alert_content_order_and_report_dropped_cursor() {
    let mut store = new_store();
    assert!(matches!(
        ingest(
            &mut store,
            "state=working:id=build:app=cargo:title=QnVpbGQ=:msg=QnVpbGRpbmc="
        ),
        IngestOutcome::Applied { .. }
    ));
    let after_working = store.snapshot(None).unwrap();
    assert_eq!(
        ingest(
            &mut store,
            "state=working:id=build:app=cargo:title=QnVpbGQ=:msg=QnVpbGRpbmc="
        ),
        IngestOutcome::Applied {
            records_changed: false
        }
    );
    assert_eq!(
        store.snapshot(None).unwrap().transitions,
        after_working.transitions
    );

    assert!(matches!(
        ingest(
            &mut store,
            "state=working:id=build:app=cargo:progress=60:title=QnVpbGQ=:msg=U3RpbGwgYnVpbGRpbmc="
        ),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        store.snapshot(None).unwrap().transitions,
        after_working.transitions
    );

    // Simulate a coalesced terminal read: a completion followed by clear still
    // leaves an independent event payload for the consumer.
    assert!(matches!(
        ingest(
            &mut store,
            "state=done:id=build:title=UmVzdWx0cw==:msg=RmluaXNoZWQ="
        ),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut store, "state=clear:id=build"),
        IngestOutcome::Applied { .. }
    ));
    let snapshot = store.snapshot(Some(TransitionCursor(0))).unwrap();
    assert!(snapshot.records.is_empty());
    let completion = snapshot
        .transitions
        .iter()
        .find(|transition| {
            transition.cause == TransitionCause::Report
                && transition
                    .after_record
                    .as_ref()
                    .is_some_and(|record| record.state == State::Done)
        })
        .unwrap();
    assert_eq!(completion.before_state, Some(State::Working));
    let completed_record = completion.after_record.as_ref().unwrap();
    assert_eq!(completed_record.title.as_deref(), Some("Results"));
    assert_eq!(completed_record.msg.as_deref(), Some("Finished"));
    assert_eq!(completed_record.app, None);
    assert_eq!(
        snapshot.transitions.last().unwrap().cause,
        TransitionCause::Clear
    );
    assert_eq!(snapshot.transitions.last().unwrap().after_record, None);

    let frame = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(frame["session_epoch"], 7);
    assert_eq!(frame["command_epoch"], 0);
    assert_eq!(frame["records"], serde_json::json!([]));
    assert_eq!(frame["transitions"][1]["before_state"], "working");
    assert_eq!(frame["transitions"][1]["after_record"]["state"], "done");
    assert_eq!(frame["transitions"][1]["after_record"]["msg"], "Finished");

    let mut overflow = new_store();
    assert!(matches!(
        ingest(&mut overflow, "state=working"),
        IngestOutcome::Applied { .. }
    ));
    for index in 0..(MAX_TRANSITIONS + 5) {
        let state = if index % 2 == 0 { "done" } else { "working" };
        assert!(matches!(
            ingest(&mut overflow, &format!("state={state}")),
            IngestOutcome::Applied { .. }
        ));
    }
    let snapshot = overflow.snapshot(Some(TransitionCursor(0))).unwrap();
    assert_eq!(snapshot.transitions.len(), MAX_TRANSITIONS);
    assert_eq!(
        snapshot.dropped_through,
        Some(TransitionCursor(snapshot.cursor.0 - MAX_TRANSITIONS as u64))
    );
    assert!(snapshot.transitions[0].cursor > snapshot.dropped_through.unwrap());
    assert_eq!(record(&snapshot, "").state, State::Done);

    let mut staged = overflow.try_clone().unwrap();
    assert!(matches!(
        ingest(&mut staged, "state=idle"),
        IngestOutcome::Applied { .. }
    ));
    assert_eq!(
        record(&staged.snapshot(None).unwrap(), "").state,
        State::Idle
    );
    assert_eq!(
        record(&overflow.snapshot(None).unwrap(), "").state,
        State::Done
    );
}

#[test]
fn reusable_copy_replaces_state_and_rejects_a_different_session_epoch() {
    let mut source = new_store();
    assert!(matches!(
        ingest(&mut source, "id=agent-a:state=working:msg=ZnJvbS1zb3VyY2U="),
        IngestOutcome::Applied { .. }
    ));
    assert!(matches!(
        ingest(&mut source, "id=agent-a:state=blocked:msg=bmVlZHMtaW5wdXQ="),
        IngestOutcome::Applied { .. }
    ));
    for index in 0..(MAX_TRANSITIONS + 4) {
        let state = if index % 2 == 0 { "working" } else { "blocked" };
        assert!(matches!(
            ingest(&mut source, &format!("id=agent-a:state={state}")),
            IngestOutcome::Applied { .. }
        ));
    }

    let mut destination = new_store();
    assert!(matches!(
        ingest(&mut destination, "id=stale:state=working"),
        IngestOutcome::Applied { .. }
    ));
    destination.copy_from(&source).unwrap();
    assert_eq!(
        destination.snapshot(None).unwrap(),
        source.snapshot(None).unwrap()
    );

    assert!(matches!(
        ingest(&mut source, "id=agent-a:state=done"),
        IngestOutcome::Applied { .. }
    ));
    destination.copy_from(&source).unwrap();
    assert_eq!(
        destination.snapshot(None).unwrap(),
        source.snapshot(None).unwrap()
    );

    let before = destination.snapshot(None).unwrap();
    let other_session = ProgramStatusStore::try_new(SessionEpoch(8)).unwrap();
    assert_eq!(destination.copy_from(&other_session), Err(StoreCopyError));
    assert_eq!(destination.snapshot(None).unwrap(), before);
}
