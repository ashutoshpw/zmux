//! Real loopback OpenSSH journey; requires sshd and ssh-keygen, never an external host.
use serde_json::Value;
use std::{
    fs,
    io::Read,
    net::{TcpListener, TcpStream},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, Request as DesktopRequest, Shell, control::ControlServer,
    transfers::TransferCommand,
};
use zmux_workspace_core::{SessionId, SurfaceId};

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
        fs::create_dir(root.join("uploads")).unwrap();
        let forced = root.join("shell");
        fs::write(&forced, format!(r#"#!/bin/sh
cd '{root}' || exit 79
export TMPDIR='{root}/uploads'
case "$SSH_ORIGINAL_COMMAND" in
  *zmux-upload*)
    if [ -f '{root}/broken-ack' ]; then
      trap '' PIPE
      /bin/sh -c "$SSH_ORIGINAL_COMMAND" | head -c 0
      exit 0
    fi
    if [ -f '{root}/hangup-ack' ]; then
      /bin/sh -c 'printf() {{ command printf "$@"; kill -HUP "$$"; }}; eval "$SSH_ORIGINAL_COMMAND"' > '{root}/ack'
      result=$?
      cat '{root}/ack'
      exit "$result"
    fi
    if [ -f '{root}/slow' ]; then sleep 4; fi
    if [ -f '{root}/delay-ack' ]; then
      /bin/sh -c "$SSH_ORIGINAL_COMMAND" > '{root}/ack'
      result=$?
      cat '{root}/ack'
      touch '{root}/ack-ready'
      sleep 4
      exit "$result"
    fi
  ;;
esac
if [ -n "$SSH_ORIGINAL_COMMAND" ]; then exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"; else exec /bin/sh -i; fi
"#, root=root.display())).unwrap();
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

fn upload(socket: &Path, id: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let uploads = json(socket, &["list-uploads"]);
        let value = uploads["uploads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["id"] == id)
            .unwrap();
        if predicate(value) {
            return value.clone();
        }
        assert!(
            Instant::now() < deadline,
            "upload did not reach expected state: {value}"
        );
        thread::sleep(Duration::from_millis(10));
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

    // A new login can no longer authenticate. Uploads must reuse this live terminal's
    // private master rather than reconnecting with another identity/configuration.
    fs::write(fixture.root.join("authorized_keys"), b"").unwrap();
    let source = fixture.root.join("image ' $(touch injected)-画像.png");
    let contents: Vec<u8> = (0..=255).cycle().take(100_003).collect();
    fs::write(&source, &contents).unwrap();
    // The frozen source delegates nonrecursive uploads to SCP: directory inputs fail,
    // while a selected symlink to a regular file uploads its bytes under the alias name.
    // A bad selection after a valid file must fail preparation before any remote operation.
    let directory = fixture.root.join("folder with spaces");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("private.txt"), b"must not upload").unwrap();
    let directory_link = fixture.root.join("folder alias");
    symlink(&directory, &directory_link).unwrap();
    let dangling = fixture.root.join("missing alias");
    symlink(fixture.root.join("does not exist"), &dangling).unwrap();
    let fifo = fixture.root.join("named pipe");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let oversized = fixture.root.join("oversized.bin");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(zmux_remote_runtime::upload::MAX_UPLOAD_FILE_BYTES + 1)
        .unwrap();
    let oversized_link = fixture.root.join("large alias");
    symlink(&oversized, &oversized_link).unwrap();
    for (rejected, reason) in [
        (&directory, "directories and special files"),
        (&directory_link, "directories and special files"),
        (&dangling, "open upload source"),
        (&fifo, "directories and special files"),
        (&oversized_link, "64 MiB upload limit"),
    ] {
        let started = json(
            &socket,
            &[
                "paste-files",
                "--surface",
                restored_surface,
                "--",
                source.to_str().unwrap(),
                rejected.to_str().unwrap(),
            ],
        );
        let id = started["created_uploads"][0].as_u64().unwrap();
        let failed = upload(&socket, id, |u| u["source_released"] == true);
        assert_eq!(failed["state"], "failed", "{failed}");
        assert_eq!(failed["delivery"], "failed", "{failed}");
        assert!(
            failed["error"].as_str().unwrap().contains(reason),
            "{failed}"
        );
        assert_eq!(failed["bytes_sent"], 0);
        assert_eq!(failed["remote_result_uncertain"], false);
        assert!(failed["remote_paths"].as_array().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
            0
        );
    }
    let alias = fixture.root.join("linked ' 画像.png");
    symlink(&source, &alias).unwrap();
    let linked = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            alias.to_str().unwrap(),
        ],
    );
    let id = linked["created_uploads"][0].as_u64().unwrap();
    let linked = upload(&socket, id, |u| u["source_released"] == true);
    assert_eq!(linked["state"], "completed", "{linked}");
    assert_eq!(linked["delivery"], "inserted", "{linked}");
    assert_eq!(linked["bytes_sent"], contents.len());
    let linked_path = Path::new(linked["remote_paths"][0].as_str().unwrap());
    assert_eq!(linked_path.file_name(), alias.file_name());
    assert_eq!(fs::read(linked_path).unwrap(), contents);
    fs::remove_file(linked_path).unwrap();
    fs::remove_dir(linked_path.parent().unwrap()).unwrap();
    // Remove the inserted path from the still-idle interactive shell before its next command.
    json(
        &socket,
        &["send-text", "--surface", restored_surface, "\u{15}"],
    );
    let restored_surface_id = SurfaceId(restored["focused_surface"].as_u64().unwrap());
    let before = desktop.snapshot().unwrap();
    let restored_session_id = before
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .find(|surface| surface.id == restored_surface_id)
        .and_then(|surface| surface.session_id)
        .unwrap();
    let stale = desktop
        .execute_transfer_receipt(DesktopRequest {
            version: 1,
            request_id: 9_001,
            payload: DesktopCommand::Transfer {
                action: TransferCommand::PasteFiles {
                    surface_id: restored_surface_id,
                    expected_session_id: Some(SessionId(restored_session_id.0 + 1)),
                    paths: vec![source.clone()],
                },
            },
        })
        .unwrap_or_else(|error| {
            panic!(
                "stale remote receipt failed before acknowledgement: {}, unknown={}",
                error.error, error.outcome_unknown
            )
        });
    assert!(
        stale
            .result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("session changed"))
    );
    assert!(stale.upload_id.is_none());
    assert_eq!(
        desktop.snapshot().unwrap().uploads.len(),
        before.uploads.len()
    );
    assert_eq!(
        fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
        0,
        "a stale native receipt started remote upload work"
    );
    json(
        &socket,
        &[
            "send-text",
            "--surface",
            restored_surface,
            "IFS= read -r pasted; eval \"set -- $pasted\"; printf 'PATH:%s\\n' \"$1\"\n",
        ],
    );
    let local_pane = initial["workspaces"][0]["root"]["pane"]["id"]
        .as_u64()
        .unwrap()
        .to_string();
    json(&socket, &["focus-pane", "--pane", &local_pane]);
    let created_upload = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(
        created_upload["focused_workspace"],
        initial["focused_workspace"]
    );
    let upload_id = created_upload["created_uploads"][0].as_u64().unwrap();
    let complete = upload(&socket, upload_id, |u| u["delivery"] != "pending");
    assert_eq!(complete["state"], "completed", "{complete}");
    assert_eq!(complete["delivery"], "inserted", "{complete}");
    assert_eq!(complete["remote_result_uncertain"], false);
    assert_eq!(complete["bytes_sent"], contents.len());
    let remote_path = complete["remote_paths"][0].as_str().unwrap();
    assert_eq!(fs::read(remote_path).unwrap(), contents);
    json(&socket, &["send-text", "--surface", restored_surface, "\n"]);
    screen(&socket, restored_surface, |s| {
        s["contents"]
            .as_str()
            .unwrap()
            .replace('\n', "")
            .contains(&format!("PATH:{remote_path}"))
    });
    assert!(
        !fixture.root.join("injected").exists(),
        "a quoted filename became shell code"
    );
    let local_screen = json(&socket, &["read-screen", "--pane", &local_pane]);
    assert!(
        !local_screen["contents"]
            .as_str()
            .unwrap()
            .contains("zmux-upload."),
        "an upload was pasted into the newly focused local terminal"
    );
    fs::remove_file(remote_path).unwrap();
    fs::remove_dir(Path::new(remote_path).parent().unwrap()).unwrap();

    // The remote script has ACKed a complete file, while a wrapper delays process exit.
    // Cancellation must suppress insertion but retain the already acknowledged path.
    fs::write(fixture.root.join("delay-ack"), b"").unwrap();
    let late = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let late_id = late["created_uploads"][0].as_u64().unwrap();
    let ack_deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.root.join("ack-ready").exists() {
        assert!(Instant::now() < ack_deadline, "remote upload did not ACK");
        thread::sleep(Duration::from_millis(10));
    }
    let bytes = fs::read(fixture.root.join("ack")).unwrap();
    let acknowledged = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(fs::read(acknowledged).unwrap(), contents);
    json(&socket, &["cancel-upload", &late_id.to_string()]);
    let late = upload(&socket, late_id, |u| u["source_released"] == true);
    assert_eq!(late["state"], "cancelled");
    assert_eq!(late["delivery"], "cancelled");
    assert_eq!(late["source_released"], true);
    assert_eq!(late["remote_result_uncertain"], false);
    assert_eq!(
        late["remote_paths"][0], acknowledged,
        "late cancellation lost an acknowledged completed file: {late}"
    );
    fs::remove_file(acknowledged).unwrap();
    fs::remove_dir(Path::new(acknowledged).parent().unwrap()).unwrap();
    fs::remove_file(fixture.root.join("delay-ack")).unwrap();

    // Deliver HUP immediately after printf writes the ACK. An acknowledged file
    // must already be retained; the remote cleanup trap must no longer delete it.
    fs::write(fixture.root.join("hangup-ack"), b"").unwrap();
    let hangup = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let hangup_id = hangup["created_uploads"][0].as_u64().unwrap();
    let hangup = upload(&socket, hangup_id, |u| u["delivery"] != "pending");
    let bytes = fs::read(fixture.root.join("ack")).unwrap();
    let committed = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
    assert!(
        Path::new(committed).is_file(),
        "HUP deleted a file after its acknowledgement: {hangup}"
    );
    assert_eq!(hangup["remote_paths"][0], committed);
    assert_eq!(hangup["remote_result_uncertain"], false);
    assert_eq!(fs::read(committed).unwrap(), contents);
    fs::remove_file(committed).unwrap();
    fs::remove_dir(Path::new(committed).parent().unwrap()).unwrap();
    fs::remove_file(fixture.root.join("hangup-ack")).unwrap();

    // A failed acknowledgement write must retain the remote cleanup trap.
    fs::write(fixture.root.join("broken-ack"), b"").unwrap();
    let broken = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let broken_id = broken["created_uploads"][0].as_u64().unwrap();
    let broken = upload(&socket, broken_id, |u| u["state"] == "failed");
    assert_eq!(broken["delivery"], "failed");
    assert_eq!(broken["remote_result_uncertain"], true);
    assert_eq!(
        fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
        0,
        "failed acknowledgement disabled remote cleanup"
    );
    fs::remove_file(fixture.root.join("broken-ack")).unwrap();

    // Stalling the remote consumer must not block cancellation or core navigation.
    fs::write(fixture.root.join("slow"), b"").unwrap();
    let large = fixture.root.join("large.bin");
    fs::write(&large, vec![37_u8; 8 * 1024 * 1024]).unwrap();
    let started = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            large.to_str().unwrap(),
        ],
    );
    let id = started["created_uploads"][0].as_u64().unwrap();
    upload(&socket, id, |u| u["bytes_sent"].as_u64().unwrap() > 0);
    json(&socket, &["status"]); // Must remain responsive while the remote consumer stalls.
    json(&socket, &["cancel-upload", &id.to_string()]);
    let cancelled = upload(&socket, id, |u| u["state"] == "cancelled");
    assert_eq!(cancelled["delivery"], "cancelled");
    assert!(cancelled["remote_paths"].as_array().unwrap().is_empty());

    let closing = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            large.to_str().unwrap(),
        ],
    );
    let closing_id = closing["created_uploads"][0].as_u64().unwrap();
    json(&socket, &["close-surface", restored_surface]);
    let closed = upload(&socket, closing_id, |u| u["state"] == "cancelled");
    assert_eq!(closed["delivery"], "cancelled");
    assert_eq!(closed["surface_id"], restored["focused_surface"]);
    fs::copy(
        fixture.root.join("client key.pub"),
        fixture.root.join("authorized_keys"),
    )
    .unwrap();
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
