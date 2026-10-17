use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

fn socket_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    std::env::temp_dir()
        .join(format!(
            "zl{:x}{nonce:x}{:x}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
        .join("c")
}

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args(args)
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap()
}

fn invoke_app(socket: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(socket)
        .arg("--timeout")
        .arg("5000")
        .args(args)
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "zmux {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn id(value: &Value, key: &str) -> String {
    value[key].as_u64().unwrap().to_string()
}

#[test]
fn help_and_version_are_complete_and_offline() {
    let mut help = None;
    for arguments in [
        Vec::<&str>::new(),
        vec!["--help"],
        vec!["--timeout", "100"],
        vec!["--socket", "relative-path-must-not-be-read"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(arguments)
            .env("ZMUX_SOCKET", "relative-path-must-not-be-read")
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.contains("Usage: zmux"));
        help.get_or_insert(output);
    }
    let help = help.unwrap();
    for command in [
        "doctor",
        "ssh",
        "status",
        "list-workspaces",
        "list-panes",
        "new-workspace",
        "rename-workspace",
        "focus-pane",
        "split-pane",
        "close-pane",
        "browser",
        "new-surface",
        "list-pane-surfaces",
        "select-surface",
        "close-surface",
        "rename-tab",
        "reorder-surface",
        "move-surface",
        "close-other-tabs",
        "close-workspace",
        "reopen-closed",
        "recently-closed",
        "notify",
        "list-notifications",
        "mark-notification-read",
        "open-notification",
        "dismiss-notification",
        "clear-notifications",
        "send-text",
        "read-screen",
        "open",
        "markdown",
        "prototype",
        "help",
    ] {
        assert!(help.contains(command), "top-level help omitted {command}");
        let output = invoke(&[command, "--help"]);
        assert!(
            output.status.success(),
            "{command} --help failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    for (args, expected) in [
        (
            vec!["rename-workspace", "--bogus"],
            "unknown rename-workspace option",
        ),
        (
            vec!["focus-pane", "--pane", "1", "--pane", "2"],
            "repeated --pane",
        ),
        (vec!["focus-pane", "--pane", "pane:0"], "positive ID"),
        (vec!["ssh", "example", "--port", "0"], "SSH port"),
        (vec!["ssh", "example", "-A", "-a"], "conflicting agent"),
        (
            vec!["ssh", "example", "--command", "uname", "--", "pwd"],
            "conflicts",
        ),
        (
            vec!["notify", "--title", "t", "--body", "", "--body", "again"],
            "invalid or repeated notify option",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-socket-must-not-be-used"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "parser did not reject before connecting: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let version = invoke(&["--version"]);
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .starts_with("zmux ")
    );
}

#[test]
fn offline_topics_and_agent_discovery_do_not_require_a_valid_socket() {
    let offline = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-path-must-not-be-used"])
            .args(args)
            .env("ZMUX_SOCKET", "relative-path-must-not-be-used")
            .output()
            .unwrap()
    };
    let split_help = offline(&["help", "new-split"]);
    assert!(split_help.status.success());
    let split_help = String::from_utf8(split_help.stdout).unwrap();
    assert!(split_help.contains("--focus true|false|--no-focus"));
    assert!(split_help.contains("created_panes"));
    let history_help = offline(&["focus-history", "--help"]);
    assert!(history_help.status.success());
    let history_help = String::from_utf8(history_help.stdout).unwrap();
    assert!(history_help.contains("<back|forward>"));
    assert!(!history_help.contains("--workspace"));
    let topic = offline(&["help", "navigate"]);
    assert!(topic.status.success());
    let topic = String::from_utf8(topic.stdout).unwrap();
    assert!(topic.contains("focus-pane"));
    assert!(topic.contains("move-surface"));
    assert!(!topic.contains("zmux prototype"));

    let guide = offline(&["guide"]);
    let alias = offline(&["--skill"]);
    assert!(guide.status.success());
    assert!(alias.status.success());
    assert_eq!(guide.stdout, alias.stdout);
    let guide = String::from_utf8(guide.stdout).unwrap();
    assert!(guide.contains("Never retry a timeout"));
    assert!(guide.contains("--expect-revision"));

    let catalog = offline(&["commands"]);
    assert!(catalog.status.success());
    let catalog: Value = serde_json::from_slice(&catalog.stdout).unwrap();
    assert_eq!(catalog["schema_version"], 1);
    assert_eq!(catalog["parity_status"], "incomplete");
    let guide = catalog["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "guide")
        .unwrap();
    assert_eq!(guide["socket_required"], false);
    assert_eq!(guide["aliases"], serde_json::json!(["--skill"]));
    assert_eq!(catalog["contract_schema_version"], 1);
    let contracts = catalog["contracts"].as_array().unwrap();
    let split = contracts
        .iter()
        .find(|leaf| leaf["name"] == "new-split")
        .unwrap();
    assert_eq!(split["parser_defaults"]["focus"], false);
    assert_eq!(split["output"]["schema_status"], "partial");
    let open = contracts
        .iter()
        .find(|leaf| leaf["name"] == "browser open")
        .unwrap();
    assert_eq!(open["parser_defaults"]["focus"], true);
    let url = contracts
        .iter()
        .find(|leaf| leaf["name"] == "browser get url")
        .unwrap();
    assert!(
        !url["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "selector")
    );
    let wait = contracts
        .iter()
        .find(|leaf| leaf["name"] == "browser download wait")
        .unwrap();
    assert_eq!(wait["parser_defaults"]["wait_timeout_ms"], 10_000);
    assert_eq!(
        catalog["error_contract"]["machine_readable_error_json"],
        false
    );
    let frozen = &catalog["frozen_coverage"];
    assert_eq!(frozen["behavioral_parity_verified"], false);
    let entries = frozen["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 428);
    {
        let spelling = "new-window";
        let row = entries
            .iter()
            .find(|row| row["reference_command"] == spelling)
            .unwrap();
        assert_eq!(row["status"], "unresolved");
        assert!(row["local_contract"].is_null());
        assert!(!contracts.iter().any(|leaf| leaf["name"] == spelling));
    }
    for (id, name) in [
        ("CMD-245", "browser dblclick"),
        ("CMD-246", "browser hover"),
        ("CMD-247", "browser focus"),
        ("CMD-248", "browser check"),
        ("CMD-249", "browser uncheck"),
        ("CMD-250", "browser scroll-into-view"),
        ("CMD-251", "browser type"),
        ("CMD-252", "browser fill"),
        ("CMD-253", "browser press"),
        ("CMD-254", "browser keydown"),
        ("CMD-255", "browser keyup"),
        ("CMD-256", "browser select"),
        ("CMD-257", "browser scroll"),
    ] {
        let row = entries
            .iter()
            .find(|row| row["inventory_id"] == id)
            .unwrap();
        assert_eq!(row["status"], "local_counterpart_compatibility_unverified");
        assert_eq!(row["local_contract"], name);
        assert!(
            contracts
                .iter()
                .any(|leaf| leaf["name"] == name && leaf["contract_status"] == "partial")
        );
    }
    // `open` and `markdown open` map to bounded local leaves; behavioural parity stays false.
    for (id, spelling, name, force) in [
        ("CMD-006", "open", "open", Value::Null),
        (
            "CMD-114",
            "markdown open",
            "markdown open",
            "markdown".into(),
        ),
    ] {
        let row = entries
            .iter()
            .find(|row| row["inventory_id"] == id)
            .unwrap();
        assert_eq!(row["reference_command"], spelling);
        assert_eq!(row["status"], "local_counterpart_compatibility_unverified");
        assert_eq!(row["local_contract"], name);
        let leaf = contracts
            .iter()
            .find(|leaf| leaf["name"] == name)
            .unwrap_or_else(|| panic!("no contract leaf {name}"));
        assert_eq!(leaf["contract_status"], "partial");
        assert_eq!(
            leaf["dispatcher"],
            if name == "open" { "open" } else { "markdown" }
        );
        assert_eq!(leaf["parser_defaults"]["focus"], true);
        assert_eq!(leaf["parser_defaults"]["client_timeout_ms"], 5000);
        assert_eq!(
            leaf["parser_probe"]["request"]["request"]["operation"]["operation"],
            "open_items"
        );
        assert_eq!(
            leaf["parser_probe"]["request"]["request"]["operation"]["force_mode"],
            force
        );
        let arguments: Vec<_> = leaf["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|argument| argument["name"].as_str().unwrap())
            .collect();
        for expected in [
            "window",
            "workspace",
            "pane",
            "surface",
            "focus",
            "no_focus",
            "operand",
        ] {
            assert!(arguments.contains(&expected), "{name} omits {expected}");
        }
    }
    let click = entries
        .iter()
        .find(|row| row["inventory_id"] == "CMD-244")
        .unwrap();
    assert_eq!(
        click["status"],
        "local_counterpart_compatibility_unverified"
    );
    assert_eq!(click["local_contract"], "browser click");
    assert!(
        contracts
            .iter()
            .any(|leaf| leaf["name"] == "browser click" && leaf["contract_status"] == "partial")
    );
    let checked = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args([
            "--socket",
            "relative-must-not-be-used",
            "commands",
            "--check",
        ])
        .env("ZMUX_LOCAL_TMUX", "1")
        .env_remove("TMUX")
        .env("ZMUX_SURFACE_ID", "surface:0")
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["status"], "passed");
    assert_eq!(checked["inventory_entries"], 428);
    assert_eq!(checked["compatibility_verified"], false);

    let version = offline(&["version"]);
    assert!(version.status.success());
    assert_eq!(version.stdout, offline(&["--version"]).stdout);
    for args in [
        vec!["guide", "unexpected"],
        vec!["help", "nonexistent-topic"],
        vec!["browser", "bogus", "--help"],
        vec!["browser", "help", "unexpected"],
        vec!["commands", "--check", "unexpected"],
    ] {
        let rejected = offline(&args);
        assert!(!rejected.status.success());
        assert!(!String::from_utf8_lossy(&rejected.stderr).contains("cannot connect"));
    }
}

fn capture_request(listener: UnixListener, version: u32) -> thread::JoinHandle<Value> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut body = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut body).unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        let response = serde_json::to_vec(&serde_json::json!({
            "version": version, "request_id": request["request_id"], "ok": true, "result": {}
        }))
        .unwrap();
        stream
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&response).unwrap();
        request
    })
}

#[test]
fn send_text_delimiter_preserves_literal_stdin_marker_on_the_wire() {
    use std::process::Stdio;
    for (arguments, expected) in [
        (vec!["send-text", "--", "--stdin"], "--stdin"),
        (vec!["send-text", "--stdin"], "input from stdin"),
    ] {
        let socket = socket_path();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let capture = capture_request(listener, 2);
        let mut child = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(&socket)
            .args(arguments)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"input from stdin")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let request = capture.join().unwrap();
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
        assert_eq!(request["command"]["request"]["operation"]["text"], expected);
    }
}

#[test]
fn incompatible_app_protocol_reports_an_upgrade_without_retrying_the_command() {
    let socket = socket_path();
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let captured = capture_request(listener, 1);
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "--password",
            "synthetic-test-only",
            "new-workspace",
            "Protocol skew",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("requires v2"));
    assert!(error.contains("Restart or update"));
    assert!(error.contains("not retried"));
    assert!(!error.contains("synthetic-test-only"));
    let request = captured.join().unwrap();
    assert_eq!(request["version"], 2);
    assert_eq!(request["command"]["method"], "new_workspace");
    fs::remove_file(&socket).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}

#[test]
fn socket_deadline_spans_fragmented_header_body_and_response_stages_without_replay() {
    let mut unexpected = Vec::new();
    for scenario in ["ordinary", "slow_header", "slow_body", "split_stages"] {
        let socket = socket_path();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let response = serde_json::to_vec(&serde_json::json!({
                "version":2,"request_id":request["request_id"],"ok":true,
                "result":{"transport":"complete"}
            }))
            .unwrap();
            let header = (response.len() as u32).to_be_bytes();
            let pause = Duration::from_millis(120);
            let packets: Vec<(Duration, Vec<u8>)> = match scenario {
                "slow_header" => header
                    .iter()
                    .enumerate()
                    .map(|(index, byte)| {
                        (if index == 0 { Duration::ZERO } else { pause }, vec![*byte])
                    })
                    .chain(std::iter::once((Duration::ZERO, response)))
                    .collect(),
                "slow_body" => std::iter::once((Duration::ZERO, header.to_vec()))
                    .chain(response.chunks(response.len().div_ceil(4)).enumerate().map(
                        |(index, chunk)| {
                            (
                                if index == 0 { Duration::ZERO } else { pause },
                                chunk.to_vec(),
                            )
                        },
                    ))
                    .collect(),
                "split_stages" => vec![
                    (Duration::from_millis(180), header.to_vec()),
                    (Duration::from_millis(180), response),
                ],
                _ => vec![
                    (Duration::ZERO, header[..2].to_vec()),
                    (Duration::from_millis(10), header[2..].to_vec()),
                    (Duration::from_millis(10), response),
                ],
            };
            for (delay, bytes) in packets {
                thread::sleep(delay);
                if let Err(error) = stream.write_all(&bytes) {
                    assert!(
                        matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::NotConnected
                        ),
                        "{error}"
                    );
                    break;
                }
            }
            (listener, request)
        });
        let started = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(&socket)
            .args(["--timeout", "300", "new-workspace", "One admission"])
            .output()
            .unwrap();
        let elapsed = started.elapsed();
        let (listener, request) = peer.join().unwrap();
        assert_eq!(request["command"]["method"], "new_workspace");
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "CLI replayed a mutation"
        );
        if scenario == "ordinary" {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap()["transport"],
                "complete"
            );
        } else if output.status.success() {
            unexpected.push(format!(
                "{scenario} exceeded the total deadline but succeeded after {elapsed:?}"
            ));
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            // A peer write does not prove the client consumed that stage before
            // its deadline: scheduling may expire the same total budget while
            // waiting for either the header or body. Assert the deadline contract,
            // rather than which read happened to observe its expiration.
            let response_phase =
                error.contains("waiting for zmux") || error.contains("reading zmux response");
            assert!(
                response_phase
                    && error.contains("request deadline elapsed")
                    && error.contains("not retried"),
                "{scenario}: {error}"
            );
            assert!(output.stdout.is_empty());
            // Do not bound Command::output wall time: it includes process startup,
            // scheduler suspension and exit collection, outside the client's budget.
            // A per-read/per-stage deadline reset is detected above by the late
            // complete response succeeding instead of this timeout failure.
        }
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("; "));
}

#[test]
fn a_large_valid_response_with_an_ample_deadline_is_rendered_completely() {
    // A valid response below the public byte limit. This exercises public-wire
    // acceptance and rendering without assumptions about a runner's CPU speed.
    let values = "0,".repeat(469_999) + "0";
    let socket = socket_path();
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: Value = serde_json::from_slice(&bytes).unwrap();
        let response = format!(
            "{{\"version\":2,\"request_id\":{},\"ok\":true,\"result\":[{values}]}}",
            request["request_id"]
        );
        assert!(response.len() < zmux_desktop_runtime::control::MAX_CONTROL_RESPONSE_BYTES);
        stream
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(response.as_bytes()).unwrap();
        listener
    });
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["--timeout", "5000", "status"])
        .output()
        .unwrap();
    let listener = peer.join().unwrap();
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        470_000
    );
    fs::remove_file(&socket).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}

#[test]
fn peer_close_accepts_a_complete_buffered_frame_and_rejects_a_truncated_frame() {
    for truncate in [false, true] {
        let socket = socket_path();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut length = [0; 4];
            stream.read_exact(&mut length).unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
            stream.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let response = serde_json::to_vec(&serde_json::json!({
                "version":2,"request_id":request["request_id"],"ok":true,
                "result":{"reply":"buffered before peer close"}
            }))
            .unwrap();
            let mut frame = (response.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&response);
            if truncate {
                frame.pop();
            }
            stream.write_all(&frame).unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
            drop(stream);
            listener
        });
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(&socket)
            .args(["--timeout", "2000", "new-workspace", "One admission"])
            .output()
            .unwrap();
        let listener = peer.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "CLI replayed a mutation after peer close"
        );
        if truncate {
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("incomplete zmux response") && error.contains("not retried"),
                "{error}"
            );
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap()["reply"],
                "buffered before peer close"
            );
        }
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
    }
}

#[test]
fn local_socket_credentials_and_live_access_changes_protect_real_mutations() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy, SocketPassword};
    let socket = socket_path();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let secret = "test-only socket credential";
    let policy =
        |access| ControlPolicy::password(access, SocketPassword::new(secret.into()).unwrap());
    let server = ControlServer::start_with_policy(
        &socket,
        Arc::clone(&desktop),
        policy(ControlAccess::Full),
    )
    .unwrap();
    let private_home = socket.parent().unwrap().join("home");
    let saved = private_home.join(".config/zmux/socket-password");
    fs::create_dir_all(saved.parent().unwrap()).unwrap();
    fs::write(&saved, format!("{secret}\n")).unwrap();
    fs::set_permissions(&saved, fs::Permissions::from_mode(0o600)).unwrap();
    let run = |args: &[&str], environment: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command
            .arg("--socket")
            .arg(&socket)
            .args(args)
            .env("HOME", &private_home)
            .env_remove("ZMUX_SOCKET_PASSWORD_FILE")
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID");
        if let Some(value) = environment {
            command.env("ZMUX_SOCKET_PASSWORD", value);
        }
        command.output().unwrap()
    };
    // A private saved file works without a flag or environment credential.
    let deadline = Instant::now() + Duration::from_secs(5);
    let initial: Value = loop {
        let output = run(&["status"], None);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        if status["workspace_count"] == 1 {
            break status;
        }
        assert!(Instant::now() < deadline, "initial workspace not ready");
        thread::sleep(Duration::from_millis(10));
    };
    // Explicit credentials override environment; environment overrides saved state.
    assert!(
        run(&["--password", secret, "status"], Some("wrong env"))
            .status
            .success()
    );
    for output in [
        run(
            &["--password", "wrong flag", "new-workspace", "Denied"],
            Some(secret),
        ),
        run(&["new-workspace", "Denied"], Some("wrong env")),
    ] {
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("unauthorized"));
        assert!(!error.contains(secret));
        assert!(!error.contains("wrong"));
    }
    let output = run(&["status"], None);
    let unchanged: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(initial["workspace_count"], unchanged["workspace_count"]);
    let created = run(&["new-workspace", "Authorized"], Some(secret));
    assert!(created.status.success());

    server.set_policy(policy(ControlAccess::ReadOnly)).unwrap();
    let caps = run(&["capabilities"], Some(secret));
    let caps: Value = serde_json::from_slice(&caps.stdout).unwrap();
    assert_eq!(caps["authorization"], "same_uid_and_password");
    assert_eq!(caps["access"], "read_only");
    for args in [
        vec!["new-workspace", "ReadOnlyDenied"],
        vec![
            "paste-files",
            "--",
            "/nonexistent/read-only-must-deny-before-file-IO",
        ],
        vec!["cancel-upload", "999"],
    ] {
        let denied = run(&args, Some(secret));
        assert!(!denied.status.success());
        assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    }
    assert!(run(&["list-uploads"], Some(secret)).status.success());
    let output = run(&["status"], Some(secret));
    let current: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        current["workspace_count"].as_u64().unwrap(),
        initial["workspace_count"].as_u64().unwrap() + 1
    );

    fs::set_permissions(&saved, fs::Permissions::from_mode(0o644)).unwrap();
    let insecure = run(&["status"], None);
    assert!(!insecure.status.success());
    assert!(String::from_utf8_lossy(&insecure.stderr).contains("private"));
    // A deliberately provided credential still wins over an insecure fallback file.
    assert!(run(&["status"], Some(secret)).status.success());
    fs::set_permissions(&saved, fs::Permissions::from_mode(0o600)).unwrap();
    let target = saved.with_extension("real");
    fs::rename(&saved, &target).unwrap();
    std::os::unix::fs::symlink(&target, &saved).unwrap();
    assert!(!run(&["status"], None).status.success());
    server
        .set_policy(ControlPolicy::same_user(ControlAccess::Disabled))
        .unwrap();
    let disabled = run(&["status"], Some(secret));
    assert!(!disabled.status.success());
    assert!(String::from_utf8_lossy(&disabled.stderr).contains("disabled"));

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir_all(socket.parent().unwrap()).unwrap();
}

#[test]
fn standalone_terminals_do_not_inherit_socket_passwords() {
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args([
            "prototype",
            "--",
            "/bin/sh",
            "-c",
            "printf 'socket-secret=%s' \"${ZMUX_SOCKET_PASSWORD-unset}\"",
        ])
        .env("ZMUX_SOCKET_PASSWORD", "test-only-do-not-inherit")
        .output()
        .unwrap();
    assert!(output.status.success());
    let screen: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = screen.to_string();
    assert!(text.contains("socket-secret=unset"), "{text}");
    assert!(!text.contains("test-only-do-not-inherit"));
}

#[test]
fn public_cli_controls_a_real_desktop_over_its_unix_socket() {
    let socket = socket_path();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    assert_eq!(
        fs::metadata(socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let startup_deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        let status = invoke_app(&socket, &["status"]);
        if status["workspace_count"] == 1 {
            break status;
        }
        assert!(
            Instant::now() < startup_deadline,
            "desktop never created its initial workspace"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status["workspace_count"], 1);
    assert_eq!(status["protocol_version"], 2);
    let capabilities = invoke_app(&socket, &["capabilities"]);
    assert_eq!(capabilities["protocol"], "zmux-local-control");
    assert_eq!(capabilities["version"], status["protocol_version"]);
    assert_eq!(capabilities["model_version"], status["model_version"]);
    assert_eq!(capabilities["authorization"], "same_uid");
    assert_eq!(capabilities["remote_relay"], false);
    for _ in 0..20 {
        let workspaces = invoke_app(&socket, &["list-workspaces"]);
        assert_eq!(workspaces["workspaces"].as_array().unwrap().len(), 1);
    }

    let rejected = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", "999999", "Missing"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("target_not_found")
    );

    let initial_catalog = invoke_app(&socket, &["list-workspaces"]);
    let initial_workspace_uuid = initial_catalog["targets"][0]["uuid"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = invoke_app(&socket, &["new-workspace", "CLI Workspace"]);
    let workspace_id = id(&created, "focused_workspace");
    let workspace_uuid = created["focused_workspace_uuid"]
        .as_str()
        .expect("created workspace must expose its stable UUID")
        .to_owned();
    let pane_uuid = created["focused_pane_uuid"]
        .as_str()
        .expect("created pane must expose its stable UUID")
        .to_owned();
    invoke_app(&socket, &["rename-workspace", &workspace_uuid, "By UUID"]);
    let wrong_kind = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", &pane_uuid, "Wrong target"])
        .output()
        .unwrap();
    assert!(!wrong_kind.status.success());
    assert!(String::from_utf8_lossy(&wrong_kind.stderr).contains("wrong_target_kind"));
    invoke_app(
        &socket,
        &[
            "rename-workspace",
            &format!("workspace:{workspace_id}"),
            "CLI Workspace",
        ],
    );
    let original_pane = id(&created, "focused_pane");
    let via_index = invoke_app(&socket, &["list-panes", "--workspace", "index:2"]);
    assert_eq!(id(&via_index["panes"][0], "pane_id"), original_pane);
    let inherited = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", "Caller workspace"])
        .env("ZMUX_WORKSPACE_ID", &initial_workspace_uuid)
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap();
    assert!(
        inherited.status.success(),
        "{}",
        String::from_utf8_lossy(&inherited.stderr)
    );
    let stale = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "rename-workspace",
            "--expect-revision",
            "18446744073709551615",
            &workspace_uuid,
            "Stale rename",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale_revision"));
    let foreign = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "focus-pane",
            "--workspace",
            &initial_workspace_uuid,
            &pane_uuid,
        ])
        .output()
        .unwrap();
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("target_scope_mismatch"));

    let workspaces = invoke_app(&socket, &["list-workspaces"]);
    assert!(
        workspaces["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["id"].as_u64().unwrap().to_string() == workspace_id
                    && item["name"] == "CLI Workspace"
            })
    );

    invoke_app(
        &socket,
        &["rename-workspace", &workspace_id, "Renamed by CLI"],
    );
    let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
    assert_eq!(panes["panes"].as_array().unwrap().len(), 1);

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
        if panes["panes"][0]["state"] == "running" {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "CLI pane never became ready"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let created_tab = invoke_app(&socket, &["new-surface", &original_pane]);
    let tab_id = id(&created_tab, "focused_surface");
    invoke_app(&socket, &["rename-tab", &tab_id, "CLI Tab"]);
    let tabs = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    let tabs = tabs["surfaces"].as_array().unwrap();
    assert_eq!(tabs.len(), 2);
    assert!(tabs.iter().any(
        |tab| tab["surface_id"].as_u64().unwrap().to_string() == tab_id
            && tab["title"] == "CLI Tab"
            && tab["selected"] == true
    ));
    let first_tab_id = id(&tabs[0], "surface_id");
    invoke_app(
        &socket,
        &[
            "notify",
            "--surface",
            &tab_id,
            "--title",
            "CLI attention",
            "--body",
            "Review requested",
        ],
    );
    let notifications = invoke_app(&socket, &["list-notifications"]);
    let notification_id = id(&notifications["notifications"][0], "id");
    assert_eq!(
        notifications["notifications"][0]["body"],
        "Review requested"
    );
    assert_eq!(notifications["unread"], 1);
    invoke_app(&socket, &["open-notification", "--id", &notification_id]);
    let notifications = invoke_app(&socket, &["list-notifications"]);
    assert_eq!(notifications["unread"], 0);
    invoke_app(&socket, &["dismiss-notification", "--id", &notification_id]);
    assert!(
        invoke_app(&socket, &["list-notifications"])["notifications"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    invoke_app(&socket, &["reorder-surface", &tab_id, "0"]);
    let reordered = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    assert_eq!(id(&reordered["surfaces"][0], "surface_id"), tab_id);
    invoke_app(&socket, &["select-surface", &first_tab_id]);
    invoke_app(&socket, &["close-surface", &tab_id]);
    let closed = invoke_app(&socket, &["recently-closed"]);
    assert_eq!(closed["closed_items"].as_array().unwrap().len(), 1);
    let history_id = id(&closed["closed_items"][0], "id");
    invoke_app(&socket, &["reopen-closed", &history_id]);
    let restore_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let closed = invoke_app(&socket, &["recently-closed"]);
        if closed["closed_items"].as_array().unwrap().is_empty() {
            break;
        }
        assert!(Instant::now() < restore_deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let restored = invoke_app(&socket, &["list-pane-surfaces", &original_pane]);
    assert_eq!(restored["surfaces"].as_array().unwrap().len(), 2);
    let restored_surface = restored["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| id(surface, "surface_id") != first_tab_id)
        .expect("reopening creates a fresh tab alongside the surviving original");
    let restored_id = id(restored_surface, "surface_id");
    assert_ne!(restored_id, tab_id);
    invoke_app(&socket, &["close-surface", &restored_id]);

    let split = invoke_app(&socket, &["split-pane", &original_pane, "horizontal"]);
    let new_pane = id(&split, "focused_pane");
    invoke_app(&socket, &["focus-pane", &original_pane]);
    invoke_app(
        &socket,
        &[
            "send-text",
            &original_pane,
            "printf '\\nZMUX_CLI_SOCKET_OK\\n'\r",
        ],
    );

    invoke_app(
        &socket,
        &[
            "send-text",
            &original_pane,
            "printf '\\nCONTEXT_W=%s\\nCONTEXT_S=%s\\n' \"$ZMUX_WORKSPACE_ID\" \"$ZMUX_SURFACE_ID\"\r",
        ],
    );
    let output_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = invoke_app(&socket, &["read-screen", &original_pane]);
        if screen["contents"].as_str().is_some_and(|contents| {
            contents.contains("ZMUX_CLI_SOCKET_OK")
                && contents.contains(&format!("CONTEXT_W={workspace_uuid}"))
                && contents.contains(&format!(
                    "CONTEXT_S={}",
                    created["focused_surface_uuid"].as_str().unwrap()
                ))
        }) {
            break;
        }
        assert!(
            Instant::now() < output_deadline,
            "CLI input/context did not reach the real terminal: {screen}; expected workspace {workspace_uuid}, surface {}",
            created["focused_surface_uuid"]
        );
        thread::sleep(Duration::from_millis(10));
    }

    let browser = invoke_app(&socket, &["browser", "open", &new_pane, "about:blank"]);
    let browser_id = id(&browser, "focused_surface");
    let surfaces = invoke_app(&socket, &["list-pane-surfaces", &new_pane]);
    let browser = surfaces["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "browser")
        .unwrap();
    assert!(browser["session_id"].is_null());
    invoke_app(
        &socket,
        &["browser", "navigate", &browser_id, "https://example.com/"],
    );
    let surfaces = invoke_app(&socket, &["list-pane-surfaces", &new_pane]);
    assert!(
        surfaces["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["url"] == "https://example.com/")
    );
    let browser_uuid = browser["surface_uuid"].as_str().unwrap();
    for command in [
        serde_json::json!({"method":"send_text", "pane_id":new_pane.parse::<u64>().unwrap(), "text":"must not reach a browser"}),
        serde_json::json!({"method":"targeted", "request": {"scope":{"surface":browser_uuid}, "operation":{"operation":"send_text","text":"must not reach a browser"}}}),
    ] {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"request_id":90001,"command":command}),
        )
        .unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let mut body = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut body).unwrap();
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(
            response["error"]["code"], "not_terminal",
            "browser rejection lost its typed error: {response}"
        );
    }
    let rejected_browser_input = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "send-text",
            "--surface",
            browser_uuid,
            "must not reach a browser",
        ])
        .output()
        .unwrap();
    assert!(!rejected_browser_input.status.success());
    assert!(String::from_utf8_lossy(&rejected_browser_input.stderr).contains("not_terminal"));
    invoke_app(&socket, &["close-pane", &new_pane]);
    let panes = invoke_app(&socket, &["list-panes", "--workspace", &workspace_id]);
    assert_eq!(panes["panes"].as_array().unwrap().len(), 1);

    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let replacement = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let replacement_server = ControlServer::start(&socket, Arc::clone(&replacement)).unwrap();
    let replaced = invoke_app(&socket, &["list-workspaces"]);
    // Startup is asynchronous; wait for the production owner's initial workspace.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut replaced = replaced;
    while replaced["workspaces"].as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
        replaced = invoke_app(&socket, &["list-workspaces"]);
    }
    assert_eq!(replaced["workspaces"][0]["id"], 1);
    let stale_uuid = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["rename-workspace", &initial_workspace_uuid, "Wrong app"])
        .output()
        .unwrap();
    assert!(!stale_uuid.status.success());
    assert!(String::from_utf8_lossy(&stale_uuid.stderr).contains("target_not_found"));
    replacement_server.shutdown(Duration::from_secs(2)).unwrap();
    replacement.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}

#[test]
fn navigation_parser_preserves_direction_focus_scope_and_global_history_on_the_wire() {
    // This boundary protects CLI grammar and protocol mapping; geometry/history behavior
    // is covered by workspace-core's own navigation tests.
    let cases = [
        (
            vec![
                "new-split",
                "left",
                "--workspace",
                "workspace:1",
                "--surface",
                "tab:2",
            ],
            serde_json::json!({"action":"split_pane_directional","direction":"left","focus":false}),
        ),
        (
            vec!["new-split", "--pane", "pane:3", "right", "--focus", "true"],
            serde_json::json!({"action":"split_pane_directional","direction":"right","focus":true}),
        ),
        (
            vec!["new-split", "up", "--no-focus"],
            serde_json::json!({"action":"split_pane_directional","direction":"up","focus":false}),
        ),
        (
            vec!["new-split", "--focus", "false", "down"],
            serde_json::json!({"action":"split_pane_directional","direction":"down","focus":false}),
        ),
        (
            vec!["select-workspace", "workspace:4"],
            serde_json::json!({"action":"select_workspace"}),
        ),
        (
            vec!["next-window", "--workspace", "workspace:4"],
            serde_json::json!({"action":"navigate_workspace","direction":"next"}),
        ),
        (
            vec!["previous-window"],
            serde_json::json!({"action":"navigate_workspace","direction":"previous"}),
        ),
        (
            vec!["last-window"],
            serde_json::json!({"action":"navigate_workspace","direction":"last"}),
        ),
        (
            vec!["next-tab", "--tab", "tab:2"],
            serde_json::json!({"action":"navigate_surface","direction":"next"}),
        ),
        (
            vec!["previous-tab"],
            serde_json::json!({"action":"navigate_surface","direction":"previous"}),
        ),
        (
            vec!["last-tab"],
            serde_json::json!({"action":"navigate_surface","direction":"last"}),
        ),
        (
            vec!["next-pane"],
            serde_json::json!({"action":"navigate_pane","direction":"next"}),
        ),
        (
            vec!["previous-pane"],
            serde_json::json!({"action":"navigate_pane","direction":"previous"}),
        ),
        (
            vec!["last-pane", "--workspace", "workspace:4"],
            serde_json::json!({"action":"navigate_pane","direction":"last"}),
        ),
        (
            vec!["focus-direction", "left", "--pane", "pane:3"],
            serde_json::json!({"action":"focus_pane_direction","direction":"left"}),
        ),
        (
            vec!["tab-action", "--tab", "tab:2", "rename", "build", "logs"],
            serde_json::json!({"action":"rename_surface","name":"build logs"}),
        ),
        (
            vec![
                "tab-action",
                "--action",
                "rename",
                "--title",
                "--literal-title",
            ],
            serde_json::json!({"action":"rename_surface","name":"--literal-title"}),
        ),
        (
            vec!["tab-action", "--action", "close_others"],
            serde_json::json!({"action":"close_other_surfaces"}),
        ),
        (
            vec!["focus-history", "back"],
            serde_json::json!({"action":"navigate_focus","direction":"back"}),
        ),
        (
            vec!["focus-history", "forward"],
            serde_json::json!({"action":"navigate_focus","direction":"forward"}),
        ),
    ];
    for (arguments, expected) in cases {
        let socket = socket_path();
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let capture = capture_request(listener, 2);
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("--socket")
            .arg(&socket)
            .args(&arguments)
            .env("ZMUX_WORKSPACE_ID", "workspace:99")
            .env("ZMUX_SURFACE_ID", "surface:98")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let command = capture.join().unwrap()["command"].clone();
        if arguments[0] == "focus-history" {
            assert_eq!(
                command,
                serde_json::json!({"method":"action", "action":expected})
            );
        } else {
            let request = &command["request"];
            assert_eq!(request["operation"]["action"], expected, "{arguments:?}");
            let explicitly_scoped = arguments
                .iter()
                .any(|arg| ["--workspace", "--pane", "--surface", "--tab"].contains(arg))
                || arguments[0] == "select-workspace";
            assert_eq!(
                request["caller"]["workspace"].is_null(),
                explicitly_scoped,
                "{arguments:?}"
            );
            if arguments[0] == "select-workspace" || arguments.contains(&"--workspace") {
                let expected_workspace = if arguments[0] == "new-split" {
                    "workspace:1"
                } else {
                    "workspace:4"
                };
                assert_eq!(
                    request["scope"]["workspace"], expected_workspace,
                    "{arguments:?}"
                );
            }
            if arguments.contains(&"--tab") || arguments.contains(&"--surface") {
                assert_eq!(request["scope"]["surface"], "tab:2", "{arguments:?}");
            }
            if arguments.contains(&"--pane") {
                assert_eq!(request["scope"]["pane"], "pane:3", "{arguments:?}");
            }
        }
        fs::remove_file(&socket).unwrap();
        fs::remove_dir(socket.parent().unwrap()).unwrap();
    }
}

#[test]
fn navigation_rejects_unsupported_or_ambiguous_options_before_connecting() {
    for (arguments, expected) in [
        (vec!["new-split", "diagonal"], "direction must be"),
        (vec!["new-split", "left", "right"], "expected one direction"),
        (
            vec!["new-split", "right", "--focus", "yes"],
            "--focus requires true or false",
        ),
        (
            vec!["new-split", "right", "--focus", "true", "--no-focus"],
            "conflicting focus",
        ),
        (vec!["new-split", "right", "--focus"], "requires a value"),
        (
            vec!["new-split", "right", "--command", "pwd"],
            "unknown new-split option",
        ),
        (
            vec!["new-split", "right", "--surface", "tab:2", "--tab", "tab:3"],
            "repeated --tab selector",
        ),
        (
            vec![
                "select-workspace",
                "--workspace",
                "workspace:1",
                "workspace:2",
            ],
            "do not repeat",
        ),
        (
            vec![
                "next-workspace",
                "--window",
                "window:1",
                "--window",
                "window:2",
            ],
            "repeated --window selector",
        ),
        (
            vec!["next-tab", "--focus", "true"],
            "unknown next-tab option",
        ),
        (vec!["last-pane", "extra"], "accepts target options only"),
        (vec!["tab-action", "pin"], "unsupported tab-action pin"),
        (
            vec!["tab-action", "rename"],
            "requires --title or trailing title",
        ),
        (
            vec!["tab-action", "rename", "title", "--title", "other"],
            "not both",
        ),
        (
            vec!["tab-action", "close-others", "--force"],
            "unknown tab-action option",
        ),
        (
            vec!["tab-action", "close-others", "--title", "unexpected"],
            "accepts no title",
        ),
        (
            vec!["focus-history", "back", "--workspace", "workspace:1"],
            "expects 1 argument",
        ),
    ] {
        let output = invoke(&[&["--socket", "relative-must-not-be-used"][..], &arguments].concat());
        assert!(
            !output.status.success(),
            "{arguments:?} unexpectedly succeeded"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn navigation_cli_routes_live_workspace_pane_and_tab_selection_without_stealing_focus() {
    let socket = socket_path();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let a = invoke_app(&socket, &["new-workspace", "Navigation A"]);
    let b = invoke_app(&socket, &["new-workspace", "Navigation B"]);
    let a_id = a["focused_workspace_uuid"].as_str().unwrap();
    let b_id = b["focused_workspace_uuid"].as_str().unwrap();
    let b_pane = id(&b, "focused_pane");
    invoke_app(&socket, &["select-workspace", "--workspace", a_id]);
    let split = invoke_app(&socket, &["new-split", "left", "--workspace", b_id]);
    assert_eq!(split["focused_workspace"], a["focused_workspace"]);
    assert_eq!(split["focused_pane"], a["focused_pane"]);
    let panes = invoke_app(&socket, &["list-panes", "--workspace", b_id]);
    let panes = panes["panes"].as_array().unwrap();
    assert_eq!(
        panes.len(),
        2,
        "explicit workspace split must not affect the focused workspace"
    );
    let new_pane = panes
        .iter()
        .find(|pane| id(pane, "pane_id") != b_pane)
        .unwrap();
    let new_pane = id(new_pane, "pane_id");
    assert_eq!(split["created_panes"].as_array().unwrap().len(), 1);
    assert_eq!(split["created_surfaces"].as_array().unwrap().len(), 1);
    let receipt = &split["created_panes"][0];
    assert_eq!(receipt["workspace_uuid"], b_id);
    assert_eq!(receipt["pane_id"].as_u64().unwrap().to_string(), new_pane);
    assert_ne!(receipt["surface_id"], split["focused_surface"]);
    assert_eq!(split["created_surfaces"][0], *receipt);
    let created_tabs = invoke_app(&socket, &["list-pane-surfaces", &new_pane]);
    assert_eq!(
        receipt["surface_uuid"],
        created_tabs["surfaces"][0]["surface_uuid"]
    );
    let focused = invoke_app(
        &socket,
        &["new-split", "down", "--pane", &new_pane, "--focus", "true"],
    );
    assert_eq!(focused["focused_workspace"], b["focused_workspace"]);
    assert_ne!(focused["focused_pane"], new_pane.parse::<u64>().unwrap());
    let last = invoke_app(&socket, &["last-pane", "--workspace", b_id]);
    assert_eq!(last["focused_pane"], b["focused_pane"]);

    let tab = invoke_app(&socket, &["new-surface", "--pane", &b_pane]);
    let tab_id = tab["focused_surface_uuid"].as_str().unwrap();
    let previous = invoke_app(&socket, &["previous-tab", "--tab", tab_id]);
    assert_eq!(previous["focused_surface"], b["focused_surface"]);
    let next = invoke_app(&socket, &["next-tab", "--pane", &b_pane]);
    assert_eq!(next["focused_surface"], tab["focused_surface"]);
    let last = invoke_app(&socket, &["last-tab", "--pane", &b_pane]);
    assert_eq!(last["focused_surface"], b["focused_surface"]);
    invoke_app(
        &socket,
        &[
            "tab-action",
            "rename",
            "--tab",
            tab_id,
            "--title",
            "Build logs",
        ],
    );
    let tabs = invoke_app(&socket, &["list-pane-surfaces", &b_pane]);
    assert!(
        tabs["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|surface| surface["surface_uuid"] == tab_id && surface["title"] == "Build logs")
    );
    invoke_app(
        &socket,
        &["tab-action", "--action", "close-others", "--tab", tab_id],
    );
    let tabs = invoke_app(&socket, &["list-pane-surfaces", &b_pane]);
    assert_eq!(tabs["surfaces"].as_array().unwrap().len(), 1);
    assert_eq!(tabs["surfaces"][0]["surface_uuid"], tab_id);

    assert_eq!(
        invoke_app(&socket, &["previous-workspace", "--workspace", b_id])["focused_workspace"],
        a["focused_workspace"]
    );
    assert_eq!(
        invoke_app(&socket, &["next-window", "--workspace", a_id])["focused_workspace"],
        b["focused_workspace"]
    );
    assert_eq!(
        invoke_app(&socket, &["last-window"])["focused_workspace"],
        a["focused_workspace"]
    );
    let rejected = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args([
            "new-split",
            "right",
            "--workspace",
            a_id,
            "--surface",
            tab_id,
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("target_scope_mismatch"));
    let rejected = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(&socket)
        .args(["next-tab", "--tab", tab_id, "--expect-revision", "0"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("stale_revision"));
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fs::remove_dir(socket.parent().unwrap()).unwrap();
}
