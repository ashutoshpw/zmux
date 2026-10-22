//! Public schedule grammar, offline help, and private request reconciliation.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};
use uuid::Uuid;
use zmux_remote_runtime::scheduler::TargetIdentity;

const SURFACE: &str = "00000000-0000-0000-0000-000000000010";

struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        // Darwin sockaddr_un.sun_path is capped at 104 bytes; TMPDIR often
        // contributes a long /var/folders prefix. Keep nested control sockets
        // comfortably below that limit on every host.
        let path = PathBuf::from("/tmp").join(format!(
            "zsched-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command.args(args).env("HOME", &self.0);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                command.env_remove(name);
            }
        }
        command
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn target_token() -> String {
    TargetIdentity {
        helper_instance_id: Uuid::from_u128(1),
        remote_uid: nix::unistd::geteuid().as_raw(),
        tmux_socket_id: "/tmp/tmux".into(),
        server_incarnation: Uuid::from_u128(2),
        server_pid: std::process::id(),
        server_start_time: 1,
        session_id: "$0".into(),
        pane_id: "%0".into(),
    }
    .target_id()
    .expect("static identity is a valid CLI target fixture")
}

fn read_frame(stream: &mut UnixStream) -> Value {
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn write_frame(stream: &mut UnixStream, response: &Value) {
    let bytes = serde_json::to_vec(response).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

fn echoing_control_server(path: &Path, message: &str) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn({
        let message = message.to_owned();
        move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_frame(&mut stream);
            write_frame(
                &mut stream,
                &json!({
                    "version": 2,
                    "request_id": request["request_id"],
                    "ok": true,
                    "result": {
                        "ok": false,
                        "error": {"code": "echoed", "message": message},
                        "outcome_unknown": true
                    }
                }),
            );
        }
    })
}

fn closing_control_server(path: &Path) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _request = read_frame(&mut stream);
    })
}

#[test]
fn schedule_help_is_offline_and_lists_the_installed_command_shape() {
    let home = Home::new();
    for args in [
        vec!["--socket", "relative-socket", "schedule", "--help"],
        vec!["--socket", "relative-socket", "help", "schedule", "edit"],
        vec!["--socket", "relative-socket", "help", "schedule", "create"],
    ] {
        let output = home.command(&args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("schedule"));
        assert!(!text.contains("cannot connect"));
    }
    let top = home.command(&["--help"]).output().unwrap();
    assert!(top.status.success());
    let top = String::from_utf8(top.stdout).unwrap();
    assert!(top.contains("schedule"));
    assert!(top.contains("       zmux schedule status"));

    let create_help = home
        .command(&["help", "schedule", "create"])
        .output()
        .unwrap();
    assert!(create_help.status.success());
    let create_help = String::from_utf8(create_help.stdout).unwrap();
    assert!(create_help.contains("five-second grace"));
    assert!(create_help.contains("outcome_unknown"));
    assert!(create_help.contains("no Return"));
}

#[test]
fn malformed_schedule_grammar_fails_without_reading_stdin_or_connecting() {
    let home = Home::new();
    let mut child = home
        .command(&[
            "--socket",
            "/no/such/zmux/control.sock",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--in",
            "5m",
            "--stdin",
            "--bogus",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(250));
    let status = child.try_wait().unwrap();
    assert!(status.is_some(), "malformed command blocked reading stdin");
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("unknown or repeated schedule create option"));
    assert!(!error.contains("cannot connect"));
}

#[test]
fn inline_message_on_an_unknown_option_is_not_echoed_to_stderr() {
    let home = Home::new();
    let secret = "SCHEDULE_FLAG_BODY_MUST_NOT_LEAK_2b54f9";
    let option = format!("--message={secret}");
    let output = home
        .command(&[
            "--socket",
            "/no/such/zmux/control.sock",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            &target_token(),
            "--in",
            "5m",
            &option,
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown or repeated schedule create option"));
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains("cannot connect"));

    let output = home
        .command(&[
            "--socket",
            "/no/such/zmux/control.sock",
            "schedule",
            &option,
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown schedule verb"));
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains("cannot connect"));
}

#[test]
fn missing_local_socket_marks_mutation_known_rejected() {
    let home = Home::new();
    let socket = home.0.join("missing-control.sock");
    let output = home
        .command(&[
            "--socket",
            socket.to_str().unwrap(),
            "--timeout",
            "1000",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            &target_token(),
            "--in",
            "5m",
            "--message",
            "known-unapplied message",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let request_id = stdout["request_id"].as_str().unwrap();
    assert_eq!(stdout["outcome_unknown"], false);
    assert_eq!(stdout["error"]["code"], "local_transport");
    assert!(
        stdout["error"]["message"]
            .as_str()
            .unwrap()
            .contains("start zmux and try again")
    );
    assert!(!stderr.contains("outcome may be unknown"));
    assert!(!stderr.contains("schedule status --request-id"));
    assert!(
        !stdout["error"]["message"]
            .as_str()
            .unwrap()
            .contains("schedule status --request-id")
    );

    let journal_path = home.0.join(".config/zmux/schedule-requests/requests.json");
    let journal: Value = serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
    let requests = journal["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["request_id"], request_id);
    assert_eq!(requests[0]["operation"], "create");
    assert_eq!(requests[0]["state"], "rejected");
}

#[test]
fn local_response_loss_after_send_keeps_mutation_outcome_unknown() {
    let home = Home::new();
    let socket_directory = home.0.join("control");
    fs::create_dir(&socket_directory).unwrap();
    fs::set_permissions(&socket_directory, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = socket_directory.join("c");
    let server = closing_control_server(&socket);
    let output = home
        .command(&[
            "--socket",
            socket.to_str().unwrap(),
            "--timeout",
            "1000",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            &target_token(),
            "--in",
            "5m",
            "--message",
            "response lost after send",
        ])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout["outcome_unknown"], true);
    assert_eq!(stdout["error"]["code"], "local_transport");

    let journal_path = home.0.join(".config/zmux/schedule-requests/requests.json");
    let journal: Value = serde_json::from_slice(&fs::read(journal_path).unwrap()).unwrap();
    let requests = journal["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["request_id"], stdout["request_id"]);
    assert_eq!(requests[0]["state"], "outcome_unknown");
}

#[test]
fn raw_tmux_pane_ids_are_rejected_before_control_socket_access() {
    let home = Home::new();
    let output = home
        .command(&[
            "--socket",
            "/no/such/zmux/control.sock",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            "%17",
            "--in",
            "5m",
            "--message",
            "private body",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("raw tmux pane IDs are not accepted"));
    assert!(!error.contains("cannot connect"));
}

#[test]
fn message_bodies_are_omitted_from_journal_diagnostics_and_json_errors() {
    let home = Home::new();
    let socket_dir = home.0.join("control");
    fs::create_dir(&socket_dir).unwrap();
    fs::set_permissions(&socket_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = socket_dir.join("c");
    let secret = "DO_NOT_LEAK_SCHEDULE_BODY_7e0ef3";
    let server = echoing_control_server(&socket, secret);
    let output = home
        .command(&[
            "--socket",
            socket.to_str().unwrap(),
            "--timeout",
            "1000",
            "schedule",
            "create",
            "--surface",
            SURFACE,
            "--remote-pane",
            &target_token(),
            "--in",
            "5m",
            "--message",
            secret,
        ])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(secret));
    assert!(!stderr.contains(secret));
    let response: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(response["error"]["message"], "[message text omitted]");
    assert_eq!(response["outcome_unknown"], true);

    let journal_path = home.0.join(".config/zmux/schedule-requests/requests.json");
    let bytes = fs::read(&journal_path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(secret));
    let journal: Value = serde_json::from_slice(&bytes).unwrap();
    let request = &journal["requests"][0];
    assert_eq!(request["operation"], "create");
    assert_eq!(request["state"], "outcome_unknown");
    assert_eq!(
        fs::metadata(&journal_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
