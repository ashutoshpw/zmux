use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    thread,
    time::{Duration, Instant},
};
use zmux_program_status::{FEATURE_DETECTION_REPLY, State as ProgramState, TransitionCause};
#[cfg(feature = "ghostty")]
use zmux_session_runtime::{EngineKind, GhosttyLimits};
use zmux_session_runtime::{
    EngineOptions, INPUT_CHUNK_BYTES, INPUT_QUEUE_CHUNKS, InputReceipt, InputWriteStatus, Launch,
    ProgramStatusCheckpoint, Session, SessionError, ShutdownPolicy, State,
};
use zmux_terminal_adapter::Size;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zmux-program-status-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn script(&self) -> PathBuf {
        self.0.join("terminal.py")
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn exited(session: &Session) -> std::sync::Arc<zmux_session_runtime::Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state == State::Exited => return snapshot,
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!("PTY session failed: {:?}", snapshot.failure)
            }
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("read PTY snapshot: {error}"),
        }
        assert!(Instant::now() < deadline, "PTY child did not exit");
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_for_output(session: &Session, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!(
                    "PTY session failed before {expected}: {:?}",
                    snapshot.failure
                )
            }
            Ok(snapshot) if snapshot.viewport.contents.contains(expected) => return,
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("read PTY snapshot: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "PTY child did not print {expected}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_for_file(path: &Path, description: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn admit_input_until_pty_backpressure(session: &Session) -> (usize, InputReceipt) {
    let operation_deadline = Instant::now() + Duration::from_secs(5);
    let receipt_deadline = Instant::now() + Duration::from_secs(15);
    for index in 0..INPUT_QUEUE_CHUNKS * 4 {
        let bytes = vec![b'A' + (index % 26) as u8; INPUT_CHUNK_BYTES];
        let mut receipt = session
            .send_input_with_receipt(&bytes, receipt_deadline)
            .unwrap_or_else(|error| panic!("admit tracked PTY input: {error}"));
        let observation_deadline =
            (Instant::now() + Duration::from_millis(250)).min(operation_deadline);
        loop {
            match receipt.try_recv() {
                Ok(result) => {
                    assert_eq!(result.status, InputWriteStatus::Complete);
                    assert_eq!(result.bytes_written, bytes.len());
                    assert_eq!(result.total_bytes, bytes.len());
                    assert_eq!(result.reason, None);
                    break;
                }
                Err(TryRecvError::Empty) if Instant::now() >= observation_deadline => {
                    assert!(
                        Instant::now() < operation_deadline,
                        "no tracked PTY write remained pending before the bounded input limit"
                    );
                    return (index + 1, receipt);
                }
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(2)),
                Err(TryRecvError::Disconnected) => {
                    panic!("tracked PTY input receipt disconnected")
                }
            }
        }
        assert!(
            Instant::now() < operation_deadline,
            "no tracked PTY write remained pending before the bounded input limit"
        );
    }
    panic!("child did not create observable PTY input backpressure");
}

fn request_checkpoint_with_bounded_busy_retry(
    session: &Session,
    deadline: Instant,
) -> ProgramStatusCheckpoint {
    let retry_deadline = Instant::now() + Duration::from_millis(200);
    loop {
        match session.request_program_status_checkpoint(deadline) {
            Ok(checkpoint) => return checkpoint,
            // Busy comes from the admission try_lock before the queue send, so
            // this response proves safe non-admission and permits a bounded retry.
            Err(SessionError::Busy) if Instant::now() < retry_deadline => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("admit worker-ordered status checkpoint: {error}"),
        }
    }
}

fn expect_checkpoint_timeout(checkpoint: &mut ProgramStatusCheckpoint) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match checkpoint.try_recv() {
            None => {}
            Some(Err(SessionError::Timeout)) => return,
            Some(Ok(snapshot)) => panic!(
                "checkpoint returned before admitted PTY input and its causal reply completed: {:?}",
                snapshot.viewport.program_status
            ),
            Some(Err(error)) => panic!("checkpoint failed before its deadline: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "checkpoint did not report its deadline"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn expect_cancelled_checkpoint(checkpoint: &mut ProgramStatusCheckpoint) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match checkpoint.try_recv() {
            None | Some(Err(SessionError::Timeout)) => {}
            Some(Err(SessionError::Cancelled)) => return,
            Some(Ok(snapshot)) => panic!(
                "cancelled checkpoint returned a successful snapshot: {:?}",
                snapshot.viewport.program_status
            ),
            Some(Err(error)) => panic!("cancelled checkpoint returned {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "worker did not retire cancelled checkpoint"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_for_input_result(receipt: &mut InputReceipt) -> zmux_session_runtime::InputWriteResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receipt.try_recv() {
            Ok(result) => return result,
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("tracked PTY input receipt disconnected"),
        }
        assert!(
            Instant::now() < deadline,
            "tracked PTY input did not complete after release"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn wait_for_checkpoint_snapshot(
    checkpoint: &mut ProgramStatusCheckpoint,
) -> std::sync::Arc<zmux_session_runtime::Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match checkpoint.try_recv() {
            Some(Ok(snapshot)) => return snapshot,
            Some(Err(error)) => panic!("live checkpoint failed: {error}"),
            None => {}
        }
        assert!(
            Instant::now() < deadline,
            "live checkpoint did not complete"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn default_real_pty_orders_query_before_da_and_retires_only_after_final_drain() {
    real_pty_orders_query_before_da_and_retires_only_after_final_drain(None, b"\x1b[?62c");
}

#[cfg(feature = "ghostty")]
#[test]
fn linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain() {
    let mapping_bytes = 16 * 1024 * 1024;
    let engine = EngineOptions {
        kind: EngineKind::Ghostty(GhosttyLimits {
            engine_bytes: mapping_bytes,
            general_bytes: mapping_bytes,
            page_bytes: mapping_bytes,
        }),
        reply_timeout: Duration::from_secs(2),
    };
    // This single hosted lane test covers linked query/final-drain ordering
    // and the explicit-close abort-before-terminal-closed path.
    real_pty_orders_query_before_da_and_retires_only_after_final_drain(
        Some(engine),
        b"\x1b[?62;22c",
    );
    run_explicit_session_close_clears_committed_program_records(Some(engine));
}

fn real_pty_orders_query_before_da_and_retires_only_after_final_drain(
    engine: Option<EngineOptions>,
    da_reply: &[u8],
) {
    let fixture = Fixture::new();
    let mut expected_reply = FEATURE_DETECTION_REPLY.to_vec();
    expected_reply.extend_from_slice(da_reply);
    let expected_reply_hex = expected_reply
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let script = format!(
        r#"import os,tty
tty.setraw(0)
payload=(b'\x1b]7501;state=working:id=active\x07'
         b'\x1b]7501;state=blocked:kind=question:id=waiting\x1b\\'
         b'\x1b]7501;state=done:id=finished\x07'
         b'\x1b]7501;?\x1b\\\x1b[c')
expected=bytes.fromhex('{expected_reply_hex}')
os.write(1,payload)
reply=b''
while len(reply)<len(expected):
    chunk=os.read(0,len(expected)-len(reply))
    if not chunk: raise SystemExit(81)
    reply+=chunk
if reply!=expected: raise SystemExit(82)
# These bytes are the child's last PTY write. The live and blocked records
# must retire only after this complete tail has been parsed at EOF.
os.write(1,b'\r\nFINAL-DRAIN\x1b]7501;state=working:id=last\x07')
"#
    );
    fs::write(fixture.script(), script).unwrap();

    let launch = Launch {
        program: "python3".into(),
        args: vec!["-u".into(), fixture.script().to_string_lossy().into_owned()],
        cwd: None,
        size: Size { rows: 8, cols: 80 },
    };
    let mut session = match engine {
        Some(engine) => {
            Session::spawn_with_engine(launch, &[], &[], ShutdownPolicy::Kill, engine, ())
        }
        None => Session::spawn(launch),
    }
    .unwrap();

    let snapshot = exited(&session);
    assert_eq!(snapshot.exit_code, Some(0));
    assert!(snapshot.viewport.contents.contains("FINAL-DRAIN"));
    let records = &snapshot.viewport.program_status.records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id.as_ref(), "finished");
    assert_eq!(records[0].state, ProgramState::Done);
    let transitions = &snapshot.viewport.program_status.transitions;
    let final_report = transitions
        .iter()
        .find(|event| event.id.as_ref() == "last" && event.cause == TransitionCause::Report)
        .expect("the final PTY write's OSC report was not parsed");
    assert_eq!(
        final_report
            .after_record
            .as_ref()
            .map(|record| record.state),
        Some(ProgramState::Working)
    );
    let retirement = transitions
        .iter()
        .find(|event| event.id.as_ref() == "last" && event.cause == TransitionCause::ChildExit)
        .expect("the final report was not retired after PTY drain");
    assert_eq!(retirement.before_state, Some(ProgramState::Working));
    assert!(retirement.after_record.is_none());
    assert!(final_report.cursor < retirement.cursor);

    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn default_real_pty_flushes_admitted_input_before_causal_query_replies() {
    let fixture = Fixture::new();
    let expected_reply = [FEATURE_DETECTION_REPLY, b"\x1b[?62c"].concat();
    let expected_reply_hex = expected_reply
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let script = format!(
        r#"import os,signal,threading,tty
tty.setraw(0)
ready=threading.Event()
signal.signal(signal.SIGUSR1,lambda *_: ready.set())
os.write(1,b'INPUT_READY')
if not ready.wait(5): raise SystemExit(81)
os.write(1,b'\x1b]7501;state=done:id=ordered\x07\x1b]7501;?\x1b\\\x1b[c')
expected_payload=b''.join(bytes([65+i%26])*{chunk_bytes} for i in range({chunks}))
expected_reply=bytes.fromhex('{expected_reply_hex}')
expected=expected_payload+expected_reply
actual=bytearray()
while len(actual)<len(expected):
    part=os.read(0,len(expected)-len(actual))
    if not part: raise SystemExit(82)
    actual.extend(part)
if actual!=expected: raise SystemExit(83)
os.write(1,b'INPUT_AND_REPLIES_ORDERED')
"#,
        chunk_bytes = INPUT_CHUNK_BYTES / 32,
        chunks = INPUT_QUEUE_CHUNKS,
        expected_reply_hex = expected_reply_hex,
    );
    fs::write(fixture.script(), &script).unwrap();

    let mut session = Session::spawn(Launch {
        program: "python3".into(),
        args: vec!["-u".into(), fixture.script().to_string_lossy().into_owned()],
        cwd: None,
        size: Size { rows: 8, cols: 80 },
    })
    .unwrap();
    wait_for_output(&session, "INPUT_READY");

    for index in 0..INPUT_QUEUE_CHUNKS {
        let bytes = vec![b'A' + (index % 26) as u8; INPUT_CHUNK_BYTES / 32];
        session
            .send_input(&bytes)
            .unwrap_or_else(|error| panic!("admit PTY input before query: {error}"));
    }

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(session.process_id() as i32),
        nix::sys::signal::Signal::SIGUSR1,
    )
    .unwrap();

    let snapshot = exited(&session);
    assert_eq!(snapshot.exit_code, Some(0));
    assert!(
        snapshot
            .viewport
            .contents
            .contains("INPUT_AND_REPLIES_ORDERED")
    );
    assert_eq!(
        snapshot
            .viewport
            .program_status
            .records
            .iter()
            .find(|record| record.id.as_ref() == "ordered")
            .map(|record| record.state),
        Some(ProgramState::Done)
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn default_real_pty_checkpoint_deadline_and_cancellation_retire_behind_input_and_replies() {
    let fixture = Fixture::new();
    let query_start = fixture.marker("start-query");
    let query_sent = fixture.marker("query-sent");
    let input_plan = fixture.marker("input-plan");
    let drain_start = fixture.marker("start-drain");
    let finish = fixture.marker("finish-child");
    let expected_reply = [FEATURE_DETECTION_REPLY, b"\x1b[?62c"].concat();
    let expected_reply_hex = expected_reply
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let script = format!(
        r#"import os,sys,time,tty
tty.setraw(0)
query_start,query_sent,input_plan,drain_start,finish=sys.argv[1:]
def wait_for(path):
    deadline=time.monotonic()+20
    while not os.path.exists(path):
        if time.monotonic()>=deadline: raise SystemExit(80)
        time.sleep(0.001)
def write_all(fd,data):
    view=memoryview(data)
    while view:
        view=view[os.write(fd,view):]
os.write(1,b'CHECKPOINT_READY')
wait_for(query_start)
chunks=int(open(input_plan,'r').read())
payload=(b'\x1b]133;A\x07'
         b'\x1b]7501;state=working:id=checkpoint-current\x07'
         b'\x1b]7501;?\x1b\\\x1b[c')
write_all(1,payload)
with open(query_sent,'wb') as receipt: receipt.write(b'sent')
expected_input=b''.join(bytes([65+i%26])*4096 for i in range(chunks))
expected_reply=bytes.fromhex('{expected_reply_hex}')
expected=expected_input+expected_reply
wait_for(drain_start)
actual=bytearray()
while len(actual)<len(expected):
    part=os.read(0,len(expected)-len(actual))
    if not part: raise SystemExit(81)
    actual.extend(part)
if actual!=expected: raise SystemExit(82)
write_all(1,b'\r\nCHECKPOINT_INPUT_AND_REPLIES_ORDERED')
wait_for(finish)
"#,
        expected_reply_hex = expected_reply_hex,
    );
    fs::write(fixture.script(), script).unwrap();

    let launch = Launch {
        program: "python3".into(),
        args: vec![
            "-u".into(),
            fixture.script().to_string_lossy().into_owned(),
            query_start.to_string_lossy().into_owned(),
            query_sent.to_string_lossy().into_owned(),
            input_plan.to_string_lossy().into_owned(),
            drain_start.to_string_lossy().into_owned(),
            finish.to_string_lossy().into_owned(),
        ],
        cwd: None,
        size: Size { rows: 8, cols: 80 },
    };
    let mut session = Session::spawn(launch).unwrap();
    wait_for_output(&session, "CHECKPOINT_READY");

    let (input_chunks, mut input_receipt) = admit_input_until_pty_backpressure(&session);
    fs::write(&input_plan, input_chunks.to_string()).unwrap();
    fs::write(&query_start, b"emit query").unwrap();
    wait_for_file(&query_sent, "child query write");
    assert!(
        matches!(input_receipt.try_recv(), Err(TryRecvError::Empty)),
        "tracked PTY input completed before the child was allowed to read"
    );
    assert!(!drain_start.exists(), "child began reading before release");

    let mut timed = request_checkpoint_with_bounded_busy_retry(
        &session,
        Instant::now() + Duration::from_millis(750),
    );
    expect_checkpoint_timeout(&mut timed);
    // Match the coordinator: a terminal timeout retires its handle. Do not
    // require a second cancellation result after the timeout was delivered.
    drop(timed);
    assert!(
        matches!(input_receipt.try_recv(), Err(TryRecvError::Empty)),
        "tracked PTY input was not blocked through the checkpoint deadline"
    );

    let mut cancelled = request_checkpoint_with_bounded_busy_retry(
        &session,
        Instant::now() + Duration::from_secs(5),
    );
    cancelled.cancel();
    let cancellation_observed = match cancelled.try_recv() {
        None => false,
        Some(Err(SessionError::Cancelled)) => true,
        Some(result) => {
            panic!("cancelled checkpoint returned before the blocked sequence resumed: {result:?}")
        }
    };
    assert!(
        matches!(input_receipt.try_recv(), Err(TryRecvError::Empty)),
        "tracked PTY input completed before the child was allowed to read"
    );
    assert!(!drain_start.exists(), "child began reading before release");

    fs::write(&drain_start, b"read admitted bytes").unwrap();
    wait_for_output(&session, "CHECKPOINT_INPUT_AND_REPLIES_ORDERED");
    let input_result = wait_for_input_result(&mut input_receipt);
    assert_eq!(input_result.status, InputWriteStatus::Complete);
    assert_eq!(input_result.bytes_written, input_result.total_bytes);
    if !cancellation_observed {
        expect_cancelled_checkpoint(&mut cancelled);
    }

    let mut live = request_checkpoint_with_bounded_busy_retry(
        &session,
        Instant::now() + Duration::from_secs(3),
    );
    let snapshot = wait_for_checkpoint_snapshot(&mut live);
    assert_eq!(snapshot.state, State::Running);
    assert!(
        snapshot
            .viewport
            .contents
            .contains("CHECKPOINT_INPUT_AND_REPLIES_ORDERED")
    );
    assert!(snapshot.viewport.program_status.command_epoch.0 > 0);
    let current = snapshot
        .viewport
        .program_status
        .records
        .iter()
        .find(|record| record.id.as_ref() == "checkpoint-current")
        .expect("live checkpoint omitted the current command status");
    assert_eq!(current.state, ProgramState::Working);

    fs::write(&finish, b"exit").unwrap();
    let completed = exited(&session);
    assert_eq!(completed.exit_code, Some(0));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn default_real_pty_fails_closed_when_ordered_output_spill_overflows() {
    let fixture = Fixture::new();
    let script = format!(
        r#"import os,signal,threading,tty
tty.setraw(0)
ready=threading.Event()
signal.signal(signal.SIGUSR1,lambda *_: ready.set())
os.write(1,b'OUTPUT_SPILL_READY')
if not ready.wait(5): raise SystemExit(81)
payload=b'X'*{chunk_bytes}
while True:
    view=memoryview(payload)
    while view:
        view=view[os.write(1,view):]
"#,
        chunk_bytes = INPUT_CHUNK_BYTES,
    );
    fs::write(fixture.script(), script).unwrap();

    let session = Session::spawn(Launch {
        program: "python3".into(),
        args: vec!["-u".into(), fixture.script().to_string_lossy().into_owned()],
        cwd: None,
        size: Size { rows: 8, cols: 80 },
    })
    .unwrap();
    wait_for_output(&session, "OUTPUT_SPILL_READY");
    for index in 0..INPUT_QUEUE_CHUNKS {
        session
            .send_input(&[b'A' + (index % 26) as u8; INPUT_CHUNK_BYTES])
            .unwrap_or_else(|error| panic!("admit PTY input before output flood: {error}"));
    }
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(session.process_id() as i32),
        nix::sys::signal::Signal::SIGUSR1,
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state == State::Failed => {
                assert!(
                    snapshot
                        .failure
                        .as_deref()
                        .is_some_and(|failure| failure.contains("deferred PTY output exceeded")),
                    "unexpected PTY recovery reason: {:?}",
                    snapshot.failure
                );
                break;
            }
            Ok(snapshot) => assert!(
                Instant::now() < deadline,
                "output spill did not fail closed: state={:?}, bytes_read={}",
                snapshot.state,
                snapshot.bytes_read
            ),
            Err(SessionError::Busy) => {}
            Err(error) => panic!("read PTY recovery snapshot: {error}"),
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn default_session_epochs_are_fresh_between_real_pty_launches() {
    let fixture = Fixture::new();
    let mut epochs = Vec::new();
    for index in 0..2 {
        let script = fixture.0.join(format!("epoch-{index}.py"));
        fs::write(
            &script,
            "import os,tty\ntty.setraw(0)\nos.write(1,b'ready')\n",
        )
        .unwrap();
        let mut session = Session::spawn(Launch {
            program: "python3".into(),
            args: vec!["-u".into(), script.to_string_lossy().into_owned()],
            cwd: None,
            size: Size { rows: 4, cols: 40 },
        })
        .unwrap();
        let snapshot = exited(&session);
        epochs.push(snapshot.viewport.program_status.session_epoch);
        session.shutdown(Duration::from_secs(3)).unwrap();
    }
    assert_ne!(epochs[0], epochs[1]);
}

#[test]
fn explicit_session_close_clears_committed_program_records() {
    run_explicit_session_close_clears_committed_program_records(None);
}

fn run_explicit_session_close_clears_committed_program_records(engine: Option<EngineOptions>) {
    let fixture = Fixture::new();
    let script = fixture.script();
    fs::write(
        &script,
        "import os,time,tty\ntty.setraw(0)\nos.write(1,b'\\x1b]7501;state=working:id=close-me\\x07ready')\nwhile True: time.sleep(1)\n",
    )
    .unwrap();
    let launch = Launch {
        program: "python3".into(),
        args: vec!["-u".into(), script.to_string_lossy().into_owned()],
        cwd: None,
        size: Size { rows: 4, cols: 40 },
    };
    let mut session = match engine {
        Some(engine) => {
            Session::spawn_with_engine(launch, &[], &[], ShutdownPolicy::Kill, engine, ())
        }
        None => Session::spawn(launch),
    }
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!(
                    "session failed before close status was published: {:?}",
                    snapshot.failure
                )
            }
            Ok(snapshot)
                if snapshot
                    .viewport
                    .program_status
                    .records
                    .iter()
                    .any(|record| record.id.as_ref() == "close-me") =>
            {
                break;
            }
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("read session snapshot before close: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "working status was not published"
        );
        thread::sleep(Duration::from_millis(2));
    }

    session.shutdown(Duration::from_secs(3)).unwrap();
    let closed = session.snapshot().unwrap();
    assert_eq!(closed.state, State::Closed);
    assert!(closed.viewport.program_status.records.is_empty());
    assert!(
        closed
            .viewport
            .program_status
            .transitions
            .iter()
            .any(|event| event.id.as_ref() == "close-me"
                && event.cause == TransitionCause::TerminalClosed)
    );
}
