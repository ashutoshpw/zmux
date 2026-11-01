//! Bounded reader for the Claude lifecycle stream carried by a managed SSH master.
//!
//! Frames are treated as unauthenticated provider claims from the selected SSH
//! account. The nonce correlates the exact terminal bootstrap; it is not provider
//! process authentication or a replay cursor.

use crate::agent_lifecycle::Target;
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    io::{self, Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::{
    agent_lifecycle::{AGENT_LIFECYCLE_PROTOCOL_VERSION, Frame, MAX_AGENT_LIFECYCLE_FRAME_BYTES},
    connection::ManagedConnection,
};
use zmux_session_runtime::shell_integration::OpenRequestNonce;

const MAX_PARTIAL_FRAME_BYTES: usize = MAX_AGENT_LIFECYCLE_FRAME_BYTES;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RECONNECT_ATTEMPTS: usize = 5;
const MAX_RECONNECT_GENERATIONS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub target: Target,
    pub launch_id: u64,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub nonce: Uuid,
}

impl Binding {
    /// Stable owner identity shared by every generation of one launched stream.
    /// Control transitions use this so a queued reconnect is not discarded just
    /// because a later generation has already become the stream's latest binding.
    pub fn same_owner(self, other: Self) -> bool {
        self.target == other.target
            && self.launch_id == other.launch_id
            && self.runtime_id == other.runtime_id
            && self.nonce == other.nonce
    }

    /// Frame claims are admitted only from the latest handshake generation.
    pub fn is_current_generation(self, other: Self) -> bool {
        self == other
    }
}

pub enum Message {
    Connected {
        binding: Binding,
        previous_generation: Option<Uuid>,
    },
    Frame {
        binding: Binding,
        frame: Frame,
    },
    Lost {
        binding: Binding,
    },
}

/// Lifecycle side-channel ownership attached to one exact terminal launch.
/// Dropping the owner cancels its reader without waiting on the desktop worker.
pub struct Stream {
    binding: Arc<Mutex<Binding>>,
    cancellation: Arc<AtomicBool>,
}

impl Stream {
    pub fn start(
        connection: Arc<ManagedConnection>,
        nonce: OpenRequestNonce,
        target: Target,
        launch_id: u64,
        sender: SyncSender<Message>,
    ) -> io::Result<Self> {
        let binding = Binding {
            target,
            launch_id,
            runtime_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
            nonce: Uuid::parse_str(&nonce.remote_lifecycle_token())
                .expect("generated SSH lifecycle token is a UUID"),
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let binding_state = Arc::new(Mutex::new(binding));
        let thread_cancellation = Arc::clone(&cancellation);
        let thread_binding_state = Arc::clone(&binding_state);
        thread::Builder::new()
            .name(format!("zmux-agent-ssh-{}", target.session_id.0))
            .spawn(move || {
                if let Err(error) = supervise(
                    connection,
                    nonce,
                    binding,
                    thread_binding_state,
                    sender.clone(),
                    &thread_cancellation,
                ) && !thread_cancellation.load(Ordering::Acquire)
                {
                    eprintln!("zmux remote agent lifecycle supervisor ended: {error}");
                }
            })?;
        Ok(Self {
            binding: binding_state,
            cancellation,
        })
    }

    pub fn binding(&self) -> Binding {
        *self
            .binding
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
    }
}

struct ChildOwner(Child);

impl Drop for ChildOwner {
    fn drop(&mut self) {
        if let Err(error) = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL)
            && error != nix::errno::Errno::ESRCH
        {
            eprintln!("zmux remote lifecycle process cancellation: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("zmux remote lifecycle process reap: {error}");
        }
    }
}

fn supervise(
    connection: Arc<ManagedConnection>,
    nonce: OpenRequestNonce,
    mut binding: Binding,
    shared_binding: Arc<Mutex<Binding>>,
    sender: SyncSender<Message>,
    cancellation: &AtomicBool,
) -> Result<(), String> {
    let mut connected = false;
    let mut reconnect_generations = 0;
    let mut failures = 0;
    let first_handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        let previous_generation = binding.generation;
        let mut handshake_generation = None;
        let attempt_handshake_deadline = (!connected).then_some(first_handshake_deadline);
        let mut attempt_state = AttemptState {
            binding: &mut binding,
            shared_binding: &shared_binding,
            connected: &mut connected,
            handshake_generation: &mut handshake_generation,
        };
        let result = run_attempt(
            Arc::clone(&connection),
            nonce,
            &mut attempt_state,
            sender.clone(),
            cancellation,
            attempt_handshake_deadline,
        );
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        if !connected {
            // The login shell may still be writing its nonce-scoped integration
            // files when the ControlMaster socket becomes usable. Retry only
            // this startup race, on the same managed route and within one
            // bounded first-handshake deadline. Until Active is validated, the
            // remote notice hook stays unlocked and emits its generic fallback.
            failures += 1;
            if failures >= MAX_RECONNECT_ATTEMPTS || Instant::now() >= first_handshake_deadline {
                return result.and(Err(
                    "managed SSH lifecycle stream ended before handshake deadline".into(),
                ));
            }
            if let Err(error) = &result {
                eprintln!("zmux remote agent lifecycle startup retry: {error}");
            }
            let delay = reconnect_delay(failures)
                .min(first_handshake_deadline.saturating_duration_since(Instant::now()));
            if !sleep_cancellable(delay, cancellation) {
                return Ok(());
            }
            continue;
        }
        if let Some(handshake_generation) = handshake_generation {
            if handshake_generation != previous_generation {
                reconnect_generations += 1;
            }
            send_cancellable(&sender, Message::Lost { binding }, cancellation)?;
        }
        if handshake_generation.is_some() {
            failures = 0;
        }
        failures += 1;
        if failures >= MAX_RECONNECT_ATTEMPTS || reconnect_generations >= MAX_RECONNECT_GENERATIONS
        {
            return result.and(Err("managed SSH lifecycle reconnect limit reached".into()));
        }
        if !sleep_cancellable(reconnect_delay(failures), cancellation) {
            return Ok(());
        }
        if let Err(error) = result {
            eprintln!("zmux remote agent lifecycle stream retry: {error}");
        }
    }
}

fn reconnect_delay(failures: usize) -> Duration {
    match failures {
        0 | 1 => Duration::from_millis(250),
        2 => Duration::from_millis(500),
        3 => Duration::from_secs(1),
        _ => Duration::from_secs(2),
    }
}

/// Preserve transition order across bounded-channel pressure. The supervisor
/// waits until a loss or a reconnect handshake has entered the same queue before
/// it proceeds to the next generation.
pub(crate) fn send_cancellable<T>(
    sender: &SyncSender<T>,
    mut message: T,
    cancellation: &AtomicBool,
) -> Result<(), &'static str> {
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err("managed SSH lifecycle message cancelled");
        }
        match sender.try_send(message) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => message = returned,
            Err(TrySendError::Disconnected(_)) => {
                return Err("managed SSH lifecycle receiver closed");
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn sleep_cancellable(duration: Duration, cancellation: &AtomicBool) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if cancellation.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(
            Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    !cancellation.load(Ordering::Acquire)
}

struct AttemptState<'a> {
    binding: &'a mut Binding,
    shared_binding: &'a Mutex<Binding>,
    connected: &'a mut bool,
    handshake_generation: &'a mut Option<Uuid>,
}

fn run_attempt(
    connection: Arc<ManagedConnection>,
    nonce: OpenRequestNonce,
    state: &mut AttemptState<'_>,
    sender: SyncSender<Message>,
    cancellation: &AtomicBool,
    first_handshake_deadline: Option<Instant>,
) -> Result<(), String> {
    let spec = connection
        .agent_lifecycle_spec(&nonce)
        .map_err(|error| error.to_string())?;
    let mut command = Command::new(spec.program);
    command
        .args(spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = ChildOwner(
        command
            .spawn()
            .map_err(|error| format!("start managed SSH stream: {error}"))?,
    );
    let mut input = child
        .0
        .stdin
        .take()
        .ok_or_else(|| "managed SSH stream has no stdin pipe".to_owned())?;
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| "managed SSH stream has no stdout pipe".to_owned())?;
    let mut output = FileDescriptor::dup(&stdout)
        .map_err(|error| format!("open managed SSH stream output: {error}"))?;
    output
        .set_non_blocking(true)
        .map_err(|error| format!("configure managed SSH stream output: {error}"))?;
    drop(stdout);

    let started_at = Instant::now();
    let handshake_deadline =
        first_handshake_deadline.unwrap_or_else(|| started_at + HANDSHAKE_TIMEOUT);
    let mut last_frame_at = started_at;
    let mut pending = Vec::new();
    let mut hello_seen = false;
    let mut active_seen = false;
    let mut last_sequence = 0;
    let mut output_eof = false;
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Ok(());
        }
        let now = Instant::now();
        if (!active_seen && now >= handshake_deadline)
            || (active_seen && now.duration_since(last_frame_at) >= HEARTBEAT_TIMEOUT)
        {
            return Err("managed SSH lifecycle heartbeat expired".into());
        }
        let status = child
            .0
            .try_wait()
            .map_err(|error| format!("poll managed SSH stream: {error}"))?;
        if status.is_some() && output_eof {
            return Err("managed SSH lifecycle command exited".into());
        }

        let mut descriptors = [pollfd {
            fd: output.as_raw_file_descriptor(),
            events: POLLIN,
            revents: 0,
        }];
        if let Err(error) = poll(&mut descriptors, Some(POLL_INTERVAL))
            && !matches!(&error, filedescriptor::Error::Poll(error) if error.kind() == io::ErrorKind::Interrupted)
        {
            return Err(format!("poll managed SSH lifecycle output: {error}"));
        }
        if descriptors[0].revents == 0 {
            continue;
        }
        for _ in 0..4 {
            let mut bytes = [0; 4_096];
            match output.read(&mut bytes) {
                Ok(0) => {
                    output_eof = true;
                    break;
                }
                Ok(length) => {
                    pending.extend_from_slice(&bytes[..length]);
                    while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                        if newline > MAX_PARTIAL_FRAME_BYTES {
                            return Err(
                                "managed SSH lifecycle frame exceeded its byte bound".into()
                            );
                        }
                        let line: Vec<_> = pending.drain(..=newline).collect();
                        if line[..newline].is_empty() {
                            continue;
                        }
                        let frame: Frame =
                            serde_json::from_slice(&line[..newline]).map_err(|error| {
                                format!("decode managed SSH lifecycle frame: {error}")
                            })?;
                        if frame.version() != AGENT_LIFECYCLE_PROTOCOL_VERSION
                            || frame.nonce() != state.binding.nonce
                        {
                            return Err("managed SSH lifecycle handshake identity mismatch".into());
                        }
                        last_frame_at = Instant::now();
                        match &frame {
                            Frame::Hello { .. } if !hello_seen && last_sequence == 0 => {
                                hello_seen = true;
                                writeln!(
                                    input,
                                    "{{\"kind\":\"activate\",\"version\":{},\"nonce\":\"{}\"}}",
                                    AGENT_LIFECYCLE_PROTOCOL_VERSION,
                                    state.binding.nonce.simple()
                                )
                                .map_err(|error| {
                                    format!("activate managed SSH lifecycle stream: {error}")
                                })?;
                                input.flush().map_err(|error| {
                                    format!("flush managed SSH lifecycle activation: {error}")
                                })?;
                            }
                            Frame::Active { .. }
                                if hello_seen && !active_seen && last_sequence == 0 =>
                            {
                                active_seen = true;
                                let previous_generation = if *state.connected {
                                    let previous = state.binding.generation;
                                    state.binding.generation = Uuid::new_v4();
                                    Some(previous)
                                } else {
                                    None
                                };
                                *state.connected = true;
                                *state.handshake_generation = Some(state.binding.generation);
                                *state
                                    .shared_binding
                                    .lock()
                                    .unwrap_or_else(|poison| poison.into_inner()) = *state.binding;
                                send_cancellable(
                                    &sender,
                                    Message::Connected {
                                        binding: *state.binding,
                                        previous_generation,
                                    },
                                    cancellation,
                                )?;
                            }
                            Frame::Hello { .. } | Frame::Active { .. } => {
                                return Err(
                                    "duplicate or out-of-order managed SSH lifecycle handshake"
                                        .into(),
                                );
                            }
                            Frame::Gap { sequence, .. } => {
                                if !active_seen || *sequence == 0 || *sequence <= last_sequence {
                                    return Err(
                                        "managed SSH lifecycle gap is stale or precedes handshake"
                                            .into(),
                                    );
                                }
                                return Err(
                                    "remote Claude lifecycle hook ingress reported lost evidence"
                                        .into(),
                                );
                            }
                            Frame::Heartbeat { sequence, .. } | Frame::Hook { sequence, .. } => {
                                if !active_seen || *sequence == 0 || *sequence <= last_sequence {
                                    return Err("managed SSH lifecycle sequence is stale or precedes handshake".into());
                                }
                                last_sequence = *sequence;
                                if sender
                                    .try_send(Message::Frame {
                                        binding: *state.binding,
                                        frame,
                                    })
                                    .is_err()
                                {
                                    return Err(
                                        "managed SSH lifecycle event queue is full or closed"
                                            .into(),
                                    );
                                }
                            }
                        }
                    }
                    if pending.len() > MAX_PARTIAL_FRAME_BYTES {
                        return Err("managed SSH lifecycle frame exceeded its byte bound".into());
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted =>
                {
                    break;
                }
                Err(error) => return Err(format!("read managed SSH lifecycle output: {error}")),
            }
        }
        if output_eof {
            return Err(format!(
                "managed SSH lifecycle stream closed (hello={hello_seen}, active={active_seen})"
            ));
        }
    }
}

fn _protocol_type_anchor(_: Frame) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        sync::mpsc::TryRecvError,
    };
    use zmux_remote_runtime::SshDescriptor;
    use zmux_session_runtime::{
        Launch, Session, ShutdownPolicy,
        shell_integration::{OpenRequestNonce, OpenRequestRegistration, OpenRequestSource},
    };
    use zmux_workspace_core::{SessionId, SurfaceId};

    const FIXTURE_DRIVER: &str = r#"
import importlib.util
import json
from pathlib import Path
import sys
import time

configuration = Path(sys.argv[1])
diagnostic = Path(sys.argv[2])
fixture_path = Path(sys.argv[3])
spec = importlib.util.spec_from_file_location("native_ssh_fixture", fixture_path)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
with fixture.native_ssh_fixture(configuration, diagnostic):
    root = Path(json.loads(configuration.read_text())["root"])
    while not (root / ".fixture-controller-stop").exists():
        time.sleep(0.02)
"#;

    struct SshFixture {
        local_root: PathBuf,
        remote_root: PathBuf,
        configuration: serde_json::Value,
        controller: Option<Child>,
    }

    impl SshFixture {
        fn start() -> Self {
            let local_root = std::env::temp_dir().join(format!(
                "zmux-remote-agent-fixture-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            fs::create_dir(&local_root).expect("create private fixture controller directory");
            fs::set_permissions(&local_root, fs::Permissions::from_mode(0o700))
                .expect("secure private fixture controller directory");
            let config = local_root.join("fixture.json");
            let diagnostic = local_root.join("sshd.log");
            let fixture_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/native-ssh-fixture.py");
            let controller = Command::new("python3")
                .arg("-c")
                .arg(FIXTURE_DRIVER)
                .arg(&config)
                .arg(&diagnostic)
                .arg(fixture_script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start private loopback SSH fixture controller");
            let mut fixture = Self {
                local_root,
                remote_root: PathBuf::new(),
                configuration: serde_json::Value::Null,
                controller: Some(controller),
            };
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if config.is_file() {
                    fixture.configuration = serde_json::from_slice(
                        &fs::read(&config).expect("read private loopback SSH fixture config"),
                    )
                    .expect("decode private loopback SSH fixture config");
                    fixture.remote_root = PathBuf::from(
                        fixture.configuration["root"]
                            .as_str()
                            .expect("fixture remote root"),
                    );
                    break;
                }
                if let Some(status) = fixture
                    .controller
                    .as_mut()
                    .expect("fixture controller")
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
            fixture.install_bootstrap_gate();
            fixture
        }

        fn install_bootstrap_gate(&self) {
            let root = shell_quote(self.remote_root.to_str().expect("fixture path is UTF-8"));
            let shell = format!(
                "#!/bin/sh\ncase \"$SSH_ORIGINAL_COMMAND\" in\n  *ZMUX_REMOTE_LIFECYCLE=1*)\n    : > {root}/.bootstrap-started\n    while [ ! -e {root}/.release-bootstrap ]; do /bin/sleep 0.01; done\n    ;;\n  *claude-lifecycle-stream.py*)\n    printf 'attempt\\n' >> {root}/.stream-attempts\n    ;;\nesac\nif [ -n \"$SSH_ORIGINAL_COMMAND\" ]; then exec /bin/sh -c \"$SSH_ORIGINAL_COMMAND\"; else exec /bin/sh -i; fi\n"
            );
            fs::write(self.remote_root.join("shell"), shell)
                .expect("install deterministic remote bootstrap gate");
            fs::set_permissions(
                self.remote_root.join("shell"),
                fs::Permissions::from_mode(0o700),
            )
            .expect("secure deterministic remote bootstrap gate");
        }

        fn connection(&self) -> Arc<ManagedConnection> {
            Arc::new(
                ManagedConnection::create(
                    SshDescriptor::builder("fixture")
                        .expect("fixture destination")
                        .config_file(
                            self.configuration["clientConfig"]
                                .as_str()
                                .expect("fixture client config"),
                        )
                        .identity(
                            self.configuration["identity"]
                                .as_str()
                                .expect("fixture identity"),
                        )
                        .build()
                        .expect("build private loopback SSH descriptor"),
                )
                .expect("create managed SSH connection"),
            )
        }
    }

    impl Drop for SshFixture {
        fn drop(&mut self) {
            let _ = fs::write(self.remote_root.join(".fixture-controller-stop"), b"stop");
            if let Some(controller) = &mut self.controller {
                let deadline = Instant::now() + Duration::from_secs(8);
                loop {
                    if controller.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        let _ = controller.kill();
                        let _ = controller.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
            let _ = fs::remove_dir_all(&self.local_root);
        }
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn private_ssh_prerequisites_available() -> bool {
        Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
            && ["/usr/sbin/sshd", "/usr/bin/ssh-keygen", "/usr/bin/ssh"]
                .iter()
                .all(|path| Path::new(path).is_file())
    }

    fn push_remote_hook(
        connection: &ManagedConnection,
        nonce: OpenRequestNonce,
        provider_session: Uuid,
        name: &str,
        notification_type: Option<&str>,
        receiver: &std::sync::mpsc::Receiver<Message>,
    ) -> (u64, zmux_remote_runtime::agent_lifecycle::ClaudeHook) {
        let mut input = serde_json::json!({
            "hook_event_name": name,
            "session_id": provider_session,
        });
        if let Some(notification_type) = notification_type {
            input["notification_type"] = notification_type.into();
        }
        let root = nonce.remote_integration_directory();
        let command = format!(
            "printf '%s\\n' {} | ZMUX_AGENT_LIFECYCLE_NONCE={} ZMUX_SHELL_INTEGRATION_DIRECTORY={} python3 {root}/claude-lifecycle-hook.py",
            shell_quote(&input.to_string()),
            nonce.remote_lifecycle_token(),
            root
        );
        let spec = connection
            .operation_spec(&command)
            .expect("build the exact-session remote hook command");
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .expect("run the remote lifecycle hook command");
        assert!(
            output.status.success(),
            "remote hook command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        loop {
            match receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("remote lifecycle frame should arrive")
            {
                Message::Frame {
                    frame:
                        zmux_remote_runtime::agent_lifecycle::Frame::Hook {
                            sequence, event, ..
                        },
                    ..
                } => return (sequence, event),
                Message::Frame {
                    frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { .. },
                    ..
                } => {}
                Message::Frame { frame, .. } => {
                    panic!("unexpected remote lifecycle frame before hook admission: {frame:?}")
                }
                Message::Lost { .. } => panic!("remote lifecycle stream was lost before hook"),
                Message::Connected { .. } => {
                    panic!("unexpected reconnect during hook interleaving")
                }
            }
        }
    }

    fn capture_remote_notice(
        connection: &ManagedConnection,
        nonce: OpenRequestNonce,
        provider_session: Uuid,
        notice: &str,
        hook_name: &str,
    ) -> String {
        let root = nonce.remote_integration_directory();
        let script = r#"import json,os,subprocess,sys; root,nonce,notice,name,session=sys.argv[1:]; fifo=os.path.join(root,"attention"); os.path.exists(fifo) and os.unlink(fifo); os.mkfifo(fifo,0o600); os.chmod(fifo,0o600); reader=os.open(fifo,os.O_RDONLY|os.O_NONBLOCK); environment=os.environ.copy(); environment["ZMUX_AGENT_LIFECYCLE_NONCE"]=nonce; environment["ZMUX_SHELL_INTEGRATION_DIRECTORY"]=root; payload=json.dumps({"hook_event_name":name,"session_id":session,"notification_type":"permission_prompt"}); result=subprocess.run(["python3",os.path.join(root,"claude-lifecycle-notice.py"),notice],input=payload.encode(),env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE); result.returncode and sys.stderr.write(result.stderr.decode()); result.returncode and sys.exit(2); sys.stdout.write(os.read(reader,128).decode())"#;
        let command = format!(
            "python3 -c {} {} {} {} {} {}",
            shell_quote(script),
            shell_quote(&root),
            nonce.remote_lifecycle_token(),
            notice,
            hook_name,
            provider_session
        );
        let spec = connection
            .operation_spec(&command)
            .expect("build the exact-session remote notice command");
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .expect("run the remote notice hook command");
        assert!(
            output.status.success(),
            "remote generic notice command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("remote notice event is UTF-8")
    }

    #[test]
    fn control_sender_waits_for_capacity_and_preserves_call_order() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender.try_send(0).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let thread_sender = sender.clone();
        let thread_cancellation = Arc::clone(&cancellation);
        let thread = thread::spawn(move || {
            send_cancellable(&thread_sender, 1, &thread_cancellation).unwrap();
            send_cancellable(&thread_sender, 2, &thread_cancellation).unwrap();
        });

        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        thread.join().unwrap();
    }

    #[test]
    fn retries_a_real_managed_ssh_bootstrap_race_then_reports_session_loss() {
        if !private_ssh_prerequisites_available() {
            eprintln!("skipping remote lifecycle race test: private SSH prerequisites unavailable");
            return;
        }

        let fixture = SshFixture::start();
        let connection = fixture.connection();
        let nonce = OpenRequestNonce::generate();
        let target = Target {
            identity_namespace: Uuid::new_v4(),
            surface_id: SurfaceId(17),
            surface_uuid: Uuid::new_v4(),
            session_id: SessionId(23),
        };
        let registration = OpenRequestRegistration::new(nonce, OpenRequestSource::ManagedSsh);
        let terminal = connection.terminal_spec(&nonce);
        let mut session = Session::spawn_with_open_requests(
            Launch {
                program: terminal.program,
                args: terminal.args,
                cwd: None,
                size: zmux_session_runtime::default_size(),
            },
            &[],
            &[],
            ShutdownPolicy::Kill,
            None,
            Some(registration),
            Arc::clone(&connection),
        )
        .expect("spawn exact managed SSH terminal");
        let bootstrap_started = fixture.remote_root.join(".bootstrap-started");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !bootstrap_started.exists() {
            let snapshot = session.snapshot().expect("inspect SSH terminal session");
            assert_ne!(
                snapshot.state,
                zmux_session_runtime::State::Failed,
                "managed SSH bootstrap failed before the race was staged: {:?}",
                snapshot.failure
            );
            assert!(
                Instant::now() < deadline,
                "managed SSH terminal did not enter the gated bootstrap"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let spec = connection
            .agent_lifecycle_spec(&nonce)
            .expect("build exact managed lifecycle operation");
        assert_eq!(spec.args[5], "ControlMaster=no");
        assert_eq!(spec.args[11], "ProxyCommand=/usr/bin/false");

        let (sender, receiver) = std::sync::mpsc::sync_channel(16);
        let stream = Stream::start(Arc::clone(&connection), nonce, target, 1, sender)
            .expect("start managed lifecycle supervisor");

        let attempts = fixture.remote_root.join(".stream-attempts");
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let count = fs::read_to_string(&attempts)
                .unwrap_or_default()
                .lines()
                .count();
            if count >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the real managed SSH stream did not retry before bootstrap readiness"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        fs::write(fixture.remote_root.join(".release-bootstrap"), b"go")
            .expect("release the remote bootstrap after pre-Hello retries");

        let connected = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("stream should complete its nonce handshake after bootstrap readiness");
        let binding = match connected {
            Message::Connected { binding, .. } => binding,
            _ => panic!("expected Connected after validated Hello/Active"),
        };
        assert_eq!(binding.target, target);
        assert_eq!(
            binding.nonce,
            nonce.remote_lifecycle_token().parse::<Uuid>().unwrap()
        );

        // Match the remote hook command order: the generic terminal notice runs
        // before lifecycle admission. A live transport lock cannot prove the
        // owner will accept a permission/Stop event without a current prompt.
        let provider_session = Uuid::new_v4();
        let mut lifecycle = crate::agent_lifecycle::Runtime::default();
        let (sequence, start) = push_remote_hook(
            &connection,
            nonce,
            provider_session,
            "SessionStart",
            None,
            &receiver,
        );
        assert_eq!(sequence, 1);
        assert!(
            lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    sequence,
                    start,
                    Instant::now(),
                )
                .unwrap()
        );

        assert_eq!(
            capture_remote_notice(
                &connection,
                nonce,
                provider_session,
                "attention",
                "Notification"
            ),
            "attention\n"
        );
        let (sequence, permission) = push_remote_hook(
            &connection,
            nonce,
            provider_session,
            "Notification",
            Some("permission_prompt"),
            &receiver,
        );
        assert_eq!(sequence, 2);
        assert!(
            !lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    sequence,
                    permission,
                    Instant::now(),
                )
                .unwrap()
        );
        assert_eq!(lifecycle.alert_cursor(), 0);

        assert_eq!(
            capture_remote_notice(&connection, nonce, provider_session, "stop", "Stop"),
            "stop\n"
        );
        let (sequence, stop) = push_remote_hook(
            &connection,
            nonce,
            provider_session,
            "Stop",
            None,
            &receiver,
        );
        assert_eq!(sequence, 3);
        assert!(
            !lifecycle
                .apply_remote_hook(
                    target,
                    binding.runtime_id,
                    binding.generation,
                    sequence,
                    stop,
                    Instant::now(),
                )
                .unwrap()
        );
        assert_eq!(lifecycle.alert_cursor(), 0);

        session
            .shutdown(Duration::from_secs(5))
            .expect("tear down exact SSH terminal session");
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(Message::Lost { binding: lost }) => {
                    assert_eq!(lost, binding);
                    break;
                }
                Ok(Message::Frame { .. }) => {}
                Ok(Message::Connected { .. }) => panic!("unexpected reconnect after session loss"),
                Err(error) => panic!("managed lifecycle did not publish explicit loss: {error}"),
            }
        }

        drop(stream);
        drop(session);
        drop(connection);
        zmux_remote_runtime::connection::request_cleanup_barrier()
            .expect("queue SSH cleanup barrier")
            .recv_timeout(Duration::from_secs(3))
            .expect("SSH cleanup barrier response");
    }
}
