use std::{
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{INPUT_CHUNK_BYTES, Launch, Session, SessionError, Snapshot, State};
use zmux_terminal_adapter::Size;

fn launch(script: &str) -> Session {
    Session::spawn(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        size: Size { rows: 8, cols: 60 },
    })
    .unwrap()
}

fn wait_for(session: &Session, predicate: impl Fn(&Snapshot) -> bool) -> std::sync::Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!("session failed: {:?}", snapshot.failure)
            }
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("snapshot failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "session did not reach expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn real_pty_output_is_parsed_and_process_exit_is_reported() {
    let mut session = launch("printf 'old\\r\\033[2Khéllo 世界\\n'; exit 7");
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert_eq!(snapshot.exit_code, Some(7));
    assert_eq!(snapshot.viewport.contents, "héllo 世界");
    assert!(snapshot.bytes_read > 0);
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn input_and_resize_reach_the_real_terminal() {
    let mut session = launch("read line; stty size; printf '\\naccepted:%s\\n' \"$line\"");
    session.resize(Size { rows: 12, cols: 45 }).unwrap();
    wait_for(&session, |s| s.viewport.size == Size { rows: 12, cols: 45 });
    session.send_input(b"test-input\n").unwrap();
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert!(snapshot.viewport.contents.contains("12 45"));
    assert!(snapshot.viewport.contents.contains("accepted:test-input"));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn output_flood_does_not_prevent_cancel_and_leader_reaping() {
    let mut session = launch("while :; do printf 'busy-output\\n'; done");
    let pid = session.process_id();
    wait_for(&session, |s| s.bytes_read > 20_000);
    session.shutdown(Duration::from_secs(3)).unwrap();
    assert_eq!(session.snapshot().unwrap().state, State::Closed);
    assert_eq!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    );
    assert!(matches!(
        session.send_input(b"no\n"),
        Err(SessionError::Closed)
    ));
}

#[test]
fn malformed_requests_do_not_change_session_geometry_or_reach_input() {
    let mut session = launch("exec sleep 60");
    let before = session.snapshot().unwrap().viewport.size;
    assert!(session.resize(Size { rows: 0, cols: 60 }).is_err());
    assert!(matches!(
        session.send_input(&vec![b'x'; INPUT_CHUNK_BYTES + 1]),
        Err(SessionError::InputSize)
    ));
    assert!(matches!(
        session.send_input(b""),
        Err(SessionError::InputSize)
    ));
    assert_eq!(session.snapshot().unwrap().viewport.size, before);
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn dropping_an_active_client_requests_cleanup_without_joining() {
    let session = launch("exec sleep 60");
    let pid = session.process_id();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None)
            == Err(nix::errno::Errno::ESRCH)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "dropped session left its leader running"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn saturated_input_returns_backpressure_without_blocking_cancellation() {
    let mut session = launch("stty raw -echo; printf 'READY'; exec sleep 60");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    let input = [b'x'; INPUT_CHUNK_BYTES];
    let mut full = false;
    for _ in 0..1_000 {
        match session.send_input(&input) {
            Ok(()) => {}
            Err(SessionError::QueueFull) => {
                full = true;
                break;
            }
            Err(error) => panic!("unexpected input failure: {error}"),
        }
    }
    assert!(
        full,
        "a non-reading process must encounter bounded input backpressure"
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
    assert_eq!(session.snapshot().unwrap().state, State::Closed);
}

#[test]
fn exited_session_rejects_new_input_and_resize_requests() {
    let mut session = launch("exit 0");
    wait_for(&session, |s| s.state == State::Exited);
    assert!(matches!(
        session.resize(Size { rows: 10, cols: 40 }),
        Err(SessionError::Closed)
    ));
    assert!(matches!(
        session.send_input(b"unused\n"),
        Err(SessionError::Closed)
    ));
    session.shutdown(Duration::from_secs(3)).unwrap();
}
