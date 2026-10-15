use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{
    Launch, Session, SessionError, ShutdownPolicy, State,
    child::{observe_child, process_identity},
};
use zmux_terminal_adapter::Size;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("zmux-shutdown-{}-{nonce:x}", std::process::id()));
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "missing actual client lifecycle marker {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn actual_cleanup_failure_remains_an_error_after_join_for_both_shutdown_policies() {
    const CHILD: &str = "ZMUX_CLEANUP_OUTCOME_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Isolate SIGCHLD disposition in an owned subprocess. SIG_IGN survives
        // exec and makes the kernel actually auto-reap its eventual PTY child.
        let mut child = std::process::Command::new("python3")
            .args([
                "-c",
                "import os,signal,sys;signal.signal(signal.SIGCHLD,signal.SIG_IGN);os.execv(sys.argv[1],sys.argv[1:])",
            ])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "actual_cleanup_failure_remains_an_error_after_join_for_both_shutdown_policies",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated cleanup-outcome child timed out");
            }
            thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "isolated cleanup-outcome child: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    for policy in [
        ShutdownPolicy::Kill,
        ShutdownPolicy::Graceful {
            timeout: Duration::from_secs(1),
        },
    ] {
        let fixture = Fixture::new();
        let mut session = Session::spawn_with_shutdown_policy(
            fixture.launch("import time;print('ready',flush=True);time.sleep(.05)"),
            &[],
            &[],
            policy,
            (),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if session.is_finished() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "auto-reaped child did not finish"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let first = session.shutdown(Duration::ZERO).unwrap_err().to_string();
        assert!(first.contains("PTY cleanup failed:"), "{first}");
        let repeated = session.shutdown(Duration::ZERO).unwrap_err().to_string();
        assert_eq!(
            first, repeated,
            "joining consumed the physical cleanup error"
        );
    }
}

#[test]
fn graceful_close_retains_wrapper_pty_and_resource_until_nested_cleanup_and_final_reap() {
    struct Resource {
        pid: Arc<AtomicU32>,
        released: mpsc::SyncSender<bool>,
    }
    impl Drop for Resource {
        fn drop(&mut self) {
            let observation = observe_child(self.pid.load(Ordering::Acquire), false);
            let reaped = matches!(observation, Err(ref error) if error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32));
            let _ = self.released.try_send(reaped);
        }
    }
    let fixture = Fixture::new();
    let script = r#"import os,pathlib,signal,subprocess,sys,time,tty
root=pathlib.Path(sys.argv[1]); tty.setraw(0)
stopping=False
def terminate(signum,frame):
    global stopping
    stopping=True
signal.signal(signal.SIGTERM,terminate)
child=subprocess.Popen([sys.executable,'-c','import pathlib,signal,sys,time; signal.signal(signal.SIGTERM,lambda *_: sys.exit(0)); pathlib.Path(sys.argv[1]).write_text(str(__import__("os").getpid())); time.sleep(4)',str(root/'child-ready')],start_new_session=True)
while not (root/'child-ready').exists(): time.sleep(.005)
(root/'ready').write_text('ready')
while not stopping: time.sleep(.005)
(root/'terminating').write_text('term')
deadline=time.monotonic()+2
while not (root/'release').exists() and time.monotonic()<deadline: time.sleep(.005)
child.terminate(); child.wait(timeout=1)
(root/'child-reaped').write_text(str(child.returncode))
pending=b'x'*262144
while pending: pending=pending[os.write(1,pending):]
(root/'cleanup-done').write_text('done')
"#;
    let pid = Arc::new(AtomicU32::new(0));
    let (released, observation) = mpsc::sync_channel(1);
    let mut session = Session::spawn_with_shutdown_policy(
        fixture.launch(script),
        &[],
        &[],
        ShutdownPolicy::Graceful {
            timeout: Duration::from_secs(3),
        },
        Resource {
            pid: pid.clone(),
            released,
        },
    )
    .unwrap();
    pid.store(session.process_id(), Ordering::Release);
    wait_file(&fixture.0.join("ready"));
    let child_pid: u32 = fs::read_to_string(fixture.0.join("child-ready"))
        .unwrap()
        .parse()
        .unwrap();
    let child = process_identity(child_pid).unwrap();
    let mut receipts = (0..zmux_session_runtime::INPUT_QUEUE_CHUNKS)
        .map(|_| {
            session
                .send_input_with_receipt(
                    &[b'x'; zmux_session_runtime::INPUT_CHUNK_BYTES],
                    Instant::now() + Duration::from_secs(10),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    session.request_close().unwrap();
    wait_file(&fixture.0.join("terminating"));
    let results = receipts
        .iter_mut()
        .map(|receipt| {
            receipt
                .try_recv()
                .expect("closing stopped the input writer before signaling the client")
        })
        .collect::<Vec<_>>();
    assert!(
        results
            .iter()
            .any(|result| result.status != zmux_session_runtime::InputWriteStatus::Complete)
    );
    for result in results {
        if result.status != zmux_session_runtime::InputWriteStatus::Complete {
            assert_eq!(
                result.reason,
                Some(zmux_session_runtime::InputWriteFailure::SessionClosed)
            );
        }
    }
    assert!(!session.is_finished());
    assert!(matches!(
        observation.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        session.shutdown(Duration::ZERO),
        Err(SessionError::Timeout)
    ));
    assert!(matches!(
        session.send_input(b"must-not-write"),
        Err(SessionError::Closed)
    ));
    fs::write(fixture.0.join("release"), b"release").unwrap();
    session.shutdown(Duration::from_secs(4)).unwrap();
    assert!(session.is_finished());
    assert_eq!(session.snapshot().unwrap().state, State::Closed);
    assert_eq!(
        fs::read_to_string(fixture.0.join("child-reaped")).unwrap(),
        "0"
    );
    assert!(fixture.0.join("cleanup-done").exists());
    assert!(!process_identity(child.pid).is_ok_and(|current| current == child));
    assert!(
        observation.recv_timeout(Duration::from_secs(1)).unwrap(),
        "resource released before final physical wrapper reap"
    );
    assert!(session.snapshot().unwrap().bytes_read >= 262144);
}

#[test]
fn graceful_deadline_escalates_only_the_retained_child_and_reports_forced_cleanup() {
    let script = "import os,pathlib,signal,sys\nsignal.signal(signal.SIGTERM,signal.SIG_IGN)\npathlib.Path(sys.argv[1],'ready').write_text('ready')\nwhile True: os.write(1,b'x'*4096)\n";
    for (timeout, shared_budget, join_wait) in [
        (Duration::from_millis(100), None, Duration::from_secs(3)),
        (
            Duration::from_secs(3),
            Some(Duration::from_millis(2100)),
            Duration::from_secs(1),
        ),
    ] {
        let fixture = Fixture::new();
        let mut session = Session::spawn_with_shutdown_policy(
            fixture.launch(script),
            &[],
            &[],
            ShutdownPolicy::Graceful { timeout },
            (),
        )
        .unwrap();
        wait_file(&fixture.0.join("ready"));
        let pid = session.process_id();
        if let Some(budget) = shared_budget {
            session
                .request_close_until(Instant::now() + budget)
                .unwrap();
            // An observer cannot extend the installed deadline by asking again.
            session
                .request_close_until(Instant::now() + Duration::from_secs(20))
                .unwrap();
        }
        let outcome = session.shutdown(join_wait);
        assert!(
            matches!(outcome, Err(SessionError::Failure(ref failure)) if failure.contains("graceful shutdown deadline")),
            "{outcome:?}"
        );
        assert!(
            session.is_finished(),
            "forced cleanup retained physical worker ownership"
        );
        let snapshot = session.snapshot().unwrap();
        assert_eq!(snapshot.state, State::Failed);
        assert!(
            snapshot.bytes_read > 0,
            "forced cleanup must remain bounded under output flood"
        );
        assert!(
            snapshot
                .failure
                .as_ref()
                .is_some_and(|failure| failure.contains("graceful shutdown deadline"))
        );
        assert!(
            matches!(observe_child(pid, false), Err(ref error) if error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32))
        );
    }
}
