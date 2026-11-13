//! Owned plugin publication and nonblocking hook ingress through the shipped CLI.
//! Synthetic provider --version only; no Claude invocation or user configuration.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;

use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt, symlink},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, client, protocol::Role};
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, Request, Shell,
    agent_lifecycle::{Activity, AlertKind, Connection, Provider},
    attention::DesktopDeliveryState,
    control::ControlServer,
};
const ROUTE_ENV: &str = "ZMUX_CLAUDE_EVENT_SOCKET";
// A fork in another test thread can transiently inherit a writable executable
// copy before exec closes CLOEXEC descriptors. Keep executable publication and
// child lifetimes sequential within this test process rather than retrying exec.
static PROCESS_FIXTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Directory {
    path: PathBuf,
    _process: std::sync::MutexGuard<'static, ()>,
}

struct BundledCli {
    path: PathBuf,
    previous: Option<(Vec<u8>, u32)>,
}
impl BundledCli {
    fn new(contents_dir: &Path) -> Self {
        let source = PathBuf::from(env!("CARGO_BIN_EXE_zmux"));
        let path = contents_dir.join("Resources/bin/zmux");
        let previous = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                assert!(
                    metadata.is_file(),
                    "bundled CLI test path must be a regular file"
                );
                Some((fs::read(&path).unwrap(), metadata.permissions().mode()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("inspect bundled CLI test path: {error}"),
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(source, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self { path, previous }
    }
}
impl Drop for BundledCli {
    fn drop(&mut self) {
        if let Some((contents, mode)) = self.previous.take() {
            fs::write(&self.path, contents).unwrap();
            fs::set_permissions(&self.path, fs::Permissions::from_mode(mode)).unwrap();
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct RunningDesktop {
    desktop: Arc<Desktop>,
    server: Option<ControlServer>,
}

struct ProviderRelease(PathBuf);
impl Drop for ProviderRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"release");
    }
}

fn shell_integration_roots() -> std::collections::HashSet<PathBuf> {
    let prefix = format!("zmux-shell-{}-", std::process::id());
    fs::read_dir(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect()
}
impl RunningDesktop {
    fn shutdown(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown(Duration::from_secs(2)).unwrap();
        }
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}

fn terminal_output_from_snapshot(
    snapshot: &desktop_runtime::Snapshot,
    session_id: zmux_workspace_core::SessionId,
) -> String {
    snapshot
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)
        .and_then(|frame| frame.snapshot.as_ref())
        .map(|terminal| terminal.viewport.contents.clone())
        .unwrap_or_else(|| "<terminal snapshot unavailable>".into())
}

fn terminal_output_for(desktop: &Desktop, session_id: zmux_workspace_core::SessionId) -> String {
    terminal_output_from_snapshot(&desktop_snapshot::read(desktop), session_id)
}

fn terminal_output_for_until(
    desktop: &Desktop,
    session_id: zmux_workspace_core::SessionId,
    deadline: Instant,
) -> String {
    terminal_output_from_snapshot(&desktop_snapshot::read_until(desktop, deadline), session_id)
}

fn bounded_fixture_terminal_output(
    desktop: &Desktop,
    session_id: zmux_workspace_core::SessionId,
    private_root: &Path,
    deadline: Instant,
) -> String {
    let private_root = private_root.to_string_lossy();
    let snapshot = desktop_snapshot::read_until(desktop, deadline);
    let terminal = snapshot
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)
        .and_then(|frame| frame.snapshot.as_ref())
        .map(|terminal| terminal.viewport.contents.as_str())
        .unwrap_or("<terminal snapshot unavailable>");
    let mut bounded = String::new();
    let mut remaining = terminal;
    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix(private_root.as_ref()) {
            if bounded.len() + "<fixture>".len() > 4096 {
                break;
            }
            bounded.push_str("<fixture>");
            remaining = rest;
            continue;
        }
        let source_character = remaining.chars().next().unwrap();
        let character = if source_character.is_control() {
            ' '
        } else {
            source_character
        };
        if bounded.len() + character.len_utf8() > 4096 {
            break;
        }
        bounded.push(character);
        remaining = &remaining[source_character.len_utf8()..];
    }
    bounded
}

fn codex_fixture_diagnostics(path: &Path) -> String {
    const PHASES: &[&str] = &[
        "starting",
        "listening",
        "connected",
        "probe_seen",
        "notification_sent",
        "notification_acked",
        "client_closed",
    ];
    const ERRORS: &[&str] = &[
        "none",
        "peer_closed",
        "handshake_timeout",
        "websocket_protocol_error",
        "invalid_json",
        "frame_timeout",
        "socket_error",
        "fixture_error",
    ];

    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(128).read_to_end(&mut bytes);
    }
    let contents = String::from_utf8_lossy(&bytes);
    let value = |key: &str, allowed: &[&str]| {
        contents
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .filter(|value| allowed.contains(value))
            .unwrap_or("unknown")
    };
    format!(
        "phase={};error={}",
        value("phase=", PHASES),
        value("error=", ERRORS)
    )
}

fn codex_fixture_liveness(pid_path: &Path, socket_path: &Path) -> String {
    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(pid_path) {
        let _ = file.take(32).read_to_end(&mut bytes);
    }
    let pid = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|pid| *pid > 0);
    let pid_alive = pid.is_some_and(|pid| kill(Pid::from_raw(pid), None).is_ok());
    let socket_present =
        fs::symlink_metadata(socket_path).is_ok_and(|metadata| metadata.file_type().is_socket());
    format!("daemon_pid_alive={pid_alive};socket_path_is_socket={socket_present}")
}

struct SyntheticCodexDaemon {
    child: Child,
    socket: PathBuf,
    diagnostics: PathBuf,
    foreign_probe: PathBuf,
    foreign_ack: PathBuf,
}

impl SyntheticCodexDaemon {
    fn start(directory: &Directory) -> Self {
        let socket = directory.path.join("codex-app-server.sock");
        let state = directory.path.join("provider-state");
        let integration_root = directory.path.join("integration-root");
        let diagnostics = directory.path.join("codex-diagnostics");
        let foreign_probe = directory.path.join("foreign-probe");
        let foreign_ack = directory.path.join("foreign-ack");
        fs::write(&state, "idle").unwrap();
        fs::create_dir(&integration_root).unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/provider-synthetic-codex.py");
        let child = Command::new("python3")
            .arg(fixture)
            .arg("app-server-daemon")
            .env("ZMUX_FAKE_CODEX_SOCKET", &socket)
            .env("ZMUX_FAKE_CODEX_STATE", state)
            .env("ZMUX_SHELL_INTEGRATION_DIRECTORY", integration_root)
            .env("ZMUX_FAKE_CODEX_DIAGNOSTICS", &diagnostics)
            .env("ZMUX_FAKE_CODEX_FOREIGN_PROBE", &foreign_probe)
            .env("ZMUX_FAKE_CODEX_FOREIGN_ACK", &foreign_ack)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the synthetic Codex app-server fixture");
        let mut daemon = Self {
            child,
            socket,
            diagnostics,
            foreign_probe,
            foreign_ack,
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if fs::symlink_metadata(&daemon.socket)
                .is_ok_and(|metadata| metadata.file_type().is_socket())
                && codex_fixture_diagnostics(&daemon.diagnostics) == "phase=listening;error=none"
            {
                return daemon;
            }
            if let Some(status) = daemon.child.try_wait().unwrap() {
                panic!("synthetic Codex fixture exited before listening: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "synthetic Codex fixture did not confirm its listening socket: {}",
                codex_fixture_diagnostics(&daemon.diagnostics)
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn is_running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}

impl Drop for SyntheticCodexDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixture_websocket_connect(path: &Path) -> Result<WebSocket<UnixStream>, String> {
    let stream = UnixStream::connect(path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let (websocket, response) =
        client("ws://localhost", stream).map_err(|error| error.to_string())?;
    if response.status().as_u16() != 101 {
        return Err("fixture did not accept a valid WebSocket handshake".into());
    }
    Ok(websocket)
}

fn fixture_valid_rpc(path: &Path) -> Result<(), String> {
    let mut websocket = fixture_websocket_connect(path)?;
    websocket
        .send(Message::Text(r#"{"id":1,"method":"initialize"}"#.into()))
        .map_err(|error| error.to_string())?;
    fixture_initialize_response(&mut websocket)
}

fn fixture_initialize_response(websocket: &mut WebSocket<UnixStream>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let message = websocket.read().map_err(|error| error.to_string())?;
        let Message::Text(response) = message else {
            if Instant::now() >= deadline {
                return Err("fixture did not answer the recovery RPC".into());
            }
            continue;
        };
        let response: Value = serde_json::from_str(&response).map_err(|error| error.to_string())?;
        if response.get("id") == Some(&json!(1)) {
            if response["result"]["userAgent"] == "synthetic" {
                return Ok(());
            }
            return Err("fixture returned an invalid initialize response".into());
        }
        if Instant::now() >= deadline {
            return Err("fixture did not answer the recovery RPC".into());
        }
    }
}

fn fixture_diagnostic_error(path: &Path) -> Option<String> {
    let mut contents = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(128)
        .read_to_end(&mut contents)
        .ok()?;
    let contents = std::str::from_utf8(&contents).ok()?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix("error=").map(str::to_owned))
}

fn wait_for_fixture_error(path: &Path, expected: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if fixture_diagnostic_error(path).as_deref() == Some(expected) {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    fixture_diagnostic_error(path).as_deref() == Some(expected)
}

#[test]
fn synthetic_codex_fixture_recovers_after_websocket_peer_errors() {
    #[derive(Clone, Copy)]
    enum PeerCase {
        Malformed,
        Oversized,
        OrderlyClose,
        Partial,
        Trickle,
    }

    let mut failures = Vec::new();
    for (name, case, expected_error) in [
        ("malformed", PeerCase::Malformed, "websocket_protocol_error"),
        ("oversized", PeerCase::Oversized, "websocket_protocol_error"),
        ("orderly_close", PeerCase::OrderlyClose, "peer_closed"),
        ("partial", PeerCase::Partial, "frame_timeout"),
        ("trickle", PeerCase::Trickle, "frame_timeout"),
    ] {
        let directory = Directory::new();
        let mut daemon = SyntheticCodexDaemon::start(&directory);
        let mut peer = match fixture_websocket_connect(&daemon.socket) {
            Ok(peer) => Some(peer),
            Err(error) => {
                failures.push(format!("{name}: could not connect fixture peer: {error}"));
                continue;
            }
        };
        let mut peer_failure = None;
        let mut trickle_writer = None;
        match case {
            PeerCase::Malformed => {
                // A client WebSocket frame must be masked; this validly reaches
                // the fixture's protocol parser and fails that contract.
                if let Err(error) = peer.as_mut().unwrap().get_mut().write_all(&[0x81, 0x00]) {
                    peer_failure = Some(format!("could not send malformed peer: {error}"));
                }
            }
            PeerCase::Oversized => {
                let mut frame = vec![0x81, 0xff];
                frame.extend_from_slice(&65_537_u64.to_be_bytes());
                if let Err(error) = peer.as_mut().unwrap().get_mut().write_all(&frame) {
                    peer_failure = Some(format!("could not send oversized peer: {error}"));
                }
            }
            PeerCase::OrderlyClose => {
                if let Err(error) = peer
                    .as_mut()
                    .unwrap()
                    .get_mut()
                    .shutdown(std::net::Shutdown::Both)
                {
                    peer_failure = Some(format!("could not close peer: {error}"));
                }
            }
            PeerCase::Partial => {
                if let Err(error) = peer.as_mut().unwrap().get_mut().write_all(&[0x81]) {
                    peer_failure = Some(format!("could not send partial frame: {error}"));
                } else {
                    fs::write(
                        &daemon.foreign_probe,
                        b"probe while client frame is partial",
                    )
                    .unwrap();
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while !daemon.foreign_ack.is_file() && Instant::now() < deadline {
                        thread::sleep(Duration::from_millis(5));
                    }
                    if !daemon.foreign_ack.is_file() {
                        peer_failure =
                            Some("partial WebSocket frame blocked the fixture probe loop".into());
                        let _ = peer
                            .as_mut()
                            .unwrap()
                            .get_mut()
                            .shutdown(std::net::Shutdown::Both);
                    }
                }
            }
            PeerCase::Trickle => {
                let mut stream = peer.as_mut().unwrap().get_mut().try_clone().unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                // Start an incomplete 65,535-byte masked frame, then keep
                // delivering bytes well inside an inactivity timeout. The
                // frame must still expire relative to its first byte.
                stream
                    .write_all(&[0x81, 0xfe, 0xff, 0xff, 0, 0, 0, 0])
                    .unwrap();
                let stopped = Arc::new(AtomicBool::new(false));
                let writer_stopped = Arc::clone(&stopped);
                let writer = thread::spawn(move || {
                    while !writer_stopped.load(Ordering::Acquire) {
                        thread::sleep(Duration::from_millis(80));
                        if stream.write_all(b"x").is_err() {
                            break;
                        }
                    }
                });
                trickle_writer = Some((stopped, writer));
            }
        }
        if !matches!(case, PeerCase::Partial | PeerCase::Trickle) {
            drop(peer.take());
        }

        if let Some(error) = peer_failure {
            failures.push(format!("{name}: {error}"));
        }
        let error_observed =
            wait_for_fixture_error(&daemon.diagnostics, expected_error, Duration::from_secs(3));
        if let Some((stopped, writer)) = trickle_writer {
            stopped.store(true, Ordering::Release);
            writer.join().unwrap();
        }
        if !error_observed {
            failures.push(format!(
                "{name}: expected static fixture error classification {expected_error}"
            ));
        }
        drop(peer.take());
        if !daemon.is_running() {
            failures.push(format!(
                "{name}: accepted peer terminated the fixture listener"
            ));
        }
        if let Err(error) = fixture_valid_rpc(&daemon.socket) {
            failures.push(format!(
                "{name}: later valid client was not served: {error}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "synthetic Codex fixture recovery contract failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn synthetic_codex_fixture_answers_a_frame_buffered_with_the_upgrade() {
    let directory = Directory::new();
    let daemon = SyntheticCodexDaemon::start(&directory);
    let mut stream = UnixStream::connect(&daemon.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut upgrade = b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n".to_vec();
    let request = br#"{"id":1,"method":"initialize"}"#;
    let mask = [1_u8, 2, 3, 4];
    upgrade.extend([0x81, 0x80 | request.len() as u8]);
    upgrade.extend(mask);
    upgrade.extend(
        request
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    // One write makes the first frame available to the HTTP upgrade read.
    // No later request can wake a fixture that forgets the buffered remainder.
    stream.write_all(&upgrade).unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        assert!(
            response.len() < 8192,
            "fixture upgrade response exceeded its bound"
        );
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .expect("fixture upgrade response");
        response.push(byte[0]);
    }
    assert!(response.starts_with(b"HTTP/1.1 101 "));
    let mut websocket = WebSocket::from_raw_socket(stream, Role::Client, None);
    fixture_initialize_response(&mut websocket)
        .expect("fixture must answer a frame buffered with its upgrade");
}

fn wait_local_codex(
    desktop: &Desktop,
    predicate: impl Fn(&zmux_desktop_runtime::agent_lifecycle::Agent) -> bool,
    context: &str,
) -> zmux_desktop_runtime::agent_lifecycle::Agent {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let snapshot = desktop_snapshot::read_until(desktop, deadline);
        if let Some(agent) = snapshot
            .agent_lifecycle
            .iter()
            .find(|agent| agent.provider == Provider::CodexLocal && predicate(agent))
        {
            return agent.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{context}; provider snapshot={:?}; fixture terminals={:?}",
            snapshot.agent_lifecycle,
            snapshot
                .control_frames
                .iter()
                .take(4)
                .filter_map(|frame| frame.snapshot.as_ref())
                .map(|terminal| terminal
                    .viewport
                    .contents
                    .chars()
                    .take(4096)
                    .collect::<String>())
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_local_provider(
    desktop: &Desktop,
    provider: Provider,
    predicate: impl Fn(&zmux_desktop_runtime::agent_lifecycle::Agent) -> bool,
    context: &str,
) -> zmux_desktop_runtime::agent_lifecycle::Agent {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let snapshot = desktop_snapshot::read_until(desktop, deadline);
        if let Some(agent) = snapshot
            .agent_lifecycle
            .iter()
            .find(|agent| agent.provider == provider && predicate(agent))
        {
            return agent.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{context}; provider snapshot={:?}",
            snapshot.agent_lifecycle
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn local_codex_completion_count(desktop: &Desktop) -> usize {
    desktop_snapshot::read(desktop)
        .agent_alert_events
        .iter()
        .filter(|event| {
            event.provider == Provider::CodexLocal && event.kind == AlertKind::Completed
        })
        .count()
}

fn local_provider_alert_count(desktop: &Desktop, provider: Provider, kind: AlertKind) -> usize {
    desktop_snapshot::read(desktop)
        .agent_alert_events
        .iter()
        .filter(|event| event.provider == provider && event.kind == kind)
        .count()
}

fn local_provider_alert_count_until(
    desktop: &Desktop,
    provider: Provider,
    kind: AlertKind,
    deadline: Instant,
) -> usize {
    desktop_snapshot::read_until(desktop, deadline)
        .agent_alert_events
        .iter()
        .filter(|event| event.provider == provider && event.kind == kind)
        .count()
}
impl Drop for RunningDesktop {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.shutdown(Duration::from_secs(2));
        }
        let _ = self.desktop.shutdown(Duration::from_secs(5));
    }
}
impl Directory {
    fn new() -> Self {
        let guard = PROCESS_FIXTURE.lock().unwrap();
        // Keep the real Desktop control socket short on macOS, where TMPDIR
        // commonly has a long /var/folders prefix and sockaddr_un.sun_path is
        // small. macOS also exposes /tmp as a symlink to /private/tmp, while
        // the production plugin opener intentionally rejects symlinked path
        // components. Resolve that alias and keep the unique basename short
        // enough for the default control-socket path. The provider hook still
        // uses the production route.
        let temp_root = PathBuf::from("/tmp")
            .canonicalize()
            .expect("canonicalize the short Unix temporary root");
        let p = temp_root.join(format!("zhooks-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            path: p,
            _process: guard,
        }
    }
    fn plugin(&self) -> PathBuf {
        self.path.join("owned-plugin")
    }
    fn provider(&self) -> PathBuf {
        let p = self.path.join("synthetic-provider");
        if p.exists() {
            return p;
        }
        fs::write(
            &p,
            "#!/bin/sh\ncase \"$1\" in\n  --version) printf '2.1.301 (Claude Code)\\n' ;;\n  --help) printf '%s\\n' '  --plugin-dir <path> Load a plugin for this session (repeatable:' '    --plugin-dir A --plugin-dir B)' ;;\n  *) printf invoked > \"$FIXTURE_PROVIDER_CALLED\" ;;\nesac\n",
        )
        .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn command(&self, args: &[&str]) -> Command {
        let adapter = self.path.join("adapter");
        if !adapter.exists() {
            fs::copy(env!("CARGO_BIN_EXE_zmux"), &adapter).unwrap();
            fs::set_permissions(&adapter, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut c = Command::new(adapter);
        c.env("HOME", &self.path)
            .env("FIXTURE_PROVIDER_CALLED", self.path.join("provider-called"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                c.env_remove(name);
            }
        }
        c.args(args);
        c
    }
    fn install(&self) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                "install",
                "--directory",
                self.plugin().to_str().unwrap(),
                "--provider",
                self.provider().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
    fn install_provider(
        &self,
        provider: &Path,
        version: &str,
        help: &str,
        replacement: Option<&Path>,
    ) -> Output {
        let mut command = self.command(&[
            "hooks",
            "claude",
            "install",
            "--directory",
            self.plugin().to_str().unwrap(),
            "--provider",
            provider.to_str().unwrap(),
        ]);
        command
            .env("FIXTURE_PROVIDER_VERSION", version)
            .env("FIXTURE_PROVIDER_HELP", help);
        if let Some(replacement) = replacement {
            command.env("FIXTURE_PROVIDER_REPLACEMENT", replacement);
        }
        command.output().unwrap()
    }
    fn call(&self, verb: &str) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                verb,
                "--directory",
                self.plugin().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn decoded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn read_event_frame(stream: &mut UnixStream) -> Value {
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let length = u32::from_be_bytes(length) as usize;
    assert!(length <= 65536, "local hook frame exceeds its bound");
    let mut frame = vec![0; length];
    stream.read_exact(&mut frame).unwrap();
    serde_json::from_slice(&frame).unwrap()
}

fn write_event_frame(stream: &mut UnixStream, frame: &Value) {
    let bytes = serde_json::to_vec(frame).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

const PTY_CHILD_TIMEOUT: Duration = Duration::from_secs(30);
const PTY_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const PTY_CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CAPTURED_PTY_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct PtyOutputCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

fn pty_output_diagnostic(capture: &Arc<Mutex<PtyOutputCapture>>) -> String {
    let capture = capture.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut output = String::from_utf8_lossy(&capture.bytes).into_owned();
    if capture.truncated {
        output.push_str("\n[PTY output truncated after 1 MiB]");
    }
    output
}

struct PtyChildCleanup {
    child_killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    process_group: Option<Pid>,
    child_reaped: bool,
    armed: bool,
}

impl PtyChildCleanup {
    fn new(
        child_killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
        process_group: Option<Pid>,
    ) -> Self {
        Self {
            child_killer,
            process_group,
            child_reaped: false,
            armed: true,
        }
    }

    fn terminate(&mut self) {
        // Reaping the child releases its PID, which is also the PTY process
        // group leader ID. Never signal that cached group after reaping.
        if self.child_reaped {
            return;
        }
        if let Some(process_group) = self.process_group {
            let _ = killpg(process_group, Signal::SIGKILL);
        }
        let _ = self.child_killer.kill();
    }

    fn mark_child_reaped(&mut self) {
        self.child_reaped = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PtyChildCleanup {
    fn drop(&mut self) {
        if self.armed {
            self.terminate();
        }
    }
}

struct PtyOutputReader {
    cancelled: Arc<AtomicBool>,
    done: mpsc::Receiver<Result<(), String>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PtyOutputReader {
    fn start(mut reader: Box<dyn Read + Send>, capture: Arc<Mutex<PtyOutputCapture>>) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let reader_cancelled = Arc::clone(&cancelled);
        let (done_tx, done) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("provider-hook-pty-reader".into())
            .spawn(move || {
                let mut buffer = [0; 8192];
                let result = loop {
                    if reader_cancelled.load(Ordering::Acquire) {
                        break Err("PTY output reader was cancelled".into());
                    }
                    match reader.read(&mut buffer) {
                        Ok(0) => break Ok(()),
                        Ok(read) => {
                            let mut capture =
                                capture.lock().unwrap_or_else(|poison| poison.into_inner());
                            let available =
                                MAX_CAPTURED_PTY_OUTPUT_BYTES.saturating_sub(capture.bytes.len());
                            let captured = read.min(available);
                            capture.bytes.extend_from_slice(&buffer[..captured]);
                            capture.truncated |= captured < read;
                        }
                        Err(error)
                            if error.raw_os_error() == Some(nix::errno::Errno::EIO as i32) =>
                        {
                            // On Unix, portable-pty maps EIO to EOF after the
                            // last slave descriptor closes.
                            break Ok(());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => break Err(format!("read PTY master: {error}")),
                    }
                };
                let _ = done_tx.send(result);
            })
            .unwrap_or_else(|error| panic!("start automatic Claude PTY output reader: {error}"));
        Self {
            cancelled,
            done,
            thread: Some(thread),
        }
    }

    fn wait(&mut self, timeout: Duration, expectation: &str) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "timed out waiting for {expectation} after {timeout:?}"
            ));
        }
        let result = match self.done.recv_timeout(remaining) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "timed out waiting for {expectation} after {timeout:?}"
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(format!("PTY output reader ended before {expectation}"));
            }
        };

        let join_deadline = Instant::now() + PTY_CLEANUP_TIMEOUT;
        while self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
        {
            if Instant::now() >= join_deadline {
                return Err(format!(
                    "PTY output reader did not stop within {PTY_CLEANUP_TIMEOUT:?}"
                ));
            }
            thread::sleep(Duration::from_millis(1));
        }
        self.thread
            .take()
            .ok_or_else(|| "PTY output reader was already joined".to_owned())?
            .join()
            .map_err(|_| "PTY output reader panicked".to_owned())?;
        result
    }

    fn cancel_and_wait(&mut self, timeout: Duration) -> Result<(), String> {
        self.cancelled.store(true, Ordering::Release);
        if self.thread.is_none() {
            return Ok(());
        }
        match self.wait(timeout, "PTY output reader cleanup") {
            Err(error) if error == "PTY output reader was cancelled" => Ok(()),
            result => result,
        }
    }
}

impl Drop for PtyOutputReader {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            // Nonblocking reads and cancellation checks ensure the reader
            // cannot remain stuck waiting for PTY output during cleanup.
            let _ = thread.join();
        }
    }
}

fn run_terminal_command(command: CommandBuilder) -> (portable_pty::ExitStatus, String) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let reader = pair.master.try_clone_reader().unwrap();
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let process_group = pair.master.process_group_leader().map(Pid::from_raw);
    let mut cleanup = PtyChildCleanup::new(child.clone_killer(), process_group);
    let output_capture = Arc::new(Mutex::new(PtyOutputCapture::default()));
    // Drain output while the child runs: waiting first can block forever when
    // the PTY output queue fills before the child exits. O_NONBLOCK makes the
    // reader thread cancellable on every Unix platform.
    let master_fd = pair
        .master
        .as_raw_fd()
        .expect("Unix PTY master exposes its file descriptor");
    let flags = fcntl(master_fd, FcntlArg::F_GETFL).expect("read PTY master flags");
    fcntl(
        master_fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .expect("make PTY master reads cancellable");
    let mut output_reader = PtyOutputReader::start(reader, Arc::clone(&output_capture));

    let child_deadline = Instant::now() + PTY_CHILD_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                cleanup.mark_child_reaped();
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                cleanup.terminate();
                let reader_cleanup = output_reader.cancel_and_wait(PTY_CLEANUP_TIMEOUT);
                panic!(
                    "poll automatic Claude PTY child failed: {error}; output reader cleanup: {reader_cleanup:?}; captured output:\n{}",
                    pty_output_diagnostic(&output_capture)
                );
            }
        }
        if Instant::now() >= child_deadline {
            cleanup.terminate();
            let cleanup_deadline = Instant::now() + PTY_CLEANUP_TIMEOUT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        cleanup.mark_child_reaped();
                        break;
                    }
                    Ok(None) if Instant::now() < cleanup_deadline => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(None) => panic!(
                        "automatic Claude PTY child did not exit within {PTY_CHILD_TIMEOUT:?} or after process-group termination; captured output:\n{}",
                        pty_output_diagnostic(&output_capture)
                    ),
                    Err(error) => panic!(
                        "reap automatic Claude PTY child after timeout: {error}; captured output:\n{}",
                        pty_output_diagnostic(&output_capture)
                    ),
                }
            }
            let reader_cleanup = output_reader.cancel_and_wait(PTY_CLEANUP_TIMEOUT);
            panic!(
                "automatic Claude PTY child exceeded {PTY_CHILD_TIMEOUT:?}; requested PTY child/process-group cleanup; output reader cleanup: {reader_cleanup:?}; captured output:\n{}",
                pty_output_diagnostic(&output_capture)
            );
        }
        thread::sleep(Duration::from_millis(10));
    };

    if let Err(error) = output_reader.wait(PTY_OUTPUT_DRAIN_TIMEOUT, "PTY EOF") {
        cleanup.terminate();
        let reader_cleanup = output_reader.cancel_and_wait(PTY_CLEANUP_TIMEOUT);
        panic!(
            "automatic Claude PTY child was reaped, but PTY output did not reach EOF cleanly: {error}; skipped process-group signaling after reap; reader cleanup: {reader_cleanup:?}; captured output:\n{}",
            pty_output_diagnostic(&output_capture)
        );
    }
    cleanup.disarm();
    drop(pair.master);
    let output = pty_output_diagnostic(&output_capture);
    (status, output)
}

#[test]
fn local_hook_ipc_sends_only_minimal_lifecycle_fields() {
    let dir = Directory::new();
    let socket_path = dir.path.join("hook-route.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        for (expected_event, expected_tool) in
            [("UserPromptSubmit", None), ("PreToolUse", Some("Other"))]
        {
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "local hook did not connect");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept local hook: {error}"),
                }
            };
            // Darwin can inherit the listener's nonblocking mode on accepted
            // sockets. This fixture reads complete bounded frames below.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let request = read_event_frame(&mut stream);
            assert_eq!(request["event"], expected_event);
            assert!(request["hook_id"].as_str().is_some());
            let input = &request["input"];
            assert_eq!(input["hook_event_name"], expected_event);
            assert_eq!(input["session_id"], "provider-session");
            assert!(input.get("prompt").is_none());
            assert!(input.get("tool_input").is_none());
            assert!(input.get("tool_use_id").is_none());
            assert!(input.get("private_unknown").is_none());
            assert_eq!(input["agent_id"], "redacted");
            if expected_event == "UserPromptSubmit" {
                assert_eq!(input["prompt_id"], "123e4567-e89b-42d3-a456-426614174000");
                assert_eq!(input["permission_mode"], "default");
            }
            if let Some(expected_tool) = expected_tool {
                assert_eq!(input["tool_name"], expected_tool);
            }
            write_event_frame(
                &mut stream,
                &json!({"hook_disposition":"not_handled","output":{}}),
            );
        }
    });

    let cases = [
        (
            "UserPromptSubmit",
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"provider-session",
                "prompt_id":"123e4567-e89b-42d3-a456-426614174000",
                "permission_mode":"default",
                "prompt":"private user prompt content",
                "tool_input":{"command":"private shell input"},
                "agent_id":"private-agent-id",
                "private_unknown":"must not cross local IPC"
            }),
        ),
        (
            "PreToolUse",
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"provider-session",
                "tool_name":"Bash",
                "tool_use_id":"private-tool-use-id",
                "tool_input":{"command":"private shell input"},
                "agent_type":"private-agent-type",
                "private_unknown":"must not cross local IPC"
            }),
        ),
    ];
    for (event, payload) in cases {
        let mut child = dir.command(&["hooks", "claude", "event", event]);
        child
            .env(ROUTE_ENV, &socket_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = child.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{event} hook client failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    server.join().unwrap();
}

#[test]
fn owned_plugin_is_idempotent_and_uninstall_preserves_unrelated_provider_settings() {
    let dir = Directory::new();
    let unrelated = dir.path.join(".claude");
    fs::create_dir(&unrelated).unwrap();
    let sentinel = b"{\"unrelated\":true,\"hooks\":{\"Stop\":[{\"command\":\"keep me\"}]}}";
    fs::write(unrelated.join("settings.json"), sentinel).unwrap();
    let first = dir.install();
    assert_eq!(first["changed"], true);
    let second = dir.install();
    assert_eq!(second["changed"], false);
    assert_eq!(first["generation"], second["generation"]);
    let installation: Value =
        serde_json::from_slice(&fs::read(dir.plugin().join("installation.json")).unwrap()).unwrap();
    assert_eq!(
        installation["generations"][0]["provider_version"], "2.1.301",
        "existing 2.1.301 installations retain their actual version and remain idempotent"
    );
    assert_eq!(dir.call("status")["installed"], true);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], false);
    assert_eq!(dir.call("status")["installed"], false);
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
}

#[test]
fn provider_preflight_rejects_future_versions_missing_repeatability_and_replaced_executables() {
    let repeatable_help = "--plugin-dir <path> Load a plugin for this session (repeatable: --plugin-dir A --plugin-dir B)";
    let cases = [
        (
            "future patch",
            "2.1.317",
            repeatable_help,
            false,
            "reviewed patch range",
        ),
        (
            "missing repeatability capability",
            "2.1.301",
            "--plugin-dir <path> Load a plugin for this session",
            false,
            "repeatable --plugin-dir",
        ),
        (
            "executable replaced during help preflight",
            "2.1.301",
            repeatable_help,
            true,
            "changed during --help preflight",
        ),
    ];

    for (case, version, help, replace_during_help, expected_error) in cases {
        let dir = Directory::new();
        let provider = dir.path.join("synthetic-provider");
        fs::write(
            &provider,
            "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s (Claude Code)\\n' \"$FIXTURE_PROVIDER_VERSION\" ;;\n  --help) if [ -n \"${FIXTURE_PROVIDER_REPLACEMENT-}\" ]; then /bin/mv \"$FIXTURE_PROVIDER_REPLACEMENT\" \"$0\"; fi; printf '%s\\n' \"$FIXTURE_PROVIDER_HELP\" ;;\n  *) printf started > \"$FIXTURE_PROVIDER_CALLED\" ;;\nesac\n",
        )
        .unwrap();
        fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
        let replacement = dir.path.join("replacement-provider");
        if replace_during_help {
            fs::write(
                &replacement,
                "#!/bin/sh\nprintf started > \"$FIXTURE_PROVIDER_CALLED\"\n",
            )
            .unwrap();
            fs::set_permissions(&replacement, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let output = dir.install_provider(
            &provider,
            version,
            help,
            replace_during_help.then_some(replacement.as_path()),
        );
        assert!(!output.status.success(), "{case} unexpectedly installed");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(expected_error),
            "{case} failed for an unexpected reason: {error}"
        );
        assert!(!dir.plugin().exists(), "{case} published a plugin");
        assert!(
            !dir.path.join("provider-called").exists(),
            "{case} unexpectedly started Claude"
        );
    }
}

#[test]
fn changed_or_symlinked_plugin_files_are_retained_after_admissions_are_disabled() {
    for substitute in [false, true] {
        let dir = Directory::new();
        let installed = dir.install();
        let config = dir
            .plugin()
            .join(installed["generation"].as_str().unwrap())
            .join("hooks/hooks.json");
        let unrelated = dir.path.join("unrelated.json");
        fs::write(&unrelated, b"unrelated sentinel").unwrap();
        if substitute {
            fs::remove_file(&config).unwrap();
            symlink(&unrelated, &config).unwrap();
        } else {
            fs::write(&config, b"changed user file").unwrap();
        }
        let before = fs::read(&config).unwrap();
        let result = dir.call("uninstall");
        assert_eq!(result["disabled"], true);
        assert_eq!(result["cleanup_pending"], true);
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated sentinel");
        let record: Value =
            serde_json::from_slice(&fs::read(dir.plugin().join("installation.json")).unwrap())
                .unwrap();
        assert_eq!(record["enabled"], false);
    }
}
#[test]
fn malformed_unrouted_and_oversized_hooks_are_nonblocking_noops() {
    let dir = Directory::new();
    for bytes in [b"not JSON".to_vec(), b"{}".to_vec(), vec![b'x'; 70_000]] {
        let mut child = dir
            .command(&["hooks", "claude", "event", "UserPromptSubmit"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let _ = input.write_all(&bytes);
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            serde_json::json!({})
        );
        assert!(output.stderr.is_empty());
    }
}
#[test]
fn headless_and_unsupported_launches_never_start_the_provider() {
    let dir = Directory::new();
    dir.install();
    for args in [
        vec![],
        vec!["--print", "prompt"],
        vec!["--settings", "{}"],
        vec!["resume"],
        vec!["--bare"],
    ] {
        let mut command = dir.command(&[
            "hooks",
            "claude",
            "run",
            "--directory",
            dir.plugin().to_str().unwrap(),
            "--surface",
            &uuid::Uuid::new_v4().to_string(),
            "--",
        ]);
        command.args(args);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(!dir.path.join("provider-called").exists());
    }
}

#[test]
fn automatic_compatibility_fallback_runs_once_before_lifecycle_setup() {
    let dir = Directory::new();
    let provider = dir.path.join("claude");
    fs::write(
        &provider,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '99.0 (Claude Code)\\n'; exit 0; fi\nprintf '<%s>\\n' \"$@\" > \"$FIXTURE_AUTO_ARGS\"\nprintf '%s' \"${ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE-unset}\" > \"$FIXTURE_AUTO_ALERT_MODE\"\nprintf started > \"$FIXTURE_AUTO_CALLED\"\nprintf 'FALLBACK_PROVIDER_STARTED\\n'\n/usr/bin/head -c 65536 /dev/zero\nprintf '\\nFALLBACK_PROVIDER_OUTPUT_DRAINED\\n'\nexit 23\n",
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let notice_plugin = dir.path.join("notice-plugin");
    fs::create_dir(&notice_plugin).unwrap();
    let report = dir.path.join("auto-arguments");
    let called = dir.path.join("auto-called");
    let alert_mode = dir.path.join("auto-alert-mode");
    let surface = uuid::Uuid::new_v4().to_string();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_zmux"));
    command.args([
        "hooks",
        "claude",
        "auto",
        "--root",
        dir.path.to_str().unwrap(),
        "--surface",
        &surface,
        "--notice-plugin",
        notice_plugin.to_str().unwrap(),
        "--",
        "--model",
        "sonnet",
        "literal $HOME ; prompt",
    ]);
    command.env("PATH", &dir.path);
    command.env("FIXTURE_AUTO_ARGS", &report);
    command.env("FIXTURE_AUTO_CALLED", &called);
    command.env("FIXTURE_AUTO_ALERT_MODE", &alert_mode);
    command.env("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE", "1");
    let (status, output) = run_terminal_command(command);
    assert_eq!(status.exit_code(), 23, "{output}");
    assert!(output.contains("automatic Claude lifecycle unavailable"));
    assert!(output.contains("FALLBACK_PROVIDER_STARTED"));
    assert!(output.contains("FALLBACK_PROVIDER_OUTPUT_DRAINED"));
    assert_eq!(
        fs::read_to_string(report).unwrap(),
        format!(
            "<--plugin-dir>\n<{}>\n<--model>\n<sonnet>\n<literal $HOME ; prompt>\n",
            notice_plugin.display()
        )
    );
    assert_eq!(fs::read(called).unwrap(), b"started");
    assert_eq!(fs::read(alert_mode).unwrap(), b"unset");
    assert!(fs::read_dir(&dir.path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("claude-auto-")
    }));
}

#[test]
fn automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli() {
    let dir = Directory::new();
    let provider = dir.path.join("claude");
    let rustc = Command::new("rustc")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/claude-provider.rs"
        ))
        .arg("-o")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();

    let args_report = dir.path.join("auto-arguments");
    let provider_called = dir.path.join("auto-called");
    let provider_release = dir.path.join("auto-release");
    let no_prompt_hooks_done = dir.path.join("auto-no-prompt-hooks-done");
    let tracked_hooks_release = dir.path.join("auto-tracked-hooks-release");
    let hooks_done = dir.path.join("auto-hooks-done");
    let provider_env = dir.path.join("auto-provider-env");
    let mut paths = vec![
        dir.path.clone(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).unwrap();
    let test_binary = std::env::current_exe().unwrap();
    let private_contents = dir.path.join("deps");
    fs::create_dir_all(&private_contents).unwrap();
    let private_test_binary = private_contents.join(test_binary.file_name().unwrap());
    fs::copy(&test_binary, &private_test_binary).unwrap();
    fs::set_permissions(&private_test_binary, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Command::new(&private_test_binary);
    child.args([
        "--ignored",
        "--exact",
        "automatic_install_to_runtime_lease_real_desktop_child",
        "--nocapture",
    ]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("ZMUX_") {
            child.env_remove(name);
        }
    }
    child.env("HOME", &dir.path);
    child.env("PATH", path);
    child.env("FIXTURE_AUTO_ARGS", &args_report);
    child.env("FIXTURE_AUTO_CALLED", &provider_called);
    child.env("FIXTURE_AUTO_RELEASE", &provider_release);
    child.env("FIXTURE_AUTO_EMIT_HOOKS", "1");
    child.env("FIXTURE_AUTO_NO_PROMPT_DONE", &no_prompt_hooks_done);
    child.env("FIXTURE_AUTO_TRACKED_RELEASE", &tracked_hooks_release);
    child.env("FIXTURE_AUTO_HOOKS_DONE", &hooks_done);
    child.env("FIXTURE_AUTO_ENV_REPORT", &provider_env);
    child.env_remove("XDG_RUNTIME_DIR");
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "child desktop lifecycle regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "invoked by the parent test with a private environment"]
fn automatic_install_to_runtime_lease_real_desktop_child() {
    let root = PathBuf::from(std::env::var_os("HOME").expect("private test HOME"));
    let contents_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let cli = BundledCli::new(&contents_dir);
    assert_eq!(
        cli.path,
        contents_dir.join("Resources/bin/zmux"),
        "the real Desktop must resolve the private bundled CLI used by this child"
    );
    let args_report = PathBuf::from(std::env::var_os("FIXTURE_AUTO_ARGS").unwrap());
    let provider_called = PathBuf::from(std::env::var_os("FIXTURE_AUTO_CALLED").unwrap());
    let provider_release = PathBuf::from(std::env::var_os("FIXTURE_AUTO_RELEASE").unwrap());
    let no_prompt_hooks_done =
        PathBuf::from(std::env::var_os("FIXTURE_AUTO_NO_PROMPT_DONE").unwrap());
    let tracked_hooks_release =
        PathBuf::from(std::env::var_os("FIXTURE_AUTO_TRACKED_RELEASE").unwrap());
    let hooks_done = PathBuf::from(std::env::var_os("FIXTURE_AUTO_HOOKS_DONE").unwrap());
    let provider_env = PathBuf::from(std::env::var_os("FIXTURE_AUTO_ENV_REPORT").unwrap());
    let shell_rc = root.join("bashrc");
    fs::write(&shell_rc, "PS1=\nprintf 'AUTO_DESKTOP_SHELL_READY\\n'\n").unwrap();
    let existing_shell_roots = shell_integration_roots();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                shell_rc.to_string_lossy().into_owned(),
                "-i".into(),
            ],
            cwd: Some(root.clone()),
        })
        .unwrap(),
    );
    #[cfg(target_os = "macos")]
    let control_socket = root.join("Library/Application Support/zmux/control.sock");
    #[cfg(not(target_os = "macos"))]
    let control_socket = root.join(".local/run/zmux/control.sock");
    let server = ControlServer::start(&control_socket, Arc::clone(&desktop)).unwrap();
    let mut running = RunningDesktop {
        desktop,
        server: Some(server),
    };
    let _release_on_drop = ProviderRelease(provider_release.clone());

    let startup_deadline = Instant::now() + Duration::from_secs(8);
    let (session_id, shell_root) = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, startup_deadline);
        let session_id = snapshot
            .model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| pane.surfaces.iter())
            .find_map(|surface| surface.session_id);
        if let Some(session_id) = session_id {
            let ready = snapshot.control_frames.iter().any(|frame| {
                frame.session_id == session_id
                    && frame.snapshot.as_ref().is_some_and(|terminal| {
                        terminal.state == zmux_session_runtime::State::Running
                            && terminal
                                .viewport
                                .contents
                                .contains("AUTO_DESKTOP_SHELL_READY")
                    })
            });
            let shell_root = shell_integration_roots()
                .difference(&existing_shell_roots)
                .find(|path| path.join("plugin/hooks/hooks.json").is_file())
                .cloned();
            if ready && let Some(shell_root) = shell_root {
                break (session_id, shell_root);
            }
        }
        assert!(
            Instant::now() < startup_deadline,
            "real Desktop terminal did not reach its interactive startup"
        );
        thread::sleep(Duration::from_millis(5));
    };

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 41,
            payload: DesktopCommand::Input {
                session_id,
                text: "claude --model sonnet 'literal $HOME ; prompt'\n".into(),
            },
        })
        .unwrap();

    let setup_deadline = Instant::now() + Duration::from_secs(8);
    let auto_directory = loop {
        let candidate = fs::read_dir(&shell_root)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("claude-auto-")
            })
            .map(|entry| entry.path());
        if let Some(candidate) = candidate
            && candidate.join("installation.json").is_file()
        {
            break candidate;
        }
        if provider_called.exists() {
            let report = fs::read_to_string(&args_report).unwrap_or_default();
            let terminal_output =
                terminal_output_for_until(&running.desktop, session_id, setup_deadline);
            panic!(
                "Claude started before automatic plugin publication: {report}\nterminal output:\n{terminal_output}"
            );
        }
        assert!(
            Instant::now() < setup_deadline,
            "real automatic launch did not publish its lifecycle plugin; terminal output:\n{}",
            terminal_output_for_until(&running.desktop, session_id, setup_deadline)
        );
        thread::sleep(Duration::from_millis(2));
    };

    // Race the shipped cleanup command against the real CLI's install-to-lease
    // interval. Its admission lock must keep cleanup out until the lease exists.
    let cleanup_before_lease = Command::new(&cli.path)
        .args([
            "hooks",
            "claude",
            "cleanup",
            "--root",
            shell_root.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let provider_deadline = Instant::now() + Duration::from_secs(8);
    while !provider_called.exists() {
        assert!(
            Instant::now() < provider_deadline,
            "real automatic launch did not spawn the synthetic provider"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let cleanup_before_lease = cleanup_before_lease.wait_with_output().unwrap();
    if cleanup_before_lease.status.success() {
        let first_cleanup: Value = serde_json::from_slice(&cleanup_before_lease.stdout).unwrap();
        assert_eq!(first_cleanup["removed"], 0);
    } else {
        assert!(
            String::from_utf8_lossy(&cleanup_before_lease.stderr)
                .contains("temporarily unavailable"),
            "unexpected cleanup failure: {}",
            String::from_utf8_lossy(&cleanup_before_lease.stderr)
        );
    }
    assert!(auto_directory.is_dir());

    let marker_deadline = Instant::now() + Duration::from_secs(3);
    while auto_directory.join("launch-unknown").exists() {
        assert!(
            Instant::now() < marker_deadline,
            "provider identity was not confirmed while its runtime lease was held"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let cleanup_during_provider = Command::new(&cli.path)
        .args([
            "hooks",
            "claude",
            "cleanup",
            "--root",
            shell_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cleanup_during_provider.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup_during_provider.stderr)
    );
    let second_cleanup: Value = serde_json::from_slice(&cleanup_during_provider.stdout).unwrap();
    let auto_name = auto_directory.file_name().unwrap().to_string_lossy();
    assert_eq!(second_cleanup["removed"], 0);
    assert!(
        second_cleanup["retained"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| { name.as_str() == Some(auto_name.as_ref()) })
    );
    assert!(
        auto_directory.is_dir(),
        "runtime lease did not retain the generation"
    );
    let retained_installation: Value =
        serde_json::from_slice(&fs::read(auto_directory.join("installation.json")).unwrap())
            .unwrap();
    assert_eq!(
        retained_installation["enabled"], true,
        "active-provider cleanup must retain lifecycle admission as well as plugin files"
    );
    assert_eq!(
        retained_installation["generations"][0]["provider_version"], "2.1.316",
        "installation metadata records the provider version discovered by the CLI"
    );

    let arguments: Vec<_> = fs::read_to_string(&args_report)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        arguments
            .iter()
            .filter(|arg| arg.as_str() == "<--plugin-dir>")
            .count(),
        2
    );
    let lifecycle_path = arguments[1]
        .strip_prefix('<')
        .unwrap()
        .strip_suffix('>')
        .unwrap();
    assert!(
        PathBuf::from(lifecycle_path)
            .join("hooks/hooks.json")
            .is_file()
    );
    assert_eq!(
        arguments[3],
        format!("<{}>", shell_root.join("plugin").display())
    );
    assert_eq!(arguments[4], "<--session-id>");
    assert_eq!(arguments[6], "<--model>");
    assert_eq!(arguments[7], "<sonnet>");
    assert_eq!(arguments[8], "<literal $HOME ; prompt>");

    let hooks_deadline = Instant::now() + Duration::from_secs(8);
    while !no_prompt_hooks_done.exists() {
        assert!(
            Instant::now() < hooks_deadline,
            "real provider did not execute the no-prompt lifecycle and OSC hooks"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let no_prompt_deadline = Instant::now() + Duration::from_secs(5);
    let no_prompt_snapshot = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, no_prompt_deadline);
        let attention_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude needs attention"
            })
            .count();
        let completion_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude finished its turn"
            })
            .count();
        let failure_notice = snapshot.notifications.iter().any(|notification| {
            notification.title == "Claude" && notification.body == "Claude stopped with an error"
        });
        assert!(
            snapshot.agent_alert_events.is_empty(),
            "the no-prompt interleaving must not create a lifecycle alert: {:?}",
            snapshot.agent_alert_events
        );
        if attention_notices >= 1 && completion_notices >= 1 && failure_notice {
            break snapshot;
        }
        assert!(
            Instant::now() < no_prompt_deadline,
            "generic notices were lost before the lifecycle owner could admit a no-prompt event: {:?}",
            snapshot
                .notifications
                .iter()
                .map(|notification| (&notification.title, &notification.body))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(5));
    };
    let fallback_bodies = ["Claude needs attention", "Claude finished its turn"];
    for body in fallback_bodies {
        let notice = no_prompt_snapshot
            .notifications
            .iter()
            .find(|notification| notification.title == "Claude" && notification.body == body)
            .unwrap_or_else(|| panic!("expected the {body:?} fallback notice"));
        assert_eq!(
            notice.source, "osc777-inbox",
            "fallback source for {body:?}"
        );
        assert_eq!(
            notice.desktop,
            Some(false),
            "fallback desktop policy for {body:?}"
        );
        assert_eq!(
            notice.delivery.state,
            DesktopDeliveryState::Suppressed,
            "fallback delivery must be suppressed for {body:?}"
        );
        assert_eq!(
            notice.delivery.reason.as_deref(),
            Some("disabled"),
            "fallback suppression reason for {body:?}"
        );
    }
    assert!(
        no_prompt_snapshot
            .native_notifications
            .desired
            .iter()
            .all(|request| !fallback_bodies.contains(&request.body.as_str())),
        "silent fallback notices must not create native notification desires: {:?}",
        no_prompt_snapshot.native_notifications.desired
    );
    let unknown_agent = no_prompt_snapshot
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::ClaudeLocal)
        .unwrap_or_else(|| {
            let agents: Vec<_> = no_prompt_snapshot
                .agent_lifecycle
                .iter()
                .map(|agent| {
                    (
                        agent.provider,
                        agent.activity,
                        agent.connection,
                        agent.prompt_id,
                    )
                })
                .collect();
            let terminal_output: Vec<_> = no_prompt_snapshot
                .frames
                .iter()
                .map(|frame| {
                    (
                        frame.session_id,
                        frame
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.viewport.contents.clone()),
                    )
                })
                .collect();
            panic!(
                "SessionStart should register the production local lifecycle owner; agents={agents:?}, last_error={:?}, terminal_output={terminal_output:?}",
                no_prompt_snapshot.last_error
            )
        });
    assert_eq!(unknown_agent.prompt_id, None);
    assert_eq!(
        unknown_agent.activity,
        zmux_desktop_runtime::agent_lifecycle::Activity::Unknown
    );

    let provider_env = fs::read_to_string(&provider_env).unwrap();
    assert!(
        provider_env
            .lines()
            .any(|line| line == format!("root={}", shell_root.display())),
        "provider did not inherit its exact session root: {provider_env}"
    );
    assert!(
        provider_env
            .lines()
            .any(|line| line == "lifecycle_alerts=1"),
        "provider did not inherit the lifecycle alert marker: {provider_env}"
    );
    let provider_cli = provider_env
        .lines()
        .find_map(|line| line.strip_prefix("cli="))
        .map(PathBuf::from)
        .expect("provider's exact bundled hook CLI");
    let event_route = provider_env
        .lines()
        .find_map(|line| line.strip_prefix("route="))
        .map(PathBuf::from)
        .expect("provider's local hook socket");
    let provider_session = provider_env
        .lines()
        .find_map(|line| line.strip_prefix("provider_session="))
        .expect("provider session ID");
    let provider_pid = provider_env
        .lines()
        .find_map(|line| line.strip_prefix("provider_pid="))
        .expect("provider process ID")
        .parse::<u32>()
        .unwrap();
    let provider_identity = zmux_session_runtime::child::process_identity(provider_pid).unwrap();
    let route_metadata = fs::symlink_metadata(&event_route).unwrap();
    assert!(route_metadata.file_type().is_socket());
    let mut unrelated_shell = Command::new("/bin/sh")
        .args([
            "-c",
            "\"$ZMUX_CLI_PATH\" hooks claude event UserPromptSubmit; result=$?; exit $result",
        ])
        .env("ZMUX_CLI_PATH", provider_cli)
        .env(ROUTE_ENV, event_route)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let shell_identity =
        zmux_session_runtime::child::process_identity(unrelated_shell.id()).unwrap();
    let test_identity = zmux_session_runtime::child::process_identity(std::process::id()).unwrap();
    assert_eq!(shell_identity.parent_pid, test_identity.pid);
    assert_ne!(shell_identity.parent_pid, provider_identity.pid);
    assert_eq!(shell_identity.uid, test_identity.uid);
    assert!(matches!(
        shell_identity
            .executable
            .file_name()
            .and_then(|name| name.to_str()),
        Some("sh" | "dash" | "bash" | "zsh")
    ));
    assert!(matches!(
        shell_identity.executable.parent(),
        Some(parent) if parent == Path::new("/bin") || parent == Path::new("/usr/bin")
    ));
    unrelated_shell
        .stdin
        .take()
        .unwrap()
        .write_all(
            &json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":provider_session,
                "prompt_id":"987e6543-e21b-4da3-b456-426614174001",
                "permission_mode":"default",
                "prompt":"untrusted same-UID shell"
            })
            .to_string()
            .into_bytes(),
        )
        .unwrap();
    let unrelated_output = unrelated_shell.wait_with_output().unwrap();
    assert!(
        unrelated_output.status.success(),
        "unrelated shell hook command should fail closed without provider acknowledgement: {}",
        String::from_utf8_lossy(&unrelated_output.stderr)
    );
    assert_eq!(
        provider_identity,
        zmux_session_runtime::child::process_identity(provider_pid).unwrap(),
        "the exact provider process must remain live across the unrelated shell attempt"
    );
    let rejected_deadline = Instant::now() + Duration::from_secs(2);
    let after_unrelated_shell = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, rejected_deadline);
        let rejected = snapshot.frames.iter().any(|frame| {
            frame.snapshot.as_ref().is_some_and(|terminal| {
                terminal
                    .viewport
                    .contents
                    .contains("local hook unavailable; no acknowledgement inferred")
            })
        });
        if rejected {
            break snapshot;
        }
        assert!(
            Instant::now() < rejected_deadline,
            "the live hook socket did not record rejection of the unrelated same-UID shell"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let still_unknown = after_unrelated_shell
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::ClaudeLocal)
        .expect("registered local agent after unrelated shell attempt");
    assert_eq!(still_unknown.prompt_id, None);
    assert_eq!(
        still_unknown.activity,
        zmux_desktop_runtime::agent_lifecycle::Activity::Unknown
    );
    assert!(after_unrelated_shell.agent_alert_events.is_empty());

    let fallback_cursor = no_prompt_snapshot.notification_cursor;
    fs::write(&tracked_hooks_release, b"continue").unwrap();
    let tracked_hooks_deadline = Instant::now() + Duration::from_secs(8);
    while !hooks_done.exists() {
        assert!(
            Instant::now() < tracked_hooks_deadline,
            "real provider did not execute the tracked prompt/notification hooks"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let alerts_deadline = Instant::now() + Duration::from_secs(5);
    let notice_snapshot = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, alerts_deadline);
        let needs_user = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.kind == zmux_desktop_runtime::agent_lifecycle::AlertKind::NeedsUser
            })
            .count();
        let completed = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.kind == zmux_desktop_runtime::agent_lifecycle::AlertKind::Completed
            })
            .count();
        let failure_notice = snapshot.notifications.iter().any(|notification| {
            notification.title == "Claude" && notification.body == "Claude stopped with an error"
        });
        if needs_user == 1 && completed == 1 && failure_notice {
            break snapshot;
        }
        assert!(
            Instant::now() < alerts_deadline,
            "lifecycle events and preserved terminal notices were not all delivered: events={:?}, notifications={:?}, agents={:?}, last_error={:?}, terminal_output={:?}",
            snapshot
                .agent_alert_events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            snapshot
                .notifications
                .iter()
                .map(|notification| (
                    &notification.title,
                    &notification.body,
                    &notification.agent_alert_kind
                ))
                .collect::<Vec<_>>(),
            snapshot
                .agent_lifecycle
                .iter()
                .map(|agent| (agent.activity, agent.connection, agent.prompt_id))
                .collect::<Vec<_>>(),
            snapshot.last_error,
            snapshot
                .frames
                .iter()
                .map(|frame| (
                    frame.session_id,
                    frame
                        .snapshot
                        .as_ref()
                        .map(|terminal| terminal.viewport.contents.clone())
                ))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("needs_user"))
            .count(),
        1,
        "needs-user lifecycle alert was duplicated"
    );
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("completed"))
            .count(),
        1,
        "completion lifecycle alert was duplicated"
    );
    assert_eq!(
        notice_snapshot.agent_lifecycle[0].activity,
        zmux_desktop_runtime::agent_lifecycle::Activity::Working,
        "explicit Stop continuation after idle_prompt must restore Working"
    );
    let new_silent_fallbacks: Vec<_> = notice_snapshot
        .notifications
        .iter()
        .filter(|notification| {
            notification.id > fallback_cursor && notification.source == "osc777-inbox"
        })
        .collect();
    assert_eq!(
        new_silent_fallbacks.len(),
        1,
        "only the typed-invalid known permission Notification needs a silent fallback; unreadable, auth-success, and receipt-covered hooks must not add one: {:?}",
        new_silent_fallbacks
            .iter()
            .map(|notification| (&notification.body, notification.id))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        notice_snapshot
            .notification_cursor
            .saturating_sub(fallback_cursor),
        3,
        "tracked permission/completion create tagged lifecycle items, while late explicit continuation adds no fallback"
    );
    assert_eq!(new_silent_fallbacks[0].body, "Claude needs attention");
    assert_eq!(new_silent_fallbacks[0].desktop, Some(false));
    assert_eq!(
        new_silent_fallbacks[0].delivery.state,
        DesktopDeliveryState::Suppressed
    );
    assert_eq!(
        new_silent_fallbacks[0].delivery.reason.as_deref(),
        Some("disabled")
    );
    assert!(
        notice_snapshot
            .native_notifications
            .desired
            .iter()
            .all(|request| !fallback_bodies.contains(&request.body.as_str())),
        "silent inbox fallback must not create native notification desire"
    );
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude"
                    && notification.body == "Claude stopped with an error"
            })
            .count(),
        1,
        "StopFailure OSC notice should remain available"
    );
    fs::write(&provider_release, b"release").unwrap();
    let cleanup_deadline = Instant::now() + Duration::from_secs(8);
    while auto_directory.exists() {
        assert!(
            Instant::now() < cleanup_deadline,
            "automatic plugin cleanup did not remove the generation after provider exit"
        );
        thread::sleep(Duration::from_millis(5));
    }
    running.shutdown();
}

#[test]
fn automatic_local_codex_uses_its_exact_real_desktop_terminal_route() {
    let dir = Directory::new();
    let bin = dir.path.join("provider-bin");
    fs::create_dir(&bin).unwrap();
    let codex = bin.join("codex");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/provider-synthetic-codex.py"),
        &codex,
    )
    .unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();

    let state = dir.path.join("provider-state");
    let report = dir.path.join("provider-config-report");
    let calls = dir.path.join("provider-calls");
    let foreign_probe = dir.path.join("foreign-probe");
    let foreign_ack = dir.path.join("foreign-ack");
    let diagnostics = dir.path.join("codex-app-server-diagnostics");
    let stop_probe = dir.path.join("stop-probe");
    let stop_result = dir.path.join("stop-result");
    let release = dir.path.join("provider-release");
    let socket = dir.path.join("codex-app-server.sock");
    let daemon_pid = dir.path.join("codex-app-server.pid");
    fs::write(&state, "idle").unwrap();

    let private_contents = dir.path.join("deps");
    fs::create_dir(&private_contents).unwrap();
    let test_binary = std::env::current_exe().unwrap();
    let private_test_binary = private_contents.join(test_binary.file_name().unwrap());
    fs::copy(&test_binary, &private_test_binary).unwrap();
    fs::set_permissions(&private_test_binary, fs::Permissions::from_mode(0o700)).unwrap();
    let mut paths = vec![bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).unwrap();
    let long_temporary_root = dir.path.join("provider-temporary-root-".repeat(6));
    fs::create_dir(&long_temporary_root).unwrap();
    fs::set_permissions(&long_temporary_root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Command::new(private_test_binary);
    child.args([
        "--ignored",
        "--exact",
        "automatic_local_codex_uses_its_exact_real_desktop_terminal_route_child",
        "--nocapture",
    ]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("ZMUX_") {
            child.env_remove(name);
        }
    }
    child
        .env("HOME", &dir.path)
        .env("TMPDIR", &long_temporary_root)
        .env("PATH", path)
        .env("CODEX_HOME", dir.path.join("codex-home"))
        .env("FIXTURE_LOCAL_CODEX_STATE", state)
        .env("FIXTURE_LOCAL_CODEX_REPORT", report)
        .env("FIXTURE_LOCAL_CODEX_CALLS", calls)
        .env("FIXTURE_LOCAL_CODEX_FOREIGN_PROBE", foreign_probe)
        .env("FIXTURE_LOCAL_CODEX_FOREIGN_ACK", foreign_ack)
        .env("FIXTURE_LOCAL_CODEX_DIAGNOSTICS", diagnostics)
        .env("FIXTURE_LOCAL_CODEX_STOP_PROBE", stop_probe)
        .env("FIXTURE_LOCAL_CODEX_STOP_RESULT", stop_result)
        .env("FIXTURE_LOCAL_CODEX_RELEASE", release)
        .env("FIXTURE_LOCAL_CODEX_SOCKET", socket)
        .env("FIXTURE_LOCAL_CODEX_DAEMON_PID", daemon_pid)
        .env("FIXTURE_LOCAL_CODEX_KEEP_DAEMON", "1")
        .env_remove("XDG_RUNTIME_DIR");
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "local Codex Desktop route regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "invoked by the parent test with an isolated Codex fixture and HOME"]
fn automatic_local_codex_uses_its_exact_real_desktop_terminal_route_child() {
    let root = PathBuf::from(std::env::var_os("HOME").expect("private fixture HOME"));
    let contents_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let _bundled_cli = BundledCli::new(&contents_dir);
    let state = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_STATE").unwrap());
    let report = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_REPORT").unwrap());
    let calls = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_CALLS").unwrap());
    let foreign_probe =
        PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_FOREIGN_PROBE").unwrap());
    let foreign_ack = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_FOREIGN_ACK").unwrap());
    let diagnostics = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_DIAGNOSTICS").unwrap());
    let stop_probe = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_STOP_PROBE").unwrap());
    let stop_result = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_STOP_RESULT").unwrap());
    let release = PathBuf::from(std::env::var_os("FIXTURE_LOCAL_CODEX_RELEASE").unwrap());
    let shell_rc = root.join("codex-shell.rc");
    fs::write(
        &shell_rc,
        format!(
            "PS1=\nexport ZMUX_FAKE_CODEX_STATE='{}'\nexport ZMUX_FAKE_CODEX_REPORT='{}'\nexport ZMUX_FAKE_CODEX_CALLS='{}'\nexport ZMUX_FAKE_CODEX_FOREIGN_PROBE='{}'\nexport ZMUX_FAKE_CODEX_FOREIGN_ACK='{}'\nexport ZMUX_FAKE_CODEX_DIAGNOSTICS='{}'\nexport ZMUX_FAKE_CODEX_STOP_PROBE='{}'\nexport ZMUX_FAKE_CODEX_STOP_RESULT='{}'\nexport ZMUX_FAKE_CODEX_RELEASE='{}'\nexport ZMUX_FAKE_CODEX_SOCKET='{}'\nexport ZMUX_FAKE_CODEX_DAEMON_PID='{}'\nexport ZMUX_FAKE_CODEX_KEEP_DAEMON=1\nexport PATH='{}':\"$PATH\"\nprintf 'LOCAL_CODEX_SHELL_READY\\n'\n",
            state.display(),
            report.display(),
            calls.display(),
            foreign_probe.display(),
            foreign_ack.display(),
            diagnostics.display(),
            stop_probe.display(),
            stop_result.display(),
            release.display(),
            std::env::var_os("FIXTURE_LOCAL_CODEX_SOCKET")
                .map(PathBuf::from)
                .unwrap()
                .display(),
            std::env::var_os("FIXTURE_LOCAL_CODEX_DAEMON_PID")
                .map(PathBuf::from)
                .unwrap()
                .display(),
            root.join("provider-bin").display(),
        ),
    )
    .unwrap();
    let baseline_shell_roots = shell_integration_roots();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                shell_rc.to_string_lossy().into_owned(),
                "-i".into(),
            ],
            cwd: Some(root.clone()),
        })
        .unwrap(),
    );
    #[cfg(target_os = "macos")]
    let control_socket = root.join("Library/Application Support/zmux/control.sock");
    #[cfg(not(target_os = "macos"))]
    let control_socket = root.join(".local/run/zmux/control.sock");
    let server = ControlServer::start(&control_socket, Arc::clone(&desktop)).unwrap();
    let mut running = RunningDesktop {
        desktop,
        server: Some(server),
    };

    let startup_deadline = Instant::now() + Duration::from_secs(10);
    let (session_id, _shell_root) = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, startup_deadline);
        let session_id = snapshot
            .model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| pane.surfaces.iter())
            .find_map(|surface| surface.session_id);
        if let Some(session_id) = session_id {
            let ready = snapshot.control_frames.iter().any(|frame| {
                frame.session_id == session_id
                    && frame.snapshot.as_ref().is_some_and(|terminal| {
                        terminal.state == zmux_session_runtime::State::Running
                            && terminal
                                .viewport
                                .contents
                                .contains("LOCAL_CODEX_SHELL_READY")
                    })
            });
            let shell_root = shell_integration_roots()
                .difference(&baseline_shell_roots)
                .find(|path| path.join("provider-launch.py").is_file())
                .cloned();
            if ready && let Some(shell_root) = shell_root {
                break (session_id, shell_root);
            }
        }
        assert!(
            Instant::now() < startup_deadline,
            "real Desktop Codex terminal did not reach interactive startup"
        );
        thread::sleep(Duration::from_millis(5));
    };

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 71,
            payload: DesktopCommand::Input {
                session_id,
                text: "command codex fixture-daemon-start && codex\n".into(),
            },
        })
        .unwrap();
    // Startup has its own 10 second fixture budget; the production status
    // probe still requires an already-running daemon within two seconds.
    let output_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let terminal = terminal_output_for_until(&running.desktop, session_id, output_deadline);
        if terminal.contains("CODEX_FIXTURE_READY") {
            break;
        }
        assert!(
            Instant::now() < output_deadline,
            "automatic Codex launch did not start in its Desktop terminal: {terminal}"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let initial = wait_local_codex(
        &running.desktop,
        |agent| agent.activity == Activity::Idle,
        "local Codex baseline did not attach to the existing root thread",
    );
    assert_eq!(initial.connection, Connection::Connected);
    assert_eq!(initial.session_id, session_id);
    assert_eq!(initial.prompt_id, None);
    let config_report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(config_report["existing_session_start_preserved"], true);
    assert_eq!(config_report["zmux_session_start_added"], true);
    assert_eq!(config_report["stop_override_added"], false);
    assert_eq!(fs::read_to_string(&calls).unwrap(), "launch\n");
    assert_eq!(local_codex_completion_count(&running.desktop), 0);

    fs::write(&foreign_probe, b"send exact foreign thread notification").unwrap();
    let foreign_deadline = Instant::now() + Duration::from_secs(3);
    while !foreign_ack.is_file() {
        assert!(
            Instant::now() < foreign_deadline,
            "synthetic daemon did not send the foreign thread notification; producer={}; {}; terminal={}",
            codex_fixture_diagnostics(&root.join("codex-app-server-diagnostics")),
            codex_fixture_liveness(
                &root.join("codex-app-server.pid"),
                &root.join("codex-app-server.sock")
            ),
            bounded_fixture_terminal_output(&running.desktop, session_id, &root, foreign_deadline)
        );
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(150));
    let after_foreign = desktop_snapshot::read(&running.desktop);
    let local_agents = after_foreign
        .agent_lifecycle
        .iter()
        .filter(|agent| agent.provider == Provider::CodexLocal)
        .collect::<Vec<_>>();
    assert_eq!(local_agents.len(), 1);
    assert_eq!(local_agents[0].activity, Activity::Idle);
    assert_eq!(local_agents[0].prompt_id, None);
    assert!(after_foreign.agent_alert_events.is_empty());

    fs::write(&stop_probe, b"attempt unsupported Stop decision").unwrap();
    let stop_deadline = Instant::now() + Duration::from_secs(3);
    while !stop_result.is_file() {
        assert!(
            Instant::now() < stop_deadline,
            "synthetic Codex Stop hook was not exercised"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_ne!(fs::read_to_string(&stop_result).unwrap(), "0");
    assert_eq!(
        wait_local_codex(
            &running.desktop,
            |agent| agent.activity == Activity::Idle,
            "Codex Stop hook changed activity without app-server evidence",
        )
        .provider_session_id,
        initial.provider_session_id
    );
    assert_eq!(local_codex_completion_count(&running.desktop), 0);

    fs::write(&state, b"start").unwrap();
    let working = wait_local_codex(
        &running.desktop,
        |agent| agent.activity == Activity::Working,
        "exact Codex root thread did not become Working",
    );
    assert_eq!(working.connection, Connection::Connected);
    assert_eq!(working.provider_session_id, initial.provider_session_id);
    assert!(working.prompt_id.is_some());
    assert_eq!(local_codex_completion_count(&running.desktop), 0);

    fs::write(&state, b"complete").unwrap();
    let completion_deadline = Instant::now() + Duration::from_secs(8);
    let completed = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, completion_deadline);
        let count = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::CodexLocal && event.kind == AlertKind::Completed
            })
            .count();
        if count == 1
            && let Some(agent) = snapshot.agent_lifecycle.iter().find(|agent| {
                agent.provider == Provider::CodexLocal && agent.activity == Activity::Idle
            })
        {
            break agent.clone();
        }
        assert!(
            Instant::now() < completion_deadline,
            "local Codex completion was not admitted exactly once: events={:?}",
            snapshot.agent_alert_events
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(completed.provider_session_id, initial.provider_session_id);
    assert_eq!(completed.prompt_id, working.prompt_id);

    fs::write(&release, b"exit provider").unwrap();
    let exit_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, exit_deadline);
        let terminal = terminal_output_from_snapshot(&snapshot, session_id);
        let ended = snapshot.agent_lifecycle.iter().find(|agent| {
            agent.provider == Provider::CodexLocal && agent.connection == Connection::Disconnected
        });
        if terminal.contains("CODEX_FIXTURE_DONE")
            && let Some(ended) = ended
        {
            assert_eq!(ended.activity, Activity::Unknown);
            assert_eq!(
                snapshot
                    .agent_alert_events
                    .iter()
                    .filter(|event| {
                        event.provider == Provider::CodexLocal && event.kind == AlertKind::Completed
                    })
                    .count(),
                1
            );
            break;
        }
        assert!(
            Instant::now() < exit_deadline,
            "local Codex exit did not mark its exact provider session disconnected: terminal={terminal:?}; agents={:?}",
            snapshot.agent_lifecycle
        );
        thread::sleep(Duration::from_millis(10));
    }

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 72,
            payload: DesktopCommand::Input {
                session_id,
                text: "codex --help\n".into(),
            },
        })
        .unwrap();
    let help_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let terminal = terminal_output_for_until(&running.desktop, session_id, help_deadline);
        if terminal.contains("CODEX_HELP_PASSTHROUGH") {
            assert_eq!(fs::read_to_string(&calls).unwrap(), "launch\n");
            break;
        }
        assert!(
            Instant::now() < help_deadline,
            "Codex help was altered by automatic lifecycle setup: {terminal}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let mut context_findings = Vec::new();
    for (index, args) in [
        vec!["--cd", "project-B"],
        vec!["--profile=selected-profile"],
        vec!["-C", "project-B"],
        vec!["-p", "selected-profile"],
        vec!["mcp", "list"],
    ]
    .iter()
    .enumerate()
    {
        let marker = format!("CODEX_CONTEXT_CASE_{index}_DONE");
        running
            .desktop
            .submit(Request {
                version: 1,
                request_id: 80 + index as u64,
                payload: DesktopCommand::Input {
                    session_id,
                    text: format!(
                        "codex {}; printf 'CODEX_CONTEXT_CASE_%s_DONE\\n' '{index}'\n",
                        args.join(" ")
                    ),
                },
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !terminal_output_for_until(&running.desktop, session_id, deadline).contains(&marker) {
            assert!(
                Instant::now() < deadline,
                "context invocation did not finish"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        if report["args"] != json!(args) || report["zmux_session_start_added"] != false {
            context_findings.push(format!("{args:?} was changed to {:?}", report["args"]));
        }
        assert_eq!(
            fs::read_to_string(&calls).unwrap().lines().count(),
            index + 2
        );
    }
    assert!(
        context_findings.is_empty(),
        "context/subcommand arguments changed: {context_findings:?}"
    );
    running.shutdown();
}

#[test]
fn automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration() {
    let dir = Directory::new();
    let bin = dir.path.join("provider-bin");
    fs::create_dir(&bin).unwrap();
    let config = dir.path.join("user-opencode-config");
    let plugins = config.join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&plugins, fs::Permissions::from_mode(0o700)).unwrap();
    let config_contents = r#"{"model":"fixture/model","permission":{"*":"ask"}}"#;
    let user_plugin_contents = "export default async () => ({});\n";
    fs::write(config.join("opencode.json"), config_contents).unwrap();
    fs::write(plugins.join("user-plugin.js"), user_plugin_contents).unwrap();
    let runner = dir.path.join("provider-opencode-runner.mjs");
    let host = dir.path.join("provider-opencode-plugin-host.mjs");
    fs::write(
        &runner,
        include_str!("../../../crates/session-runtime/tests/fixtures/provider-opencode-runner.mjs"),
    )
    .unwrap();
    fs::write(
        &host,
        include_str!(
            "../../../crates/session-runtime/tests/fixtures/provider-opencode-plugin-host.mjs"
        ),
    )
    .unwrap();
    let opencode = bin.join("opencode");
    fs::write(
        &opencode,
        "#!/bin/sh\nif [ \"$#\" -eq 1 ] && [ \"$1\" = \"--help\" ] && [ \"${FIXTURE_OPENCODE_HELP_DELAY_MS+x}\" != x ]; then\n    if [ \"${FIXTURE_OPENCODE_STANDALONE_SUPPORT-}\" = \"0\" ]; then\n        printf 'FLAGS\\n  --server string  Connect to a server\\n'\n    else\n        printf 'FLAGS\\n  --standalone  Run with a private server\\n'\n    fi\n    exit 0\nfi\nexec node \"$FIXTURE_OPENCODE_RUNNER\" \"$FIXTURE_OPENCODE_PLUGIN_HOST\" \"$FIXTURE_OPENCODE_ROOT_SESSION\" \"$FIXTURE_OPENCODE_REPORT\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&opencode, fs::Permissions::from_mode(0o700)).unwrap();

    let calls = dir.path.join("opencode-calls");
    let report = dir.path.join("opencode-report");
    let help_delay_started = dir.path.join("opencode-help-delay-started");
    let help_delay_completed = dir.path.join("opencode-help-delay-completed");
    let root_session = "session:root/opaque";
    let control_socket = dir.path.join("control.sock");
    let shell_rc = dir.path.join("opencode-shell.rc");
    let baseline_shell_roots = shell_integration_roots();
    fs::write(
        &shell_rc,
        format!(
            "PS1=\nunset FIXTURE_OPENCODE_HELP_DELAY_MS\nexport PATH='{}':\"$PATH\"\nexport ZMUX_SOCKET_PATH='{}'\nexport OPENCODE_CONFIG_DIR='{}'\nexport FIXTURE_OPENCODE_RUNNER='{}'\nexport FIXTURE_OPENCODE_PLUGIN_HOST='{}'\nexport FIXTURE_OPENCODE_ROOT_SESSION='{}'\nexport FIXTURE_OPENCODE_REPORT='{}'\nexport FIXTURE_OPENCODE_CALLS='{}'\nexport FIXTURE_OPENCODE_USER_CONFIG='{}'\nexport FIXTURE_OPENCODE_USER_CONFIG_CONTENT='{}'\nexport FIXTURE_OPENCODE_USER_PLUGIN_CONTENT='{}'\nexport FIXTURE_OPENCODE_HELP_DELAY_STARTED='{}'\nexport FIXTURE_OPENCODE_HELP_DELAY_COMPLETED='{}'\nexport FIXTURE_OPENCODE_STANDALONE_SUPPORT=1\nzmux_fixture_ready_prompt() {{ printf 'LOCAL_OPENCODE_SHELL_READY\\n'; }}\nPROMPT_COMMAND=\"${{PROMPT_COMMAND:+$PROMPT_COMMAND; }}zmux_fixture_ready_prompt\"\n",
            bin.display(),
            control_socket.display(),
            config.display(),
            runner.display(),
            host.display(),
            root_session,
            report.display(),
            calls.display(),
            config.display(),
            config_contents,
            user_plugin_contents,
            help_delay_started.display(),
            help_delay_completed.display(),
        ),
    )
    .unwrap();

    let contents_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let _bundled_cli = BundledCli::new(&contents_dir);
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                shell_rc.to_string_lossy().into_owned(),
                "-i".into(),
            ],
            cwd: Some(dir.path.clone()),
        })
        .unwrap(),
    );
    let server = ControlServer::start(&control_socket, Arc::clone(&desktop)).unwrap();
    let mut running = RunningDesktop {
        desktop,
        server: Some(server),
    };

    let startup_deadline = Instant::now() + Duration::from_secs(10);
    let terminal_session = loop {
        let snapshot = desktop_snapshot::read_until(&running.desktop, startup_deadline);
        let session = snapshot
            .model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| pane.surfaces.iter())
            .find_map(|surface| surface.session_id);
        if let Some(session) = session {
            let ready = snapshot.control_frames.iter().any(|frame| {
                frame.session_id == session
                    && frame.snapshot.as_ref().is_some_and(|terminal| {
                        terminal.state == zmux_session_runtime::State::Running
                            && terminal
                                .viewport
                                .contents
                                .contains("LOCAL_OPENCODE_SHELL_READY")
                    })
            });
            let shell_ready = shell_integration_roots()
                .difference(&baseline_shell_roots)
                .any(|path| path.join("provider-launch.py").is_file());
            if ready && shell_ready {
                break session;
            }
        }
        assert!(
            Instant::now() < startup_deadline,
            "real Desktop OpenCode terminal did not reach interactive startup"
        );
        thread::sleep(Duration::from_millis(5));
    };

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 72,
            payload: DesktopCommand::Input {
                session_id: terminal_session,
                text: format!("opencode --session {root_session}\n"),
            },
        })
        .unwrap();
    let launch_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let terminal =
            terminal_output_for_until(&running.desktop, terminal_session, launch_deadline);
        if terminal.contains(r#"{"ready":true}"#) {
            break;
        }
        assert!(
            Instant::now() < launch_deadline,
            "automatic OpenCode launch did not reach its plugin host: {terminal}"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let launch_report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        launch_report["args"],
        json!(["--standalone", "--session", root_session])
    );
    assert_eq!(launch_report["config_directory_is_invocation"], true);
    assert_eq!(launch_report["config_is_symlink"], true);
    assert_eq!(launch_report["config_preserved"], true);
    assert_eq!(launch_report["user_plugin_is_symlink"], true);
    assert_eq!(launch_report["user_plugin_preserved"], true);
    assert_eq!(launch_report["zmux_plugin_count"], 1);
    assert_eq!(launch_report["zmux_plugin_mode"], 0o600);
    assert_eq!(
        fs::read(config.join("opencode.json")).unwrap(),
        config_contents.as_bytes()
    );
    assert_eq!(
        fs::read(plugins.join("user-plugin.js")).unwrap(),
        user_plugin_contents.as_bytes()
    );

    let send = |request_id, operation: &str| {
        running
            .desktop
            .submit(Request {
                version: 1,
                request_id,
                payload: DesktopCommand::Input {
                    session_id: terminal_session,
                    text: format!("{{\"op\":\"{operation}\"}}\n"),
                },
            })
            .unwrap();
    };

    send(73, "created");
    let initial = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Idle,
        "OpenCode root session did not publish its baseline",
    );
    assert_eq!(initial.connection, Connection::Connected);
    assert_eq!(initial.session_id, terminal_session);
    assert_eq!(initial.prompt_id, None);
    send(74, "foreign");
    thread::sleep(Duration::from_millis(100));
    let after_foreign = desktop_snapshot::read(&running.desktop);
    let local_agents = after_foreign
        .agent_lifecycle
        .iter()
        .filter(|agent| agent.provider == Provider::OpenCodeLocal)
        .collect::<Vec<_>>();
    assert_eq!(local_agents.len(), 1);
    assert_eq!(
        local_agents[0].provider_session_id,
        initial.provider_session_id
    );
    assert_eq!(local_agents[0].activity, Activity::Idle);
    assert!(after_foreign.agent_alert_events.is_empty());

    send(75, "start");
    let working = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Working,
        "OpenCode prompt callback did not become Working",
    );
    assert_eq!(working.provider_session_id, initial.provider_session_id);
    assert!(working.prompt_id.is_some());
    send(76, "wait");
    let waiting = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Waiting,
        "OpenCode permission callback did not become Waiting",
    );
    assert_eq!(waiting.prompt_id, working.prompt_id);
    assert_eq!(
        local_provider_alert_count(
            &running.desktop,
            Provider::OpenCodeLocal,
            AlertKind::NeedsUser
        ),
        1
    );
    send(77, "resume");
    wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Working,
        "OpenCode permission reply did not resume the exact root turn",
    );
    send(78, "idle");
    wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Idle,
        "OpenCode idle snapshot did not refresh the root activity",
    );
    assert_eq!(
        local_provider_alert_count(
            &running.desktop,
            Provider::OpenCodeLocal,
            AlertKind::Completed
        ),
        0
    );
    send(79, "background");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        local_provider_alert_count(
            &running.desktop,
            Provider::OpenCodeLocal,
            AlertKind::Completed
        ),
        0
    );
    send(80, "complete");
    let completion_deadline = Instant::now() + Duration::from_secs(8);
    while local_provider_alert_count_until(
        &running.desktop,
        Provider::OpenCodeLocal,
        AlertKind::Completed,
        completion_deadline,
    ) != 1
    {
        assert!(
            Instant::now() < completion_deadline,
            "OpenCode success evidence did not complete its exact live root turn"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let completed = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Idle,
        "OpenCode completion did not leave the root session idle",
    );
    assert_eq!(completed.prompt_id, working.prompt_id);
    send(81, "next-start");
    let next_working = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Working,
        "second OpenCode root turn did not start",
    );
    assert_ne!(next_working.prompt_id, working.prompt_id);
    send(82, "interrupt");
    wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.activity == Activity::Idle,
        "OpenCode interruption did not return to idle",
    );
    assert_eq!(
        local_provider_alert_count(
            &running.desktop,
            Provider::OpenCodeLocal,
            AlertKind::Completed
        ),
        1
    );

    send(83, "shutdown");
    let disconnected = wait_local_provider(
        &running.desktop,
        Provider::OpenCodeLocal,
        |agent| agent.connection == Connection::Disconnected,
        "OpenCode process exit did not end its exact provider session",
    );
    assert_eq!(disconnected.activity, Activity::Unknown);

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 84,
            payload: DesktopCommand::Input {
                session_id: terminal_session,
                text: format!(
                    "export FIXTURE_OPENCODE_STANDALONE_SUPPORT=0\nopencode --session {root_session}\n"
                ),
            },
        })
        .unwrap();
    let fallback_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let terminal =
            terminal_output_for_until(&running.desktop, terminal_session, fallback_deadline);
        if terminal.contains("OPENCODE_FALLBACK_LAUNCHED") {
            break;
        }
        assert!(
            Instant::now() < fallback_deadline,
            "unsupported standalone mode did not fall back to one original OpenCode launch: {terminal}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let calls = fs::read_to_string(&calls).unwrap();
    let calls = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        calls.len(),
        2,
        "help probes do not count as provider launches"
    );
    assert_eq!(calls[1]["args"], json!(["--session", root_session]));
    assert_eq!(calls[1]["config_directory_is_invocation"], false);
    assert_eq!(calls[1]["config_preserved"], true);
    assert_eq!(calls[1]["user_plugin_is_symlink"], false);
    assert_eq!(calls[1]["user_plugin_preserved"], true);
    assert_eq!(calls[1]["zmux_plugin_count"], 0);
    assert_eq!(
        desktop_snapshot::read(&running.desktop)
            .agent_lifecycle
            .iter()
            .filter(|agent| agent.provider == Provider::OpenCodeLocal)
            .count(),
        1,
        "untracked fallback must not fabricate another lifecycle session"
    );
    assert!(
        terminal_output_for(&running.desktop, terminal_session)
            .contains("stage=standalone_option_not_advertised; elapsed_ms=")
    );

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 85,
            payload: DesktopCommand::Input {
                session_id: terminal_session,
                text: format!(
                    "export FIXTURE_OPENCODE_STANDALONE_SUPPORT=1 FIXTURE_OPENCODE_HELP_DELAY_MS=5000\nopencode --session {root_session}\n"
                ),
            },
        })
        .unwrap();
    let delayed_fallback_deadline = Instant::now() + Duration::from_secs(8);
    let delayed_calls = loop {
        let output = fs::read_to_string(dir.path.join("opencode-calls")).unwrap_or_default();
        if output.lines().count() >= 3 {
            break output
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
        }
        let terminal = terminal_output_for_until(
            &running.desktop,
            terminal_session,
            delayed_fallback_deadline,
        );
        assert!(
            Instant::now() < delayed_fallback_deadline,
            "slow OpenCode help preflight did not fail closed to one launch: {terminal}"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(delayed_calls.len(), 3);
    assert_eq!(delayed_calls[2]["args"], json!(["--session", root_session]));
    assert_eq!(delayed_calls[2]["config_directory_is_invocation"], false);
    assert_eq!(delayed_calls[2]["config_preserved"], true);
    assert_eq!(delayed_calls[2]["user_plugin_preserved"], true);
    assert_eq!(delayed_calls[2]["zmux_plugin_count"], 0);
    assert!(help_delay_started.is_file());
    let help_pid = fs::read_to_string(&help_delay_started)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    assert!(help_pid > 0);
    assert_eq!(
        kill(Pid::from_raw(help_pid), None),
        Err(nix::errno::Errno::ESRCH),
        "the delayed help child must be terminated and reaped before fallback"
    );
    assert!(
        !help_delay_completed.exists(),
        "timed-out OpenCode help preflight must stop its synthetic child"
    );
    // The calls file and the rendered PTY snapshot are independent boundaries.
    // Wait for this invocation's diagnostic, rather than inspecting an older
    // snapshot that can still contain the preceding fallback's launch marker.
    let elapsed = loop {
        let terminal = terminal_output_for_until(
            &running.desktop,
            terminal_session,
            delayed_fallback_deadline,
        );
        let elapsed = terminal
            .rsplit_once("stage=run_standalone_help_preflight; elapsed_ms=")
            .and_then(|(_, suffix)| suffix.split_once(')'))
            .and_then(|(elapsed, _)| elapsed.parse::<u64>().ok());
        if let Some(elapsed) = elapsed
            && terminal.contains("OPENCODE_FALLBACK_LAUNCHED")
        {
            break elapsed;
        }
        assert!(
            Instant::now() < delayed_fallback_deadline,
            "slow preflight diagnostic was not published within its original wait budget"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        (1900..=2700).contains(&elapsed),
        "two-second help preflight must finish inside the existing 2.7-second hook budget; elapsed_ms={elapsed}"
    );
    assert_eq!(
        desktop_snapshot::read(&running.desktop)
            .agent_lifecycle
            .iter()
            .filter(|agent| agent.provider == Provider::OpenCodeLocal)
            .count(),
        1,
        "slow preflight fallback must not fabricate another lifecycle session"
    );
    assert_eq!(
        fs::read(config.join("opencode.json")).unwrap(),
        config_contents.as_bytes()
    );
    assert_eq!(
        fs::read(plugins.join("user-plugin.js")).unwrap(),
        user_plugin_contents.as_bytes()
    );

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 86,
            payload: DesktopCommand::Input {
                session_id: terminal_session,
                text: "unset FIXTURE_OPENCODE_HELP_DELAY_MS\nopencode --help\n".into(),
            },
        })
        .unwrap();
    let help_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let terminal = terminal_output_for_until(&running.desktop, terminal_session, help_deadline);
        if terminal.contains("FLAGS") {
            break;
        }
        assert!(
            Instant::now() < help_deadline,
            "OpenCode help was altered by automatic lifecycle setup: {terminal}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    running.shutdown();
}

#[test]
fn provider_command_exposes_only_session_scoped_codex_and_opencode_tracking() {
    let dir = Directory::new();
    let output = dir
        .command(&["hooks", "codex", "install", "--directory", "/tmp/nope"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("hooks supports claude and the session-scoped provider auto adapters")
    );

    let usage = dir.command(&["hooks", "provider"]).output().unwrap();
    assert!(!usage.status.success());
    assert!(
        String::from_utf8_lossy(&usage.stderr)
            .contains("hooks provider requires auto codex|opencode")
    );
    assert!(!dir.path.join("provider-called").exists());
}

#[test]
fn uninstall_retains_identical_bytes_at_an_unowned_replacement_inode() {
    use std::os::unix::fs::MetadataExt;
    let dir = Directory::new();
    let installed = dir.install();
    let config = dir
        .plugin()
        .join(installed["generation"].as_str().unwrap())
        .join("hooks/hooks.json");
    let inode = fs::metadata(&config).unwrap().ino();
    let replacement = config.with_extension("replacement");
    fs::write(&replacement, fs::read(&config).unwrap()).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(replacement, &config).unwrap();
    assert_ne!(fs::metadata(&config).unwrap().ino(), inode);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], true);
    assert!(
        config.exists(),
        "same bytes do not prove ownership of a replacement file"
    );
}
#[test]
fn interrupted_uninstall_removes_only_remaining_verified_owned_artifacts() {
    let dir = Directory::new();
    let installed = dir.install();
    let generation = dir.plugin().join(installed["generation"].as_str().unwrap());
    // A prior cleanup may have unlinked one verified file before interruption.
    fs::remove_file(generation.join("hooks/hooks.json")).unwrap();
    let orphan = dir.plugin().join("unpublished-generation");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("keep"), b"unknown staging evidence").unwrap();
    let result = dir.call("uninstall");
    assert_eq!(result["cleanup_pending"], false);
    assert!(!generation.exists());
    assert_eq!(
        fs::read(orphan.join("keep")).unwrap(),
        b"unknown staging evidence"
    );
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
}
