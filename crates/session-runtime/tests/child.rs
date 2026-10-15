use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};
struct Reap(std::process::Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
use zmux_session_runtime::child::{ExitObservation, observe_child, process_identity};

#[test]
fn live_identity_contains_actual_parent_and_stable_boot_start_path() {
    let identity = process_identity(std::process::id()).unwrap();
    assert_eq!(identity.pid, std::process::id());
    assert_eq!(identity.parent_pid, nix::unistd::getppid().as_raw() as u32);
    assert_eq!(identity, process_identity(std::process::id()).unwrap());
    assert!(identity.start[0] > 0);
    assert!(identity.executable.is_absolute());
    assert!(process_identity(0).is_err());
    assert!(process_identity(u32::MAX).is_err());
}

#[test]
fn repeated_exit_observation_retains_the_child_until_final_wait() {
    let mut child = Reap(
        Command::new("/bin/sh")
            .args(["-c", "kill -STOP $$; exit 7"])
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(
            observe_child(pid, true).unwrap(),
            Some(ExitObservation::Stopped(_))
        ) {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let identity = process_identity(pid).unwrap();
    assert_eq!(identity.parent_pid, std::process::id());
    assert_eq!(observe_child(pid, false).unwrap(), None);
    kill(Pid::from_raw(pid as i32), Signal::SIGCONT).unwrap();
    loop {
        if let Some(status) = observe_child(pid, false).unwrap() {
            assert_eq!(status, ExitObservation::Exited(7));
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        observe_child(pid, false).unwrap(),
        Some(ExitObservation::Exited(7))
    );
    assert_eq!(child.0.wait().unwrap().code(), Some(7));
    assert!(observe_child(pid, false).is_err());
}
