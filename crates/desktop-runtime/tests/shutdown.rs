use std::{
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{
    Command, Desktop, DesktopError, Request, Shell, downloads::NativeDownload, shutdown::Phase,
};
use zmux_session_runtime::child::observe_child;

#[test]
fn expired_shutdown_retains_native_barrier_and_proves_terminal_reap_before_release() {
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exec sleep 20".into()],
        cwd: None,
    })
    .unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        match desktop.snapshot() {
            Ok(snapshot) => {
                if let Some(pid) = snapshot.frames.iter().find_map(|frame| frame.process_id) {
                    break pid;
                }
            }
            Err(DesktopError::Busy) => (),
            Err(error) => panic!("{error}"),
        }
        assert!(Instant::now() < ready_deadline);
        thread::sleep(Duration::from_millis(5));
    };
    let initial = desktop
        .begin_shutdown(Duration::from_millis(1), true)
        .unwrap();
    thread::sleep(Duration::from_millis(10));
    let started = Instant::now();
    let pending = desktop
        .wait_shutdown(initial.ticket, Duration::from_secs(10))
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "wait renewed the root budget"
    );
    assert!(pending.deadline_expired);
    assert!(!pending.physical_cleanup_complete);
    assert!(pending.pending.native_cleanup);
    let repeated = desktop
        .begin_shutdown(Duration::from_secs(30), false)
        .unwrap();
    assert_eq!(repeated.ticket, initial.ticket);
    assert_eq!(repeated.remaining_ms, 0);
    assert!(repeated.pending.native_cleanup);
    assert!(matches!(
        desktop.submit(Request {
            version: 1,
            request_id: 99,
            payload: Command::Action {
                action: zmux_workspace_core::Action::CreateWorkspace {
                    name: "must not start".into()
                }
            }
        }),
        Err(DesktopError::Closed)
    ));
    // This trusted callback remains useful while regular commands are frozen.
    desktop.native_download(NativeDownload::Drain).unwrap();
    loop {
        let report = desktop.shutdown_status(initial.ticket).unwrap();
        if report.runtime_cleanup_complete {
            assert!(!report.physical_cleanup_complete);
            assert_eq!(report.phase, Phase::Draining);
            break;
        }
        assert!(Instant::now() < ready_deadline, "{report:?}");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(observe_child(pid, false), Err(ref error) if error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32)),
        "runtime cleanup was reported before final terminal reap"
    );
    desktop.shutdown_native_complete(initial.ticket).unwrap();
    loop {
        let report = desktop.shutdown_status(initial.ticket).unwrap();
        if report.physical_cleanup_complete {
            assert_eq!(report.phase, Phase::Complete);
            break;
        }
        assert!(Instant::now() < ready_deadline, "{report:?}");
        thread::sleep(Duration::from_millis(5));
    }
}
