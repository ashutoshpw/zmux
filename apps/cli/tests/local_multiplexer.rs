//! Public headless lifecycle and real PTY attach; never touches the default tmux server.
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt},
    path::PathBuf,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{Launch, Session, State};
struct Fixture {
    root: PathBuf,
    binary: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").join(format!("zmc-{}", uuid_string()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("tmux"))
            .find(|p| p.is_file())
            .expect("real tmux is required");
        Self {
            root: root.canonicalize().unwrap(),
            binary,
        }
    }
    fn socket(&self) -> PathBuf {
        self.root.join(".config/zmux/local-tmux/server.sock")
    }
    fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".into(), self.root.to_str().unwrap().into()),
            (
                "ZMUX_LOCAL_TMUX_BIN".into(),
                self.binary.to_str().unwrap().into(),
            ),
            (
                "ZMUX_SOCKET_PATH".into(),
                "relative-unavailable-app-socket".into(),
            ),
            (
                "ZMUX_SOCKET_PASSWORD_FILE".into(),
                self.root.join("missing-password").to_str().unwrap().into(),
            ),
            (
                "ZMUX_WORKSPACE_ID".into(),
                "invalid-caller-workspace".into(),
            ),
            ("ZMUX_SURFACE_ID".into(), "invalid-caller-surface".into()),
        ]
    }
    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(args)
            .envs(self.environment())
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        let o = self.output(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn attach(&self, args: &[&str]) -> Session {
        Session::spawn_with_environment(
            Launch {
                program: env!("CARGO_BIN_EXE_zmux").into(),
                args: args.iter().map(|s| (*s).into()).collect(),
                cwd: Some(self.root.clone()),
                size: zmux_session_runtime::default_size(),
            },
            &self.environment(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if fs::symlink_metadata(self.socket()).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = Command::new(&self.binary)
                .args(["-f", "/dev/null", "-S"])
                .arg(self.socket())
                .arg("kill-server")
                .output();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn uuid_string() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "public tmux lifecycle timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn headless_cli_attaches_detaches_and_reattaches_the_same_live_shell_without_app_socket() {
    let f = Fixture::new();
    let pidfile = f.root.join("owner-pid");
    let command = format!(
        "printf '%s' \"$$\" > '{}.pending'; mv '{}.pending' '{}'; exec /bin/sh -i",
        pidfile.to_str().unwrap(),
        pidfile.to_str().unwrap(),
        pidfile.to_str().unwrap()
    );
    let created = f.json(&[
        "local-tmux",
        "start",
        "work",
        "--detached",
        "--cwd",
        f.root.to_str().unwrap(),
        "--command",
        &command,
    ]);
    let id = created["session"]["id"].as_str().unwrap();
    assert_eq!(created["session"]["state"], "live");
    wait(|| pidfile.is_file());
    let pid = fs::read_to_string(&pidfile).unwrap();
    assert!(!pid.is_empty());
    assert_eq!(
        f.json(&["local-tmux", "list", "--json"])["sessions"][0]["id"],
        id
    );
    let mut first = f.attach(&["local-tmux", "attach", "--id", id, "--headless"]);
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    let first_input = f.root.join("first-client-input");
    let input = format!(
        "printf '%s' accepted > '{}.pending'; mv '{}.pending' '{}'\r",
        first_input.to_str().unwrap(),
        first_input.to_str().unwrap(),
        first_input.to_str().unwrap()
    );
    first.send_input(input.as_bytes()).unwrap();
    wait(|| fs::read_to_string(&first_input).is_ok_and(|value| value == "accepted"));
    assert_eq!(
        f.json(&["local-tmux", "detach", "--id", id])["detached_clients"],
        1
    );
    wait(|| first.snapshot().is_ok_and(|s| s.state == State::Exited));
    first.shutdown(Duration::from_secs(2)).unwrap();
    assert_eq!(
        f.json(&["local-tmux", "status", "--id", id])["session"]["state"],
        "live"
    );
    assert_eq!(fs::read_to_string(&pidfile).unwrap(), pid);
    let mut second = f.attach(&["tmux", "attach", "work", "--headless"]);
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    let confirmation = f.root.join("reattached-pid");
    let command = format!(
        "printf '%s' \"$$\" > '{}.pending'; mv '{}.pending' '{}'\r",
        confirmation.to_str().unwrap(),
        confirmation.to_str().unwrap(),
        confirmation.to_str().unwrap()
    );
    second.send_input(command.as_bytes()).unwrap();
    wait(|| confirmation.is_file());
    assert_eq!(fs::read_to_string(confirmation).unwrap(), pid);
    second.shutdown(Duration::from_secs(2)).unwrap();
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    });
    assert_eq!(
        f.json(&["local-tmux", "close", "--id", id])["closed_id"],
        id
    );
    assert!(
        f.json(&["local-tmux", "list"])["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn offline_help_and_invalid_gui_alias_or_client_forms_do_not_open_app_or_profile() {
    let f = Fixture::new();
    for command in ["local-tmux", "tmux"] {
        let o = f.output(&[command, "--help"]);
        assert!(o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("--headless"));
    }
    for args in [
        vec!["local-tmux", "start", "work"],
        vec!["local-tmux", "attach", "work"],
        vec!["tmux", "list"],
        vec!["local-tmux", "detach", "work", "--client", "one"],
        vec![
            "local-tmux",
            "attach",
            "work",
            "--headless",
            "--workspace",
            "1",
        ],
        vec!["local-tmux", "start", "bad/name", "--detached"],
        vec!["local-tmux", "cleanup", "work"],
        vec!["local-tmux", "list", "--id", "bad"],
        vec!["local-tmux", "detach", "work", "--all", "--all"],
        vec!["local-tmux", "status", "work", "--id", "bad"],
    ] {
        let o = f.output(&args);
        assert!(!o.status.success(), "{args:?}");
        let error = String::from_utf8_lossy(&o.stderr);
        assert!(!error.contains("socket path must be absolute"));
        assert!(!error.contains("credential"));
        assert!(
            !f.root.join(".config").exists(),
            "profile touched for {args:?}"
        );
    }
    let o = f.output(&["tmux", "attach", "work", "--headless"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("requires terminal stdin and stdout"));
    assert!(!f.root.join(".config").exists());
}
