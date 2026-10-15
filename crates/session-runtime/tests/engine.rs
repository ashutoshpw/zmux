#![cfg(feature = "ghostty")]
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{
    EngineKind, EngineOptions, GhosttyLimits, InputReceipt, InputWriteStatus, Launch, Session,
    SessionError, ShutdownPolicy, Snapshot, State,
};
use zmux_terminal_adapter::{Size, Wheel};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zmux-engine-{}-{nonce:x}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
    fn launch(&self, script: &str) -> Launch {
        let path = self.0.join("client.py");
        fs::write(&path, script).unwrap();
        Launch {
            program: "python3".into(),
            args: vec![
                "-u".into(),
                path.to_str().unwrap().into(),
                self.0.to_str().unwrap().into(),
            ],
            cwd: None,
            size: Size { rows: 8, cols: 60 },
        }
    }
    fn session(&self, script: &str, timeout: Duration) -> Session {
        Session::spawn_with_engine(
            self.launch(script),
            &[],
            &[],
            ShutdownPolicy::Kill,
            options(timeout),
            (),
        )
        .unwrap()
    }
    fn mark(&self, name: &str) {
        fs::write(self.0.join(name), b"go").unwrap();
    }
    fn wait_file(&self, name: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(value) = fs::read_to_string(self.0.join(name)) {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "missing client marker {name}; actual ordering={:?}",
                fs::read_to_string(self.0.join("ordering-observation"))
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn options(timeout: Duration) -> EngineOptions {
    let bytes = 16 * 1024 * 1024; // Explicit fixture mapping caps, not production budgets.
    EngineOptions {
        kind: EngineKind::Ghostty(GhosttyLimits {
            engine_bytes: bytes,
            general_bytes: bytes,
            page_bytes: bytes,
        }),
        reply_timeout: timeout,
    }
}
fn wait(session: &Session, predicate: impl Fn(&Snapshot) -> bool) -> std::sync::Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!("session failed: {:?}", snapshot.failure)
            }
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("snapshot: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "session did not reach expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn receipt(receipt: &mut InputReceipt) -> zmux_session_runtime::InputWriteResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receipt.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Empty) => {}
            Err(error) => panic!("receipt: {error}"),
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn actual_child_receives_engine_replies_raw_input_and_kernel_resize_through_session() {
    let fixture = Fixture::new();
    let script = r#"import fcntl,os,pathlib,struct,sys,time,tty
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
def mark(name,value):
    p=root/(name+'.tmp');p.write_text(value);p.replace(root/name)
def read(n):
    value=b''
    while len(value)<n:value+=os.read(0,n-len(value))
    return value
os.write(1,b'GOOD\x1b]2;published\x07')
assert read(3)==b'PRE'
os.write(1,b'\x1b[6n')
reply=read(6);assert reply==b'\x1b[1;5R',repr(reply)
mark('reply',reply.hex())
deadline=time.monotonic()+4
while struct.unpack('HHHH',fcntl.ioctl(0,0x5413 if sys.platform!='darwin' else 0x40087468,b'\0'*8))[:2]!=(5,20):
    assert time.monotonic()<deadline;time.sleep(.005)
mark('resize','5x20')
assert read(4)==b'POST'
os.write(1,b'\r\nDONE');mark('done','done')
"#;
    let mut session = fixture.session(script, Duration::from_secs(2));
    wait(&session, |s| s.viewport.contents.contains("GOOD"));
    assert!(
        session
            .wheel(Wheel {
                lines: 1,
                row: 0,
                col: 0,
                modifiers: 0,
                history: false
            })
            .is_err()
    );
    let mut before = session
        .send_input_with_receipt(b"PRE", Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(receipt(&mut before).status, InputWriteStatus::Complete);
    assert_eq!(fixture.wait_file("reply"), "1b5b313b3552");
    session.resize(Size { rows: 5, cols: 20 }).unwrap();
    assert_eq!(fixture.wait_file("resize"), "5x20");
    session.send_input(b"POST").unwrap();
    fixture.wait_file("done");
    let snapshot = wait(&session, |s| s.state == State::Exited);
    assert_eq!(snapshot.viewport.size, Size { rows: 5, cols: 20 });
    assert!(snapshot.viewport.contents.contains("DONE"));
    session.scroll(1).unwrap();
    session.shutdown(Duration::from_secs(3)).unwrap();
}

const BLOCKED: &str = r#"import fcntl,json,os,pathlib,struct,sys,time,tty
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
def mark(name,value):
    p=root/(name+'.tmp');p.write_text(value);p.replace(root/name)
os.write(1,b'GOOD\x1b]2;published\x07')
while not (root/'query').exists():time.sleep(.002)
os.write(1,b'\x1b[?1h\x1b[?2004h\x1b[6n\x1b]2;candidate\x07');mark('query-sent','sent')
while not (root/'release').exists():
    if (root/'check-size').exists() and not (root/'old-size').exists():
        size=struct.unpack('HHHH',fcntl.ioctl(0,0x5413 if sys.platform!='darwin' else 0x40087468,b'\0'*8))[:2]
        mark('old-size',str(size))
    time.sleep(.002)
value=b'';reply=b'\x1b[1;5R'
while reply not in value:value+=os.read(0,4096)
prefix,suffix=value.split(reply,1)
expected_prefix=int((root/'expected-prefix').read_text())
assert len(prefix)==expected_prefix and set(prefix)=={ord('p')},repr((expected_prefix,len(prefix),prefix[-40:]))
while b'POST' not in suffix:suffix+=os.read(0,4096)
prior,after=suffix.split(b'POST',1)
semantic=b'\x1bOA\x1b[200~P Q\n\x1b[201~'
mark('ordering-observation',json.dumps({'prior_tail':prior[-64:].hex(),'reply':reply.hex(),'after_tail':after[-64:].hex()}))
assert prior.endswith(semantic) and set(prior[:-len(semantic)])<={ord('p')} and not after,repr((prior[-40:],after))
mark('received',json.dumps({'prefix':len(prefix),'reply':reply.hex()}))
os.write(1,b'\r\nDONE')
while True:time.sleep(1)
"#;
fn block_input(session: &Session, reserved_events: usize) -> (usize, Vec<InputReceipt>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut receipts = Vec::new();
    while receipts.len() < 16 - reserved_events {
        match session
            .send_input_with_receipt(&[b'p'; 4096], Instant::now() + Duration::from_secs(20))
        {
            Ok(receipt) => receipts.push(receipt),
            Err(SessionError::QueueFull) => thread::sleep(Duration::from_millis(2)),
            Err(error) => panic!("input: {error}"),
        }
        assert!(Instant::now() < deadline);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut finished = vec![false; receipts.len()];
    let mut previous = 0;
    let mut unchanged = Instant::now();
    loop {
        for (index, receipt) in receipts.iter_mut().enumerate() {
            if !finished[index]
                && let Ok(result) = receipt.try_recv()
            {
                assert_eq!(result.status, InputWriteStatus::Complete);
                assert_eq!(result.bytes_written, 4096);
                assert_eq!(result.total_bytes, 4096);
                finished[index] = true;
            }
        }
        let count = finished.iter().filter(|finished| **finished).count();
        assert!(
            count < receipts.len(),
            "fixture did not create actual PTY backpressure"
        );
        if count != previous {
            previous = count;
            unchanged = Instant::now();
        }
        // Darwin can fill its PTY during the first chunk. A completed chunk is
        // not required: final receipts/child bytes prove positive acceptance.
        if unchanged.elapsed() >= Duration::from_millis(100) {
            break;
        }
        if Instant::now() >= deadline {
            // Failure evidence distinguishes a first partial write from zero
            // accepted bytes without weakening the blocked-fixture contract.
            let diagnostic_deadline = Instant::now() + Duration::from_secs(3);
            eprintln!(
                "blocked fixture close request: {:?}",
                session.request_close_until(diagnostic_deadline)
            );
            eprintln!("blocked fixture completed receipts before close: {count}");
            let mut observations: Vec<_> = finished
                .iter()
                .map(|done| done.then(|| "completed before close".to_owned()))
                .collect();
            while Instant::now() < diagnostic_deadline && observations.iter().any(Option::is_none) {
                for (input, observation) in receipts.iter_mut().zip(&mut observations) {
                    if observation.is_none() {
                        match input.try_recv() {
                            Ok(result) => *observation = Some(format!("{result:?}")),
                            Err(mpsc::TryRecvError::Disconnected) => {
                                *observation = Some("disconnected before result".to_owned());
                            }
                            Err(mpsc::TryRecvError::Empty) => {}
                        }
                    }
                }
                thread::sleep(Duration::from_millis(2));
            }
            for (index, observation) in observations.iter().enumerate() {
                eprintln!(
                    "blocked fixture receipt {index}: {}",
                    observation
                        .as_deref()
                        .unwrap_or("pending at shared diagnostic deadline")
                );
            }
            panic!("PTY acceptance never reached blocked quiescence");
        }
        thread::sleep(Duration::from_millis(2));
    }
    // Already-received receipts have been independently verified. Retain only
    // unfinished receipts so later checks never poll a completed channel twice.
    let completed = finished.iter().filter(|done| **done).count();
    eprintln!("blocked fixture completed receipts before stage: {completed}");
    let pending = receipts
        .into_iter()
        .zip(finished)
        .filter_map(|(receipt, done)| (!done).then_some(receipt))
        .collect();
    (completed, pending)
}

#[test]
fn blocked_reply_keeps_publication_and_resize_old_then_finishes_current_user_suffix_first() {
    let fixture = Fixture::new();
    let mut session = fixture.session(BLOCKED, Duration::from_secs(4));
    let before = wait(&session, |s| s.viewport.title == "published");
    // Reserve three queue slots for semantic key, whole paste and trailing raw
    // bytes. Real physical PTY backpressure is still independently required.
    let (completed, mut receipts) = block_input(&session, 3);
    fs::write(
        fixture.0.join("expected-prefix"),
        ((completed + 1) * 4096).to_string(),
    )
    .unwrap();
    fixture.mark("query");
    fixture.wait_file("query-sent");
    let staged = wait(&session, |s| s.bytes_read >= before.bytes_read + 4);
    assert_eq!(staged.viewport.title, "published");
    assert_eq!(staged.viewport.contents, "GOOD");
    assert!(!staged.viewport.application_cursor && !staged.viewport.bracketed_paste);
    session.resize(Size { rows: 5, cols: 20 }).unwrap();
    fixture.mark("check-size");
    assert_eq!(fixture.wait_file("old-size"), "(8, 60)");
    session
        .send_key(zmux_session_runtime::KeyEvent {
            key: zmux_session_runtime::Key::ArrowUp,
            action: zmux_session_runtime::KeyAction::Press,
            modifiers: 0,
            legacy_control: None,
        })
        .unwrap();
    session.send_paste("P\x1bQ\n").unwrap();
    let mut post = session
        .send_input_with_receipt(b"POST", Instant::now() + Duration::from_secs(4))
        .unwrap();
    fixture.mark("release");
    let report = fixture.wait_file("received");
    eprintln!("blocked fixture child-observed ordering: {report}");
    assert!(report.contains("1b5b313b3552"));
    let snapshot = wait(&session, |s| {
        s.viewport.title == "candidate" && s.viewport.size == Size { rows: 5, cols: 20 }
    });
    assert_eq!(snapshot.state, State::Running);
    for receipt in &mut receipts {
        let result = self::receipt(receipt);
        assert_eq!(result.status, InputWriteStatus::Complete);
        assert_eq!(result.bytes_written, 4096);
        assert_eq!(result.total_bytes, 4096);
    }
    assert_eq!(receipt(&mut post).status, InputWriteStatus::Complete);
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn blocked_reply_deadline_or_close_aborts_without_publishing_or_replaying_queued_input() {
    for cancel in [false, true] {
        let fixture = Fixture::new();
        let mut session = fixture.session(
            BLOCKED,
            if cancel {
                Duration::from_secs(4)
            } else {
                Duration::from_millis(500)
            },
        );
        let before = wait(&session, |s| s.viewport.title == "published");
        let (completed, mut receipts) = block_input(&session, 0);
        fixture.mark("query");
        fixture.wait_file("query-sent");
        let staged = wait(&session, |s| s.bytes_read >= before.bytes_read + 4);
        assert_eq!(staged.viewport.title, "published");
        if cancel {
            session
                .request_close_until(Instant::now() + Duration::from_secs(3))
                .unwrap();
            session.shutdown(Duration::from_secs(3)).unwrap();
        } else {
            let failed = wait(&session, |s| s.state == State::Failed);
            assert!(
                failed
                    .failure
                    .as_deref()
                    .unwrap()
                    .contains("terminal causal reply deadline"),
                "unexpected Session failure: {:?}",
                failed.failure
            );
            session.shutdown(Duration::from_secs(3)).unwrap();
        }
        assert!(session.is_finished());
        let last = session.snapshot().unwrap();
        assert!(
            last.failure
                .as_deref()
                .is_none_or(|failure| !failure.contains("PTY cleanup failed:")),
            "physical cleanup failed: {:?}",
            last.failure
        );
        assert_eq!(last.viewport.title, "published");
        assert_eq!(last.viewport.contents, "GOOD");
        let results: Vec<_> = receipts.iter_mut().map(receipt).collect();
        let accepted_prefix = completed * 4096
            + results
                .iter()
                .map(|result| result.bytes_written)
                .sum::<usize>();
        eprintln!(
            "blocked fixture cancel={cancel} accepted-prefix={accepted_prefix} receipts: {results:?}"
        );
        assert!(accepted_prefix > 0, "fixture accepted no physical input");
        assert!(
            results
                .iter()
                .all(|r| r.status != InputWriteStatus::Complete)
        );
        assert!(session.send_input(b"no replay").is_err());
    }
}

#[test]
fn engine_constructor_failure_precedes_child_launch_and_releases_resource_on_worker() {
    struct Resource(mpsc::SyncSender<thread::ThreadId>);
    impl Drop for Resource {
        fn drop(&mut self) {
            let _ = self.0.try_send(thread::current().id());
        }
    }
    let fixture = Fixture::new();
    let (released, observation) = mpsc::sync_channel(1);
    let caller = thread::current().id();
    let config = EngineOptions {
        kind: EngineKind::Ghostty(GhosttyLimits {
            engine_bytes: 4096,
            general_bytes: 4096,
            page_bytes: 4096,
        }),
        reply_timeout: Duration::from_secs(1),
    };
    let result = Session::spawn_with_engine(
        fixture.launch("import pathlib,sys;pathlib.Path(sys.argv[1],'launched').write_text('bad')"),
        &[],
        &[],
        ShutdownPolicy::Kill,
        config,
        Resource(released),
    );
    assert!(
        matches!(result,Err(SessionError::Failure(ref error))if error.contains("EngineConstruction"))
    );
    assert_ne!(observation.try_recv().unwrap(), caller);
    assert!(!fixture.0.join("launched").exists());
}
