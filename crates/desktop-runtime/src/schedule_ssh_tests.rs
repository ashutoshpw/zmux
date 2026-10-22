use super::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_remote_runtime::SshDescriptor;
use zmux_remote_runtime::scheduler::{
    DeadlineInput, ReceiptState, SchedulerReply, SchedulerRequest,
};

const LOOPBACK_FIXTURE_DRIVER: &str = r#"
import importlib.util
import json
from pathlib import Path
import signal
import sys
import time

configuration = Path(sys.argv[1])
diagnostic = Path(sys.argv[2])
fixture_path = Path(sys.argv[3])
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
spec = importlib.util.spec_from_file_location("native_ssh_fixture", fixture_path)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
with fixture.native_ssh_fixture(
    configuration,
    diagnostic,
    scheduler_rpc_request_log=True,
    drop_scheduler_rpc_response_after_execution=True,
):
    root = Path(json.loads(configuration.read_text())["root"])
    while not (root / ".fixture-controller-stop").exists():
        time.sleep(0.02)
"#;

#[test]
fn scheduler_rpc_uses_the_live_master_and_does_not_retry_a_lost_create_reply() {
    if !private_ssh_test_prerequisites_available() {
        eprintln!(
            "skipping managed scheduler SSH test: private loopback SSH prerequisites unavailable"
        );
        return;
    }

    let scheduler_binary = scheduler_binary();
    let fixture = PrivateSshFixture::start();
    let fixture_root = PathBuf::from(
        fixture
            .configuration
            .get("root")
            .and_then(serde_json::Value::as_str)
            .expect("private SSH fixture root"),
    );
    let state_root = fixture_root.join("scheduler-state");
    fs::create_dir(&state_root).expect("create private scheduler state");
    fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700))
        .expect("secure scheduler state");
    let tmux_socket = fixture_root.join("scheduler-tmux.sock");
    let tmux_binary = find_program("tmux").expect("tmux prerequisite was checked");
    let tmux = Command::new(&tmux_binary)
        .args(["-f", "/dev/null", "-S"])
        .arg(&tmux_socket)
        .args([
            "new-session",
            "-d",
            "-s",
            "schedule-ssh-test",
            "/bin/sleep",
            "3600",
        ])
        .status()
        .expect("start private scheduler tmux server");
    assert!(
        tmux.success(),
        "private scheduler tmux server did not start"
    );
    let tmux_server = TmuxServerGuard {
        binary: tmux_binary.clone(),
        socket: tmux_socket.clone(),
    };

    let helper_wrapper = fixture_root.join("zmux-scheduler-test-wrapper");
    let wrapper_contents = format!(
        "#!/bin/sh\nexport ZMUX_SCHEDULER_ROOT={}\nexport ZMUX_SCHEDULER_TMUX_SOCKET={}\nexport ZMUX_SCHEDULER_TMUX_BIN={}\nexec {} \"$@\"\n",
        zmux_remote_runtime::upload::quote_shell_path(state_root.to_str().unwrap()),
        zmux_remote_runtime::upload::quote_shell_path(tmux_socket.to_str().unwrap()),
        zmux_remote_runtime::upload::quote_shell_path(&tmux_binary),
        zmux_remote_runtime::upload::quote_shell_path(scheduler_binary.to_str().unwrap()),
    );
    fs::write(&helper_wrapper, wrapper_contents).expect("write private scheduler helper wrapper");
    fs::set_permissions(&helper_wrapper, fs::Permissions::from_mode(0o700))
        .expect("make scheduler helper wrapper executable");

    let daemon_log = fixture_root.join("scheduler-daemon.log");
    let daemon_output = fs::File::create(&daemon_log).expect("create scheduler daemon log");
    let daemon = Command::new(&scheduler_binary)
        .arg("serve")
        .env("ZMUX_SCHEDULER_ROOT", &state_root)
        .env("ZMUX_SCHEDULER_TMUX_SOCKET", &tmux_socket)
        .env("ZMUX_SCHEDULER_TMUX_BIN", &tmux_binary)
        .stdin(Stdio::null())
        .stdout(Stdio::from(daemon_output.try_clone().unwrap()))
        .stderr(Stdio::from(daemon_output))
        .spawn()
        .expect("start private scheduler daemon");
    let mut daemon = ChildGuard(Some(daemon));
    wait_for_scheduler_daemon(
        &scheduler_binary,
        &state_root,
        &tmux_socket,
        &tmux_binary,
        &mut daemon,
        &daemon_log,
    );

    let ssh_descriptor = SshDescriptor::builder("fixture")
        .expect("fixture destination")
        .config_file(
            fixture
                .configuration
                .get("clientConfig")
                .and_then(serde_json::Value::as_str)
                .expect("fixture SSH config"),
        )
        .identity(
            fixture
                .configuration
                .get("identity")
                .and_then(serde_json::Value::as_str)
                .expect("fixture SSH identity"),
        )
        .command(["/bin/sleep", "3600"])
        .build()
        .expect("build fixture SSH descriptor");
    let connection = Arc::new(
        ManagedConnection::create(ssh_descriptor).expect("create exact private SSH owner"),
    );
    let terminal = connection
        .terminal_spec(&zmux_session_runtime::shell_integration::OpenRequestNonce::generate());
    let mut master = ChildGuard(Some(
        Command::new(&terminal.program)
            .args(&terminal.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start private loopback SSH master"),
    ));
    wait_for_live_master(&connection, &mut master);

    let runtime = Runtime::new(Some(helper_wrapper)).expect("configure scheduler helper");
    let stopping = AtomicBool::new(false);
    let namespace = Uuid::new_v4();
    let cancellation = Arc::new(AtomicBool::new(false));
    let route = || PreparedRoute {
        identity_namespace: namespace,
        surface_id: SurfaceId(1),
        session_id: SessionId(1),
        launch_generation: 1,
        cancellation: Arc::clone(&cancellation),
        connection: Arc::clone(&connection),
    };

    let status_id = Uuid::new_v4();
    let status = runtime.execute(
        route(),
        request(
            status_id,
            SchedulerAction::Status {
                receipt_request_id: None,
            },
        ),
        &stopping,
    );
    assert!(status.ok, "live helper Status failed: {:?}", status.error);
    let Some(SchedulerResult::Success(SchedulerReply::Status { status })) =
        status.reply.map(|reply| reply.result)
    else {
        panic!("actual helper did not return Status through the existing master");
    };
    assert_eq!(status.protocol_version, SCHEDULER_PROTOCOL_VERSION);
    assert_eq!(status.store_health, StoreHealth::Healthy);

    let targets_id = Uuid::new_v4();
    let targets = runtime.execute(
        route(),
        request(targets_id, SchedulerAction::Targets),
        &stopping,
    );
    assert!(
        targets.ok,
        "live helper Targets failed: {:?}",
        targets.error
    );
    let Some(SchedulerResult::Success(SchedulerReply::Targets { targets })) =
        targets.reply.map(|reply| reply.result)
    else {
        panic!("actual helper did not return tmux targets through the existing master");
    };
    let target_id = targets
        .first()
        .expect("private tmux fixture has one scheduler target")
        .id
        .clone();

    let create_id = Uuid::new_v4();
    let drop_marker = fixture
        .configuration
        .get("schedulerRpcDropResponseMarker")
        .and_then(serde_json::Value::as_str)
        .expect("fixture response-drop marker");
    fs::write(drop_marker, b"drop exactly the next scheduler RPC response")
        .expect("arm private RPC response cut");
    let create = runtime.execute(
        route(),
        request(
            create_id,
            SchedulerAction::Create {
                target_id,
                deadline: DeadlineInput::InSeconds { seconds: 3_600 },
                message: "managed SSH lost-reply regression".into(),
                submit: false,
            },
        ),
        &stopping,
    );
    assert!(!create.ok, "the cut response must not look acknowledged");
    assert!(
        create.outcome_unknown,
        "a cut create response must preserve uncertain outcome"
    );
    assert_eq!(
        create.error.as_ref().and_then(|error| error.request_id),
        Some(create_id)
    );

    let receipt_id = Uuid::new_v4();
    let receipt = runtime.execute(
        route(),
        request(
            receipt_id,
            SchedulerAction::Status {
                receipt_request_id: Some(create_id),
            },
        ),
        &stopping,
    );
    assert!(
        receipt.ok,
        "receipt reconciliation failed: {:?}",
        receipt.error
    );
    let Some(SchedulerResult::Success(SchedulerReply::RequestStatus { receipt })) =
        receipt.reply.map(|reply| reply.result)
    else {
        panic!("actual helper did not return the create receipt");
    };
    assert_eq!(receipt.request_id, create_id);
    assert_eq!(receipt.state, ReceiptState::Applied);

    let log_path = fixture
        .configuration
        .get("schedulerRpcRequestLog")
        .and_then(serde_json::Value::as_str)
        .expect("fixture scheduler RPC request log");
    let requests = read_captured_requests(log_path);
    assert_eq!(
        requests
            .iter()
            .filter(|request| matches!(request.action, SchedulerAction::Create { .. }))
            .filter(|request| request.request_id == Some(create_id))
            .count(),
        1,
        "the uncertain create must reach the helper once; transport must not replay it"
    );

    let stale_cancellation = Arc::new(AtomicBool::new(true));
    let stale_route = PreparedRoute {
        identity_namespace: namespace,
        surface_id: SurfaceId(1),
        session_id: SessionId(1),
        launch_generation: 1,
        cancellation: stale_cancellation,
        connection: Arc::clone(&connection),
    };
    let log_len_before_stale = read_captured_requests(log_path).len();
    let stale = runtime.execute(
        stale_route,
        request(
            Uuid::new_v4(),
            SchedulerAction::Status {
                receipt_request_id: None,
            },
        ),
        &stopping,
    );
    assert!(!stale.ok);
    assert_eq!(
        stale.error.as_ref().map(|error| error.code.as_str()),
        Some("connection_unavailable")
    );
    assert_eq!(read_captured_requests(log_path).len(), log_len_before_stale);

    master.stop();
    let log_len_before_closed_master = read_captured_requests(log_path).len();
    let closed_master = runtime.execute(
        route(),
        request(
            Uuid::new_v4(),
            SchedulerAction::Status {
                receipt_request_id: None,
            },
        ),
        &stopping,
    );
    assert!(
        !closed_master.ok,
        "a closed master must not be re-authenticated"
    );
    assert!(!closed_master.outcome_unknown, "Status is a read operation");
    assert_eq!(
        read_captured_requests(log_path).len(),
        log_len_before_closed_master,
        "a closed ControlMaster must fail closed without reaching the helper"
    );

    drop(master);
    drop(connection);
    zmux_remote_runtime::connection::request_cleanup_barrier()
        .expect("SSH cleanup worker")
        .recv_timeout(Duration::from_secs(3))
        .expect("SSH control path cleanup");
    drop(daemon);
    drop(tmux_server);
}

fn request(request_id: Uuid, action: SchedulerAction) -> SchedulerRequest {
    SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id: Some(request_id),
        action,
    }
}

fn private_ssh_test_prerequisites_available() -> bool {
    ["python3", "cargo", "tmux"]
        .iter()
        .all(|program| find_program(program).is_some())
        && ["/usr/sbin/sshd", "/usr/bin/ssh-keygen", "/usr/bin/ssh"]
            .iter()
            .all(|path| Path::new(path).is_file())
}

fn find_program(program: &str) -> Option<String> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(Path::new)
        .map(|directory| directory.join(program))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

fn scheduler_binary() -> PathBuf {
    let executable = std::env::current_exe().expect("test executable path");
    let debug_dir = executable
        .parent()
        .and_then(Path::parent)
        .expect("test executable under target debug directory");
    let binary = debug_dir.join("zmux-scheduler");
    if !binary.is_file() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let target_dir = debug_dir.parent().expect("Cargo target directory");
        let status = Command::new("cargo")
            .args(["build", "--locked", "--offline", "--manifest-path"])
            .arg(manifest)
            .args([
                "--package",
                "zmux-remote-runtime",
                "--bin",
                "zmux-scheduler",
                "--target-dir",
            ])
            .arg(target_dir)
            .status()
            .expect("build the actual scheduler helper for the loopback test");
        assert!(status.success(), "build the scheduler helper binary");
    }
    assert!(
        binary.is_file(),
        "actual scheduler helper binary is available"
    );
    binary
}

fn wait_for_scheduler_daemon(
    binary: &Path,
    state_root: &Path,
    tmux_socket: &Path,
    tmux_binary: &str,
    daemon: &mut ChildGuard,
    log_path: &Path,
) {
    let mut request = serde_json::to_vec(&SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id: Some(Uuid::new_v4()),
        action: SchedulerAction::Status {
            receipt_request_id: None,
        },
    })
    .expect("encode scheduler readiness request");
    request.push(b'\n');
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = daemon
            .0
            .as_mut()
            .expect("scheduler daemon child")
            .try_wait()
            .expect("inspect scheduler daemon")
        {
            panic!(
                "scheduler daemon exited before accepting requests ({status}): {}",
                fs::read_to_string(log_path).unwrap_or_default()
            );
        }
        let mut child = Command::new(binary)
            .arg("rpc")
            .env("ZMUX_SCHEDULER_ROOT", state_root)
            .env("ZMUX_SCHEDULER_TMUX_SOCKET", tmux_socket)
            .env("ZMUX_SCHEDULER_TMUX_BIN", tmux_binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start scheduler readiness RPC");
        child
            .stdin
            .take()
            .expect("readiness request pipe")
            .write_all(&request)
            .expect("write scheduler readiness request");
        let output = child
            .wait_with_output()
            .expect("wait for scheduler readiness RPC");
        if output.status.success()
            && serde_json::from_slice::<SchedulerResponse>(&output.stdout)
                .is_ok_and(|response| matches!(response.result, SchedulerResult::Success(_)))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "scheduler daemon did not accept a Status request: {}",
            fs::read_to_string(log_path).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_live_master(connection: &ManagedConnection, master: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = master
            .0
            .as_mut()
            .expect("managed SSH master child")
            .try_wait()
            .expect("inspect managed SSH master")
        {
            panic!("managed SSH master exited before becoming ready ({status})");
        }
        let spec = connection
            .operation_spec("true")
            .expect("build live-master probe");
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("start live-master probe");
        if output.success() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the exact managed SSH master did not become ready"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_captured_requests(path: &str) -> Vec<SchedulerRequest> {
    fs::read_to_string(path)
        .expect("read private scheduler request log")
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("parse captured scheduler request"))
        .collect()
}

struct PrivateSshFixture {
    controller_root: PathBuf,
    config_path: PathBuf,
    child: Option<Child>,
    configuration: serde_json::Value,
}

impl PrivateSshFixture {
    fn start() -> Self {
        let controller_root = std::env::temp_dir().join(format!(
            "zmux-schedule-ssh-controller-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir(&controller_root).expect("create private SSH fixture controller");
        fs::set_permissions(&controller_root, fs::Permissions::from_mode(0o700))
            .expect("secure private SSH fixture controller");
        let config_path = controller_root.join("fixture.json");
        let diagnostic = controller_root.join("sshd.log");
        let fixture_script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/native-ssh-fixture.py");
        let child = Command::new("python3")
            .arg("-c")
            .arg(LOOPBACK_FIXTURE_DRIVER)
            .arg(&config_path)
            .arg(&diagnostic)
            .arg(fixture_script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start private loopback SSH fixture controller");
        let mut fixture = Self {
            controller_root,
            config_path,
            child: Some(child),
            configuration: serde_json::Value::Null,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if fixture.config_path.is_file() {
                fixture.configuration = serde_json::from_slice(
                    &fs::read(&fixture.config_path).expect("read private SSH fixture config"),
                )
                .expect("parse private SSH fixture config");
                break;
            }
            if let Some(status) = fixture
                .child
                .as_mut()
                .expect("fixture controller child")
                .try_wait()
                .expect("inspect fixture controller")
            {
                panic!(
                    "private loopback SSH fixture exited ({status}): {}",
                    fs::read_to_string(&diagnostic).unwrap_or_default()
                );
            }
            assert!(
                Instant::now() < deadline,
                "private loopback SSH fixture did not become ready: {}",
                fs::read_to_string(&diagnostic).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(25));
        }
        fixture
    }
}

impl Drop for PrivateSshFixture {
    fn drop(&mut self) {
        if let Some(root) = self
            .configuration
            .get("root")
            .and_then(serde_json::Value::as_str)
        {
            let _ = fs::write(Path::new(root).join(".fixture-controller-stop"), b"stop");
        }
        if let Some(child) = &mut self.child {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(child.id() as i32),
                        nix::sys::signal::Signal::SIGTERM,
                    );
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
        let _ = fs::remove_dir_all(&self.controller_root);
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TmuxServerGuard {
    binary: String,
    socket: PathBuf,
}

impl Drop for TmuxServerGuard {
    fn drop(&mut self) {
        let _ = Command::new(&self.binary)
            .args(["-N", "-f", "/dev/null", "-S"])
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }
}
