//! Session-scoped Codex and OpenCode lifecycle adapters.
//!
//! These adapters never make provider decisions. Codex is observed through the
//! already-running app-server daemon by the exact opaque thread ID reported by
//! its reviewed SessionStart hook. OpenCode reports sanitized plugin events from
//! its invocation-scoped standalone server.

mod codex;
mod opencode;

use super::*;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};
use zmux_remote_runtime::agent_lifecycle::{
    ProviderKind, ProviderLifecycleObservation, ProviderScope,
};

const PROVIDER_EVENT_LIMIT: usize = 4096;
const PROVIDER_QUEUE_LIMIT: usize = 64;
const PROVIDER_EVENT_SOCKET: &str = "ZMUX_PROVIDER_EVENT_SOCKET";
const LOCAL_RUNTIME_NAME: &str = "ZMUX_PROVIDER_RUNTIME";
const LOCAL_GENERATION_NAME: &str = "ZMUX_PROVIDER_GENERATION";

pub(crate) enum Invocation {
    Auto {
        provider: ProviderKind,
        root: PathBuf,
        surface: Uuid,
        args: Vec<OsString>,
    },
    CodexSessionStart,
    OpenCodeEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderName {
    Codex,
    OpenCode,
}

impl ProviderName {
    fn executable(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

enum AdapterEvent {
    CodexSession { session_id: String },
    OpenCodeObservation(ProviderLifecycleObservation),
    EvidenceLost,
}

pub(super) fn parse(tail: &[OsString]) -> Result<Invocation, CliError> {
    let separator = tail.iter().position(|argument| argument == "--");
    let prefix = &tail[..separator.unwrap_or(tail.len())];
    let args = utf8_args(prefix)?;
    if separator.is_none()
        && args
            .get(1)
            .is_some_and(|event| event == "codex-session-start")
        && args.len() == 2
    {
        return Ok(Invocation::CodexSessionStart);
    }
    if separator.is_none()
        && args.get(1).is_some_and(|event| event == "opencode-event")
        && args.len() == 2
    {
        return Ok(Invocation::OpenCodeEvent);
    }
    if let Some(separator) = separator
        && args.len() == 7
        && args.get(1).is_some_and(|verb| verb == "auto")
        && args.get(3).is_some_and(|option| option == "--root")
        && args.get(5).is_some_and(|option| option == "--surface")
    {
        let provider = match args[2].as_str() {
            "codex" => ProviderKind::Codex,
            "opencode" => ProviderKind::OpenCode,
            _ => return Err(CliError("provider auto supports codex or opencode".into())),
        };
        let surface = args[6]
            .parse::<Uuid>()
            .map_err(|_| CliError("provider surface must be a canonical UUID".into()))?;
        if surface.is_nil() {
            return Err(CliError("provider surface cannot be nil".into()));
        }
        let provider_args = tail[separator + 1..].to_vec();
        return Ok(Invocation::Auto {
            provider,
            root: PathBuf::from(&args[4]),
            surface,
            args: provider_args,
        });
    }
    Err(CliError(
        "hooks provider requires auto codex|opencode, codex-session-start, or opencode-event"
            .into(),
    ))
}

pub(super) fn run(
    invocation: Invocation,
    socket: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
) -> Result<u8, Box<dyn Error>> {
    match invocation {
        Invocation::CodexSessionStart => {
            codex_session_start()?;
            Ok(0)
        }
        Invocation::OpenCodeEvent => {
            opencode_event()?;
            Ok(0)
        }
        Invocation::Auto {
            provider,
            root,
            surface,
            args,
        } => automatic_launch(provider, socket, timeout, password, &root, surface, args),
    }
}

fn codex_session_start() -> Result<(), CliError> {
    let payload = bounded_input(PROVIDER_EVENT_LIMIT * 2)?;
    let payload: Value = serde_json::from_slice(&payload)
        .map_err(|_| CliError("invalid Codex SessionStart hook input".into()))?;
    if payload["hook_event_name"] != "SessionStart" {
        return Err(CliError("provider hook event was not SessionStart".into()));
    }
    let session_id = provider_id(&payload["session_id"])?;
    send_local_event(&json!({"kind":"codex_session","session_id":session_id}))
}

fn opencode_event() -> Result<(), CliError> {
    let payload = bounded_input(PROVIDER_EVENT_LIMIT)?;
    let value: Value = serde_json::from_slice(&payload)
        .map_err(|_| CliError("invalid sanitized OpenCode event".into()))?;
    let object = value
        .as_object()
        .ok_or_else(|| CliError("invalid sanitized OpenCode event shape".into()))?;
    if object.len() == 1 && value["kind"] == "provider_loss" {
        return send_local_event(&json!({"kind":"provider_loss"}));
    }
    if object.len() != 2
        || !object.contains_key("observation")
        || value["kind"] != "opencode_observation"
    {
        return Err(CliError("invalid sanitized OpenCode event shape".into()));
    }
    let observation: ProviderLifecycleObservation =
        serde_json::from_value(value["observation"].clone())
            .map_err(|_| CliError("invalid normalized OpenCode observation".into()))?;
    if observation.provider != ProviderKind::OpenCode || observation.validate().is_err() {
        return Err(CliError("OpenCode observation was not admissible".into()));
    }
    send_local_event(&json!({"kind":"opencode_observation","observation":observation}))
}

fn bounded_input(limit: usize) -> Result<Vec<u8>, CliError> {
    let mut input = std::io::stdin().take((limit + 1) as u64);
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(CliError("oversized provider hook input".into()));
    }
    Ok(bytes)
}

fn provider_id(value: &Value) -> Result<&str, CliError> {
    let value = value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| CliError("provider supplied an invalid opaque session ID".into()))?;
    Ok(value)
}

fn send_local_event(value: &Value) -> Result<(), CliError> {
    let path = env::var_os(PROVIDER_EVENT_SOCKET)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| CliError("provider event route is unavailable".into()))?;
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.connect_timeout(&socket2::SockAddr::unix(path)?, Duration::from_millis(100))?;
    socket.set_nonblocking(false)?;
    let mut stream: UnixStream = socket.into();
    stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    let bytes = serde_json::to_vec(value).map_err(|_| CliError("invalid provider event".into()))?;
    if bytes.len() + 1 > PROVIDER_EVENT_LIMIT {
        return Err(CliError("provider event exceeded its byte bound".into()));
    }
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    Ok(())
}

fn automatic_launch(
    provider: ProviderKind,
    socket: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
    root: &Path,
    surface: Uuid,
    args: Vec<OsString>,
) -> Result<u8, Box<dyn Error>> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return launch_untracked(
            provider,
            args,
            "this invocation has no interactive terminal",
        );
    }
    let options = match client_options(socket, timeout.min(EVENT_BUDGET), password) {
        Ok(options) => options,
        Err(_) => {
            return launch_untracked(provider, args, "Desktop lifecycle routing is unavailable");
        }
    };
    let route = match route(&options, surface, Instant::now() + options.timeout).and_then(|route| {
        check_terminal_ancestor(&route)?;
        Ok(route)
    }) {
        Ok(route) => route,
        Err(_) => {
            return launch_untracked(
                provider,
                args,
                "the exact Desktop terminal could not be verified",
            );
        }
    };
    let root_dir = match plugin::Directory::open(root, false) {
        Ok(root_dir) => root_dir,
        Err(_) => {
            return launch_untracked(
                provider,
                args,
                "the private session adapter directory is unavailable",
            );
        }
    };
    let _root_dir = root_dir;
    let provider_name = match provider {
        ProviderKind::Codex => ProviderName::Codex,
        ProviderKind::OpenCode => ProviderName::OpenCode,
    };
    if provider_name == ProviderName::Codex && !codex::interactive(&args) {
        return launch_untracked(
            provider,
            args,
            "this Codex command is not an interactive TUI session",
        );
    }
    let executable = find_executable(provider_name.executable())?;

    let runtime = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let runtime_dir = match create_invocation_directory(root, provider_name) {
        Ok(runtime_dir) => runtime_dir,
        Err(_) => return launch_untracked(provider, args, "private provider adapter setup failed"),
    };
    let socket_path = runtime_dir.join("provider-events.sock");
    let EventListener {
        worker: listener_thread,
        receiver,
        stop: stop_listener,
        lost: listener_lost,
    } = match event_listener(&socket_path) {
        Ok(listener) => listener,
        Err(_) => {
            let _ = fs::remove_dir_all(&runtime_dir);
            return launch_untracked(
                provider,
                args,
                "private provider event route could not be created",
            );
        }
    };
    let mut command = Command::new(&executable);
    command
        .env(PROVIDER_EVENT_SOCKET, &socket_path)
        .env(LOCAL_RUNTIME_NAME, runtime.to_string())
        .env(LOCAL_GENERATION_NAME, generation.to_string());
    let launch_args = match provider_name {
        ProviderName::Codex => match codex::launch_args(&executable, &args) {
            Ok(launch_args) => launch_args,
            Err(error) => {
                stop_listener.store(true, Ordering::Release);
                let _ = listener_thread.join();
                let _ = fs::remove_dir_all(&runtime_dir);
                return launch_untracked(provider, args, &error.0);
            }
        },
        ProviderName::OpenCode => {
            match opencode::prepare_launch(&executable, &args, &runtime_dir) {
                Ok(launch) => {
                    command.env("OPENCODE_CONFIG_DIR", launch.config_dir);
                    if let Some(session_id) = launch.expected_session {
                        command.env("ZMUX_OPENCODE_EXPECTED_SESSION_ID", session_id);
                    }
                    launch.args
                }
                Err(_) => {
                    stop_listener.store(true, Ordering::Release);
                    let _ = listener_thread.join();
                    let _ = fs::remove_dir_all(&runtime_dir);
                    return launch_untracked(
                        provider,
                        args,
                        "OpenCode session plugin setup failed",
                    );
                }
            }
        }
    };
    command.args(launch_args);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // The provider spawn is attempted once. If it returns a child, every later
    // observer error is reported as unknown evidence and never starts a second
    // provider instance.
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            stop_listener.store(true, Ordering::Release);
            let _ = listener_thread.join();
            let _ = fs::remove_dir_all(&runtime_dir);
            return Err(error.into());
        }
    };

    let result = supervise_provider(ProviderSupervision {
        provider: provider_name,
        executable: &executable,
        options: &options,
        route: &route,
        runtime,
        generation,
        child: &mut child,
        events: receiver,
        listener_stop: Arc::clone(&stop_listener),
        listener_lost,
    });
    stop_listener.store(true, Ordering::Release);
    let _ = listener_thread.join();
    match fs::remove_dir_all(&runtime_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => eprintln!("zmux: provider adapter files retained for review"),
    }
    result
}

fn launch_untracked(
    provider: ProviderKind,
    args: Vec<OsString>,
    reason: &str,
) -> Result<u8, Box<dyn Error>> {
    eprintln!(
        "zmux: automatic {} tracking unavailable ({reason}); launching once without lifecycle adapter",
        provider.wire_name()
    );
    let executable = find_executable(match provider {
        ProviderKind::Codex => "codex",
        ProviderKind::OpenCode => "opencode",
    })?;
    let status = Command::new(executable)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(exit_code(status))
}

fn find_executable(name: &str) -> Result<PathBuf, CliError> {
    let search_path = env::var_os("PATH").ok_or_else(|| CliError("PATH is unavailable".into()))?;
    for directory in env::split_paths(&search_path) {
        let candidate = directory.join(name);
        if fs::metadata(&candidate)
            .is_ok_and(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0)
        {
            return candidate.canonicalize().map_err(Into::into);
        }
    }
    Err(CliError(format!("{name} executable was not found on PATH")))
}

fn create_invocation_directory(root: &Path, provider: ProviderName) -> Result<PathBuf, CliError> {
    for _ in 0..8 {
        let path = root.join(format!(
            "{}-auto-{}",
            provider.executable(),
            Uuid::new_v4().simple()
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(CliError(
        "cannot allocate a private provider adapter directory".into(),
    ))
}

struct EventListener {
    worker: thread::JoinHandle<()>,
    receiver: Receiver<AdapterEvent>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
}

fn event_listener(path: &Path) -> Result<EventListener, CliError> {
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let (sender, receiver) = mpsc::sync_channel(PROVIDER_QUEUE_LIMIT);
    let stop = Arc::new(AtomicBool::new(false));
    let lost = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_lost = Arc::clone(&lost);
    let worker = thread::Builder::new()
        .name("zmux-provider-events".into())
        .spawn(move || accept_events(listener, sender, thread_stop, thread_lost))?;
    Ok(EventListener {
        worker,
        receiver,
        stop,
        lost,
    })
}

fn accept_events(
    listener: UnixListener,
    sender: SyncSender<AdapterEvent>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if stream.set_nonblocking(false).is_err() {
                    lost.store(true, Ordering::Release);
                    stop.store(true, Ordering::Release);
                    break;
                }
                let Some(value) = read_event(&mut stream) else {
                    lost.store(true, Ordering::Release);
                    stop.store(true, Ordering::Release);
                    break;
                };
                let event = match value["kind"].as_str() {
                    Some("codex_session") => provider_id(&value["session_id"]).map(|session_id| {
                        AdapterEvent::CodexSession {
                            session_id: session_id.to_owned(),
                        }
                    }),
                    Some("opencode_observation") => {
                        serde_json::from_value::<ProviderLifecycleObservation>(
                            value["observation"].clone(),
                        )
                        .map_err(|_| CliError("invalid OpenCode observation".into()))
                        .and_then(|observation| {
                            if observation.provider == ProviderKind::OpenCode
                                && observation.scope == ProviderScope::Root
                                && observation.validate().is_ok()
                            {
                                Ok(AdapterEvent::OpenCodeObservation(observation))
                            } else {
                                Err(CliError("OpenCode observation rejected".into()))
                            }
                        })
                    }
                    Some("provider_loss") => Ok(AdapterEvent::EvidenceLost),
                    _ => Err(CliError("unknown provider event".into())),
                };
                match event {
                    Ok(event) => {
                        if sender.try_send(event).is_err() {
                            lost.store(true, Ordering::Release);
                            stop.store(true, Ordering::Release);
                            break;
                        }
                    }
                    Err(_) => {
                        lost.store(true, Ordering::Release);
                        stop.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                lost.store(true, Ordering::Release);
                stop.store(true, Ordering::Release);
                break;
            }
        }
    }
}

fn read_event(stream: &mut UnixStream) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_millis(200);
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        stream.set_read_timeout(Some(remaining)).ok()?;
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => {
                bytes.push(byte[0]);
                if bytes.len() >= PROVIDER_EVENT_LIMIT {
                    return None;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    serde_json::from_slice(&bytes).ok()
}

struct ProviderSupervision<'a> {
    provider: ProviderName,
    executable: &'a Path,
    options: &'a ClientOptions,
    route: &'a Route,
    runtime: Uuid,
    generation: Uuid,
    child: &'a mut Child,
    events: Receiver<AdapterEvent>,
    listener_stop: Arc<AtomicBool>,
    listener_lost: Arc<AtomicBool>,
}

fn supervise_provider(context: ProviderSupervision<'_>) -> Result<u8, Box<dyn Error>> {
    let ProviderSupervision {
        provider,
        executable,
        options,
        route,
        runtime,
        generation,
        child,
        events,
        listener_stop,
        listener_lost,
    } = context;
    let mut open_code = opencode::ObservationOwner::new(options, route, runtime, generation);
    let mut codex_watcher: Option<codex::Watcher> = None;
    let mut codex_session_id: Option<String> = None;
    let mut codex_setup_warning_at = Instant::now() + Duration::from_secs(4);
    let mut opencode_setup_warning_at = Instant::now() + Duration::from_secs(8);
    let mut observer_disabled = false;
    loop {
        if let Ok(status) = child.try_wait()
            && let Some(status) = status
        {
            if let Some(watcher) = &mut codex_watcher
                && watcher.session_ended(options, route).is_err()
            {
                eprintln!("zmux: Codex session exit could not be confirmed by Desktop");
            }
            if open_code.session_ended_all(options).is_err() {
                eprintln!("zmux: OpenCode session exit could not be confirmed by Desktop");
            }
            return Ok(exit_code(status));
        }
        if listener_lost.swap(false, Ordering::AcqRel) && !observer_disabled {
            observer_disabled = true;
            listener_stop.store(true, Ordering::Release);
            if let Some(watcher) = &mut codex_watcher
                && watcher.evidence_lost(options, route).is_err()
            {
                eprintln!("zmux: Codex evidence loss could not be recorded");
            }
            if open_code.evidence_lost(options).is_err() {
                eprintln!("zmux: OpenCode evidence loss could not be recorded");
            }
            eprintln!("zmux: provider event route was lost; activity remains Unknown");
        }
        let received = if observer_disabled {
            Err(mpsc::RecvTimeoutError::Timeout)
        } else {
            events.recv_timeout(Duration::from_millis(100))
        };
        match received {
            Ok(AdapterEvent::CodexSession { session_id }) if provider == ProviderName::Codex => {
                if codex_session_id
                    .as_deref()
                    .is_none_or(|current| current == session_id)
                    && codex_watcher.is_none()
                {
                    codex_session_id = Some(session_id.clone());
                    match codex::Watcher::attach(
                        executable, options, route, runtime, generation, session_id,
                    ) {
                        Ok(watcher) => codex_watcher = Some(watcher),
                        Err(_) => eprintln!(
                            "zmux: Codex thread could not be attached to the existing app-server; activity remains Unknown"
                        ),
                    }
                }
            }
            Ok(AdapterEvent::OpenCodeObservation(observation))
                if provider == ProviderName::OpenCode =>
            {
                if open_code.observe(observation, options).is_err() {
                    eprintln!(
                        "zmux: OpenCode lifecycle evidence is unavailable; activity remains Unknown"
                    );
                    observer_disabled = true;
                    listener_stop.store(true, Ordering::Release);
                }
            }
            Ok(AdapterEvent::EvidenceLost) => {
                if let Some(watcher) = &mut codex_watcher
                    && watcher.evidence_lost(options, route).is_err()
                {
                    eprintln!("zmux: Codex evidence loss could not be recorded");
                }
                if open_code.evidence_lost(options).is_err() {
                    eprintln!("zmux: OpenCode evidence loss could not be recorded");
                }
                observer_disabled = true;
                listener_stop.store(true, Ordering::Release);
            }
            Ok(_) => {
                observer_disabled = true;
                listener_stop.store(true, Ordering::Release);
                if provider == ProviderName::Codex
                    && let Some(watcher) = &mut codex_watcher
                {
                    let _ = watcher.evidence_lost(options, route);
                }
                if provider == ProviderName::OpenCode {
                    let _ = open_code.evidence_lost(options);
                }
                eprintln!(
                    "zmux: provider event did not match this invocation; activity remains Unknown"
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if !observer_disabled
            && provider == ProviderName::Codex
            && let Some(watcher) = &mut codex_watcher
            && watcher.poll_if_due(options, route).is_err()
        {
            if watcher.evidence_lost(options, route).is_err() {
                eprintln!("zmux: Codex evidence loss could not be recorded");
            }
            observer_disabled = true;
            listener_stop.store(true, Ordering::Release);
            eprintln!("zmux: Codex app-server evidence was lost; activity remains Unknown");
        }
        if provider == ProviderName::Codex
            && codex_session_id.is_none()
            && Instant::now() >= codex_setup_warning_at
        {
            eprintln!(
                "zmux: Codex SessionStart tracking is not active; review the Zmux hook in /hooks. No lifecycle state is being inferred."
            );
            codex_setup_warning_at = Instant::now() + Duration::from_secs(60);
        }
        if !observer_disabled
            && provider == ProviderName::OpenCode
            && !open_code.has_sessions()
            && Instant::now() >= opencode_setup_warning_at
        {
            eprintln!(
                "zmux: OpenCode root-session tracking has not started; review Zmux plugin loading and trust in OpenCode's startup output. Activity remains Unknown."
            );
            opencode_setup_warning_at = Instant::now() + Duration::from_secs(60);
        }
    }
}

fn target(route: &Route) -> LifecycleTarget {
    LifecycleTarget {
        identity_namespace: route.namespace,
        surface_id: route.surface_id,
        surface_uuid: route.surface,
        session_id: SessionId(route.session),
    }
}

fn submit_observation(
    options: &ClientOptions,
    route: &Route,
    runtime: Uuid,
    generation: Uuid,
    sequence: u64,
    observation: ProviderLifecycleObservation,
) -> Result<(), CliError> {
    observation
        .validate()
        .map_err(|_| CliError("provider observation failed contract validation".into()))?;
    let response = lifecycle_operation(
        options,
        Instant::now() + options.timeout.min(LIFECYCLE_BUDGET),
        LifecycleRequest {
            request_id: Uuid::new_v4(),
            action: LifecycleAction::ProviderObservation {
                target: target(route),
                runtime_id: runtime,
                generation,
                sequence,
                observation,
            },
        },
    )?;
    if response["accepted"] != true {
        return Err(CliError(
            "desktop did not accept provider observation".into(),
        ));
    }
    Ok(())
}

pub(super) struct ProviderSessionEnd {
    provider: ProviderKind,
    session_id: String,
    runtime: Uuid,
    generation: Uuid,
    sequence: u64,
    lost: bool,
}

fn submit_session_end(
    options: &ClientOptions,
    route: &Route,
    session_end: ProviderSessionEnd,
) -> Result<(), CliError> {
    let ProviderSessionEnd {
        provider,
        session_id,
        runtime,
        generation,
        sequence,
        lost,
    } = session_end;
    let action = if lost {
        LifecycleAction::ProviderEvidenceLost {
            target: target(route),
            provider,
            session_id,
            runtime_id: runtime,
            generation,
            sequence,
        }
    } else {
        LifecycleAction::ProviderSessionEnded {
            target: target(route),
            provider,
            session_id,
            runtime_id: runtime,
            generation,
            sequence,
        }
    };
    let response = lifecycle_operation(
        options,
        Instant::now() + options.timeout.min(LIFECYCLE_BUDGET),
        LifecycleRequest {
            request_id: Uuid::new_v4(),
            action,
        },
    )?;
    if response["accepted"] != true {
        return Err(CliError(
            "desktop did not accept provider session end".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trickled_event_has_an_absolute_deadline_and_bounded_worker_shutdown() {
        let directory = Path::new("/tmp")
            .canonicalize()
            .unwrap()
            .join(format!("zpev-{}", Uuid::new_v4().simple()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let path = directory.join("events");
        let listener = event_listener(&path).unwrap();
        let mut peer = UnixStream::connect(&path).unwrap();
        let shutdown = peer.try_clone().unwrap();
        let producer = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if peer.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
        });
        let result = listener.receiver.recv_timeout(Duration::from_millis(600));
        listener.stop.store(true, Ordering::Release);
        let _ = shutdown.shutdown(std::net::Shutdown::Both);
        listener.worker.join().unwrap();
        producer.join().unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
        assert!(
            matches!(result, Err(mpsc::RecvTimeoutError::Disconnected)),
            "trickled bytes must not extend the frame deadline"
        );
        assert!(listener.lost.load(Ordering::Acquire));
    }
}
