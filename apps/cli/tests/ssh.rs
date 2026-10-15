//! Real loopback OpenSSH journey; requires sshd and ssh-keygen, never an external host.
use serde_json::Value;
use std::{
    fs,
    io::Read,
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

struct Fixture {
    root: PathBuf,
    daemon: Child,
}
impl Fixture {
    fn start() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // macOS TMPDIR paths can exceed sockaddr_un's path limit once a socket is added.
        let root = PathBuf::from("/tmp").join(format!("zs{}-{nonce:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["host", "client key"] {
            let generated = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(root.join(name))
                .output()
                .expect("SSH fixtures require ssh-keygen");
            assert!(generated.status.success(), "ssh-keygen failed");
        }
        fs::copy(root.join("client key.pub"), root.join("authorized_keys")).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let user = Command::new("id").arg("-un").output().unwrap();
        assert!(user.status.success());
        let user = String::from_utf8(user.stdout).unwrap().trim().to_owned();
        let forced = root.join("shell");
        fs::write(&forced, "#!/bin/sh\nif [ -n \"$SSH_ORIGINAL_COMMAND\" ]; then exec /bin/sh -c \"$SSH_ORIGINAL_COMMAND\"; else exec /bin/sh -i; fi\n").unwrap();
        fs::set_permissions(&forced, fs::Permissions::from_mode(0o700)).unwrap();
        let server_config = root.join("server");
        fs::write(&server_config, format!("Port {port}\nListenAddress 127.0.0.1\nHostKey {}/host\nAuthorizedKeysFile {}/authorized_keys\nPidFile {}/pid\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nStrictModes no\nLogLevel ERROR\nPrintMotd no\nPrintLastLog no\nForceCommand {}\n", root.display(), root.display(), root.display(), forced.display())).unwrap();
        fs::write(root.join("client"), format!("Host fixture\n HostName 127.0.0.1\n Port {port}\n User {user}\n UserKnownHostsFile {}/known\n StrictHostKeyChecking accept-new\n BatchMode yes\n IdentitiesOnly yes\n ConnectTimeout 1\n ConnectionAttempts 1\n", root.display())).unwrap();
        let daemon = Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(server_config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("SSH fixtures require openssh-server (/usr/sbin/sshd)");
        let mut fixture = Self { root, daemon };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            if let Some(status) = fixture.daemon.try_wait().unwrap() {
                let mut error = String::new();
                fixture
                    .daemon
                    .stderr
                    .as_mut()
                    .unwrap()
                    .read_to_string(&mut error)
                    .unwrap();
                panic!("fixture sshd failed {status}: {error}");
            }
            assert!(Instant::now() < deadline, "fixture sshd did not listen");
            thread::sleep(Duration::from_millis(10));
        }
        fixture
    }
    fn stop(&mut self) {
        if self.daemon.try_wait().unwrap().is_none() {
            self.daemon.kill().unwrap();
            self.daemon.wait().unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("fixture cleanup: {error}");
        }
    }
}
fn invoke(socket: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(socket)
        .args(args)
        .env("HOME", socket.parent().unwrap())
        .env_remove("ZMUX_SOCKET_PASSWORD")
        .env_remove("ZMUX_SOCKET_PASSWORD_FILE")
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap()
}
fn json(socket: &Path, args: &[&str]) -> Value {
    let output = invoke(socket, args);
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn screen(socket: &Path, surface: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = invoke(socket, &["read-screen", "--surface", surface]);
        if output.status.success() {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            if predicate(&value) {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "remote terminal did not reach expected state: {value}"
            );
        } else {
            assert!(
                Instant::now() < deadline,
                "remote screen unavailable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command() {
    let mut fixture = Fixture::start();
    let socket = fixture.root.join("control.sock");
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let initial = loop {
        let initial = json(&socket, &["list-workspaces"]);
        if !initial["focused_workspace"].is_null() {
            break initial;
        }
        assert!(Instant::now() < deadline, "initial workspace not ready");
        thread::sleep(Duration::from_millis(10));
    };
    let config = fixture.root.join("client").to_string_lossy().into_owned();
    let key = fixture
        .root
        .join("client key")
        .to_string_lossy()
        .into_owned();
    let counter = fixture.root.join("executions");
    let startup = format!(
        "[ -t 0 ] || exit 77; printf 'run\\n' >> '{}'; printf 'REMOTE_START\\n'; IFS= read -r line; printf 'REMOTE_INPUT:%s\\n' \"$line\"",
        counter.display()
    );
    let created = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config,
            "--identity",
            &key,
            "--name",
            "Remote fixture",
            "--no-focus",
            "--command",
            &startup,
        ],
    );
    assert_eq!(created["focused_workspace"], initial["focused_workspace"]);
    let surface = created["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    let started = screen(&socket, surface, |s| {
        s["contents"].as_str().unwrap().contains("REMOTE_START") || s["state"] == "exited"
    });
    assert!(
        started["contents"]
            .as_str()
            .unwrap()
            .contains("REMOTE_START"),
        "remote terminal did not receive a PTY: {started}"
    );
    json(
        &socket,
        &["send-text", "--surface", surface, "hello remote\n"],
    );
    let exited = screen(&socket, surface, |s| s["state"] == "exited");
    assert_eq!(exited["exit_code"], 0);
    assert!(
        exited["contents"]
            .as_str()
            .unwrap()
            .contains("REMOTE_INPUT:hello remote")
    );
    assert_eq!(fs::read_to_string(&counter).unwrap(), "run\n");
    json(&socket, &["close-surface", surface]);
    let restored = json(&socket, &["reopen-closed"]);
    let restored_surface = restored["focused_surface_uuid"].as_str().unwrap();
    // Readiness is asynchronous; start an input only after a running PTY is observable.
    screen(&socket, restored_surface, |s| s["state"] == "running");
    json(
        &socket,
        &[
            "send-text",
            "--surface",
            restored_surface,
            "printf 'REOPEN_REMOTE_READY\\n'\n",
        ],
    );
    screen(&socket, restored_surface, |s| {
        s["contents"]
            .as_str()
            .unwrap()
            .lines()
            .any(|line| line.ends_with("REOPEN_REMOTE_READY"))
    });
    assert_eq!(fs::read_to_string(&counter).unwrap(), "run\n");
    json(&socket, &["close-surface", restored_surface]);
    fixture.stop();
    let failed = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config,
            "--identity",
            &key,
            "--command",
            &startup,
        ],
    );
    let failed_surface = failed["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    let failure = screen(&socket, failed_surface, |s| s["state"] == "exited");
    assert_eq!(failure["exit_code"], 255);
    assert_eq!(
        fs::read_to_string(&counter).unwrap(),
        "run\n",
        "connection failure must never run the command locally"
    );
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
