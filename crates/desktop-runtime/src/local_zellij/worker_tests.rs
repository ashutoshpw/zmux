//! Real owned-child reproduction of portable-pty's early spawn-pipe closure.
//! This isolates the descriptor-closing mechanism, not its full spawn function.
use super::*;
use std::process::Child;

struct GatedChild {
    child: Child,
    root: PathBuf,
    executable: PathBuf,
}
impl GatedChild {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zmux-cli-exec-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let executable = Path::new("/bin/cat").canonicalize().unwrap();
        let child = zmux_session_test_support::spawn_with_closed_spawn_pipe_gate(
            &executable,
            &root.join("release"),
        )
        .unwrap();
        Self {
            child,
            root,
            executable,
        }
    }
    fn inherited(&self) -> ProcessIdentity {
        let observed = process_identity(self.child.id()).unwrap();
        let launcher = process_identity(std::process::id()).unwrap();
        assert_eq!(
            observed.executable, launcher.executable,
            "gate must expose inherited image"
        );
        assert_eq!(observed.parent_pid, launcher.pid);
        observed
    }
    fn release(&self) {
        fs::write(self.root.join("release"), b"exec").unwrap();
    }
}
impl Drop for GatedChild {
    fn drop(&mut self) {
        // This is the directly retained physical child, never an adopted PID.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn configured_cli_waits_for_same_retained_child_to_exec_after_spawn_pipe_closes() {
    let child = GatedChild::new();
    let inherited = child.inherited();
    let launcher = process_identity(std::process::id()).unwrap();
    child.release();
    let identity = configured_cli_identity(
        inherited.clone(),
        &launcher,
        &child.executable,
        Instant::now() + Duration::from_secs(2),
        &AtomicBool::new(false),
    )
    .expect("same retained child must reach its configured image before admission");
    assert_eq!(identity.executable, child.executable);
    assert_eq!(
        (
            identity.pid,
            identity.uid,
            identity.parent_pid,
            identity.start,
            &identity.boot
        ),
        (
            inherited.pid,
            inherited.uid,
            inherited.parent_pid,
            inherited.start,
            &inherited.boot
        )
    );
    assert_eq!(process_identity(child.child.id()).unwrap(), identity);
}

#[test]
fn configured_cli_exec_admission_rejects_cancellation_deadline_and_identity_drift() {
    let child = GatedChild::new();
    let inherited = child.inherited();
    let launcher = process_identity(std::process::id()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    for (cancelled, deadline) in [(true, deadline), (false, Instant::now())] {
        let error = configured_cli_identity(
            inherited.clone(),
            &launcher,
            &child.executable,
            deadline,
            &AtomicBool::new(cancelled),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("cancelled or deadline"),
            "{error}"
        );
    }
    for field in ["parent", "uid", "boot"] {
        let mut changed = inherited.clone();
        match field {
            "parent" => changed.parent_pid += 1,
            "uid" => changed.uid += 1,
            _ => changed.boot = "00000000-0000-4000-8000-000000000001".into(),
        }
        let error = configured_cli_identity(
            changed,
            &launcher,
            &child.executable,
            deadline,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("physical child identity changed"),
            "{error}"
        );
    }
    let mut changed_start = inherited.clone();
    changed_start.start[0] += 1;
    child.release();
    let error = configured_cli_identity(
        changed_start,
        &launcher,
        &child.executable,
        deadline,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("physical child identity changed"),
        "{error}"
    );
}

#[test]
fn configured_cli_exec_admission_rejects_unrelated_image_and_departed_child() {
    let mut child = GatedChild::new();
    let inherited = child.inherited();
    let launcher = process_identity(std::process::id()).unwrap();
    child.release();
    let error = configured_cli_identity(
        inherited.clone(),
        &launcher,
        &Path::new("/bin/sh").canonicalize().unwrap(),
        Instant::now() + Duration::from_secs(2),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("unconfigured executable"),
        "{error}"
    );
    drop(child.child.stdin.take());
    assert!(child.child.wait().unwrap().success());
    let error = configured_cli_identity(
        inherited,
        &launcher,
        &child.executable,
        Instant::now() + Duration::from_secs(2),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("inspect retained CLI exec transition"),
        "{error}"
    );
}

#[path = "worker_tests/readiness.rs"]
mod readiness;
