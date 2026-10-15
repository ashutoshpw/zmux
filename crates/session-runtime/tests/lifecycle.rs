use std::{
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{
    INPUT_CHUNK_BYTES, InputReceipt, InputWriteFailure, InputWriteResult, InputWriteStatus, Launch,
    Session, SessionError, Snapshot, State,
};
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

fn write_result(receipt: &mut InputReceipt) -> InputWriteResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receipt.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(error) => panic!("write completion disappeared: {error}"),
        }
        assert!(Instant::now() < deadline, "PTY write did not complete");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn failed_start_releases_its_owned_resource_on_the_session_worker() {
    struct Resource(std::sync::mpsc::SyncSender<thread::ThreadId>);
    impl Drop for Resource {
        fn drop(&mut self) {
            let _ = self.0.try_send(thread::current().id());
        }
    }
    let caller = thread::current().id();
    let (released, observation) = std::sync::mpsc::sync_channel(1);
    let result = Session::spawn_with_resource(
        Launch {
            program: "/dev/null/zmux-missing-command".into(),
            args: Vec::new(),
            cwd: None,
            size: Size { rows: 8, cols: 60 },
        },
        &[],
        Resource(released),
    );
    assert!(
        matches!(result, Err(SessionError::Failure(message)) if message.contains("launch PTY command"))
    );
    let worker = observation
        .try_recv()
        .expect("failed startup returned before resource cleanup");
    assert_ne!(
        worker, caller,
        "startup cleanup ran on the launching caller"
    );
}

#[test]
fn write_receipt_confirms_full_pty_acceptance_in_the_existing_input_order() {
    let mut session = launch("stty raw -echo; printf READY; cat");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    session.send_input(b"before-").unwrap();
    let mut receipt = session
        .send_input_with_receipt(b"reply-", Instant::now() + Duration::from_secs(2))
        .unwrap();
    session.send_input(b"after").unwrap();
    let result = write_result(&mut receipt);
    assert_eq!(result.status, InputWriteStatus::Complete);
    assert_eq!(result.bytes_written, 6);
    assert_eq!(result.total_bytes, 6);
    assert_eq!(result.reason, None);
    wait_for(&session, |s| {
        s.viewport.contents.contains("before-reply-after")
    });
    assert!(matches!(
        receipt.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn queued_write_deadline_cancels_before_a_blocked_predecessor_drains_without_replay() {
    let mut session = launch("stty raw -echo; printf READY; kill -STOP $$; cat");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    for _ in 0..8 {
        session.send_input(&[b'x'; INPUT_CHUNK_BYTES]).unwrap();
    }
    let mut receipt = session
        .send_input_with_receipt(b"EXPIRED_TOKEN", Instant::now() + Duration::from_millis(40))
        .unwrap();
    let result = write_result(&mut receipt);
    assert_eq!(result.status, InputWriteStatus::NotWritten);
    assert_eq!(result.bytes_written, 0);
    assert_eq!(result.reason, Some(InputWriteFailure::Deadline));
    // The child cannot drain its input until this explicit release, so expiry
    // must complete while an earlier input is still blocked in the PTY.
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(session.process_id() as i32),
        nix::sys::signal::Signal::SIGCONT,
    )
    .unwrap();
    session.send_input(b"\r\nAFTER_DEADLINE\r\n").unwrap();
    let after = wait_for(&session, |s| s.viewport.contents.contains("AFTER_DEADLINE"));
    assert!(!after.viewport.contents.contains("EXPIRED_TOKEN"));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn bounded_write_receipts_finish_once_under_backpressure_deadline_and_shutdown() {
    for shutdown in [false, true] {
        let mut session = launch("stty raw -echo; printf READY; exec sleep 60");
        wait_for(&session, |s| s.viewport.contents.contains("READY"));
        let deadline = Instant::now()
            + if shutdown {
                Duration::from_secs(30)
            } else {
                Duration::from_millis(80)
            };
        let mut receipts = Vec::new();
        for _ in 0..zmux_session_runtime::INPUT_QUEUE_CHUNKS {
            receipts.push(
                session
                    .send_input_with_receipt(&[b'x'; INPUT_CHUNK_BYTES], deadline)
                    .unwrap(),
            );
        }
        if shutdown {
            session.shutdown(Duration::from_secs(3)).unwrap();
        }
        let results: Vec<_> = receipts.iter_mut().map(write_result).collect();
        assert!(
            results
                .iter()
                .any(|result| result.status != InputWriteStatus::Complete),
            "a non-reading PTY must apply backpressure"
        );
        for (receipt, result) in receipts.iter_mut().zip(results) {
            assert_eq!(result.total_bytes, INPUT_CHUNK_BYTES);
            match result.status {
                InputWriteStatus::Complete => {
                    assert_eq!(result.bytes_written, INPUT_CHUNK_BYTES);
                    assert_eq!(result.reason, None);
                }
                InputWriteStatus::NotWritten => assert_eq!(result.bytes_written, 0),
                InputWriteStatus::Partial => {
                    assert!(result.bytes_written > 0 && result.bytes_written < INPUT_CHUNK_BYTES)
                }
                InputWriteStatus::Unknown => {
                    panic!("normal deadline/shutdown lost worker ownership")
                }
            }
            if result.status != InputWriteStatus::Complete {
                assert_eq!(
                    result.reason,
                    Some(if shutdown {
                        InputWriteFailure::SessionClosed
                    } else {
                        InputWriteFailure::Deadline
                    })
                );
            }
            // The worker may still own a cancelled queued input, but cannot
            // issue another result or write it after the receipt was returned.
            assert!(receipt.try_recv().is_err());
        }
        if !shutdown {
            session.shutdown(Duration::from_secs(3)).unwrap();
        }
        assert!(receipts.iter_mut().all(|receipt| matches!(
            receipt.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        )));
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
fn socket_password_scrubbing_wins_over_application_environment_overrides() {
    let mut session = Session::spawn_with_environment(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "printf 'credential=%s context=%s' \"${ZMUX_SOCKET_PASSWORD-unset}\" \"$ZMUX_SURFACE_ID\"".into()],
        cwd: None,
        size: Size { rows: 8, cols: 60 },
    }, &[("ZMUX_SOCKET_PASSWORD".into(), "synthetic-secret".into()), ("ZMUX_SURFACE_ID".into(), "owned-surface".into())]).unwrap();
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert_eq!(
        snapshot.viewport.contents,
        "credential=unset context=owned-surface"
    );
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
fn interactive_shell_preserves_exact_multiline_input_across_native_chunk_boundaries() {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("multiline input fixture cleanup: {error}");
            }
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("zmux-multiline-{}-{nonce:x}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = Fixture(root);
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let payload: String = (0..24)
        .map(|row| format!("row {row:02}: finally keeps its exact bytes.\n"))
        .collect();
    for (name, program, args) in [
        ("bash", "/bin/bash", vec!["--noprofile", "--norc", "-i"]),
        ("zsh", "/bin/zsh", vec!["-f", "-i"]),
    ] {
        // Both are stock macOS shells. Linux hosts without zsh still exercise
        // bash's interactive line editor; no user startup files are loaded.
        if !std::path::Path::new(program).is_file() {
            assert_eq!(name, "zsh", "the bash fixture dependency is missing");
            continue;
        }
        let output = fixture.0.join(format!("{name}.txt"));
        let command = format!(
            "python3 -c {} {} {}\r",
            quote(
                "import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(sys.argv[2].encode()); print('MULTILINE_'+'CAPTURED', flush=True)"
            ),
            quote(output.to_str().unwrap()),
            quote(&payload)
        );
        assert!(command.len() > 1024 && command.len() < 2048);
        let mut session = Session::spawn_with_environment(
            Launch {
                program: program.into(),
                args: args.into_iter().map(String::from).collect(),
                cwd: Some(fixture.0.clone()),
                size: Size { rows: 31, cols: 63 },
            },
            &[("HOME".into(), fixture.0.to_string_lossy().into_owned())],
        )
        .unwrap();
        // Match the native bridge's two 1024-byte admissions without bypassing
        // the real session writer, shell line discipline, or shell argument parser.
        for chunk in command.as_bytes().chunks(1024) {
            session.send_input(chunk).unwrap();
        }
        wait_for(&session, |value| {
            value.viewport.contents.contains("MULTILINE_CAPTURED")
        });
        assert_eq!(fs::read(output).unwrap(), payload.as_bytes(), "{name}");
        session.shutdown(Duration::from_secs(3)).unwrap();
    }
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

#[test]
fn exited_process_history_remains_navigable_without_accepting_new_input() {
    let mut session =
        launch("i=0; while [ $i -lt 30 ]; do printf 'DONE_%02d\\n' $i; i=$((i+1)); done; exit 7");
    let done = wait_for(&session, |s| s.state == State::Exited);
    let bytes = done.bytes_read;
    assert!(!done.viewport.contents.contains("DONE_00"));
    session.scroll(1000).unwrap();
    let history = wait_for(&session, |s| s.viewport.contents.contains("DONE_00"));
    assert_eq!(history.state, State::Exited);
    assert_eq!(history.exit_code, Some(7));
    assert_eq!(history.bytes_read, bytes);
    assert!(matches!(
        session.send_input(b"must not execute"),
        Err(SessionError::Closed)
    ));
    session.scroll(0).unwrap();
    wait_for(&session, |s| {
        s.viewport.scrollback_offset == 0 && s.viewport.contents.contains("DONE_29")
    });
    session.shutdown(Duration::from_secs(3)).unwrap();
}
