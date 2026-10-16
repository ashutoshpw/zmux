use super::{
    Authorization, AuthorizationReceiver, ClientAdmission, ClientLaunchFailure, ClientReady,
    EventSender, LaunchEvent, LaunchOperation, LaunchRequest, Observation, ObserverHandle,
    namespace::{Namespace, View},
};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zmux_remote_runtime::local_zellij::{
    Record, completion,
    observer_protocol::{self as wire, Acknowledgment, Event, Frame, OwnerOutcome},
};
use zmux_session_runtime::{
    Launch, SessionError, ShutdownPolicy,
    child::{ProcessIdentity, process_identity},
};

const POLL: Duration = Duration::from_millis(2);
const CLOSE_BUDGET: Duration = Duration::from_secs(3);
fn fail(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn diagnostic(state: &Arc<Mutex<Observation>>, error: impl ToString) {
    match state.lock() {
        Ok(mut state) => {
            if state.error.is_none() {
                state.error = Some(error.to_string().chars().take(512).collect());
            }
        }
        Err(_) => eprintln!("local Zellij observer diagnostic state poisoned"),
    }
}
fn startup(request: &LaunchRequest, stopped: &AtomicBool) -> io::Result<()> {
    if stopped.load(Ordering::Acquire) || request.cancellation.load(Ordering::Acquire) {
        return Err(fail(
            "foreground Zellij launch cancelled; creation must not be replayed",
        ));
    }
    if Instant::now() >= request.startup_deadline {
        return Err(fail(
            "foreground Zellij startup deadline; creation outcome is unknown, do not replay",
        ));
    }
    Ok(())
}
fn executable(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(fail(
            "local Zellij executable must be explicit and absolute",
        ));
    }
    let canonical = path.canonicalize()?;
    if canonical.to_str().is_none() {
        return Err(fail("local Zellij executable path must be UTF-8"));
    }
    let info = canonical.metadata()?;
    if !info.is_file() || info.permissions().mode() & 0o111 == 0 {
        return Err(fail(
            "local Zellij executable is not an executable regular file",
        ));
    }
    Ok(canonical)
}
struct Validated {
    cli: PathBuf,
    zellij: PathBuf,
    profile: PathBuf,
}
fn validate(request: &LaunchRequest) -> io::Result<Validated> {
    request.correlation.validate()?;
    if request.installation.epoch != request.correlation.installation_epoch
        || request.profile.id != request.correlation.profile_id
        || request.profile.id.is_nil()
        || !request.profile.root.is_absolute()
        || request.profile.root.to_str().is_none()
    {
        return Err(fail(
            "local Zellij installation/profile correlation mismatch",
        ));
    }
    let cli = executable(&request.installation.cli)?;
    let zellij = executable(&request.installation.zellij)?;
    match &request.operation {
        LaunchOperation::Start { name, cwd, command } => {
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !cwd.is_absolute()
                || cwd.to_str().is_none()
                || !cwd.is_dir()
                || command
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty() || s.len() > 8192 || s.contains('\0'))
            {
                return Err(fail("invalid explicit local Zellij Start name/cwd/command"));
            }
        }
        LaunchOperation::Attach { identity } => {
            identity.validate().map_err(|e| fail(e.to_string()))?;
            if identity.profile_id != request.profile.id {
                return Err(fail("Attach owner belongs to another profile"));
            }
        }
    }
    match fs::DirBuilder::new()
        .mode(0o700)
        .create(&request.profile.root)
    {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let info = request.profile.root.symlink_metadata()?;
    if !info.is_dir() || info.uid() != nix::unistd::geteuid().as_raw() || info.mode() & 0o077 != 0 {
        return Err(fail(
            "local Zellij profile must be a private owned directory",
        ));
    }
    let profile = request.profile.root.canonicalize()?;
    if profile.to_str().is_none() {
        return Err(fail("canonical local Zellij profile path must be UTF-8"));
    }
    Ok(Validated {
        cli,
        zellij,
        profile,
    })
}
// The Session worker drops this only after its PTY, retained CLI child and
// physical cleanup have ended. No UI/core thread waits for this join.
struct Resource {
    stopped: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    namespace: Namespace,
    state: Arc<Mutex<Observation>>,
}
impl Drop for Resource {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if self
            .monitor
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            diagnostic(
                &self.state,
                "local Zellij observer monitor panicked; outcome unknown",
            );
        }
        let cleaned = match self.namespace.cleanup() {
            Ok(()) => true,
            Err(e) => {
                diagnostic(&self.state, format!("observer namespace cleanup: {e}"));
                false
            }
        };
        match self.state.lock() {
            Ok(mut state) => {
                state.monitor_joined = true;
                state.namespace_cleaned = cleaned;
            }
            Err(_) => eprintln!("local Zellij observer join publication poisoned"),
        }
    }
}
// A PTY spawn may return before exec: portable-pty closes the standard
// library's spawn error pipe in pre_exec. Keep the physical child identity
// separate from the configured executable admission decision.
fn configured_cli_identity(
    identity: ProcessIdentity,
    launcher: &ProcessIdentity,
    configured: &Path,
    deadline: Instant,
    cancellation: &AtomicBool,
) -> io::Result<ProcessIdentity> {
    let anchor = identity.clone();
    let mut current = identity;
    loop {
        if cancellation.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(fail(format!(
                "retained CLI exec admission cancelled or deadline; do not replay: observed={current:?}; expected_executable={}",
                configured.display()
            )));
        }
        if current.pid != anchor.pid
            || current.uid != anchor.uid
            || current.parent_pid != anchor.parent_pid
            || current.start != anchor.start
            || current.boot != anchor.boot
            || current.parent_pid != launcher.pid
            || current.uid != launcher.uid
            || current.boot != launcher.boot
        {
            return Err(fail(format!(
                "retained CLI physical child identity changed or is not the direct launcher child: observed={current:?}; anchor={anchor:?}"
            )));
        }
        if current.executable == configured {
            return Ok(current);
        }
        // Only the exact native launcher's inherited image can precede exec.
        // Any unrelated image is rejected, even with unchanged PID/start fields.
        if current.executable != launcher.executable {
            return Err(fail(format!(
                "retained CLI entered an unconfigured executable: observed={current:?}; expected_executable={}",
                configured.display()
            )));
        }
        thread::sleep(POLL);
        current = process_identity(anchor.pid).map_err(|error| {
            fail(format!(
                "inspect retained CLI exec transition: {error}; retained_child={anchor:?}"
            ))
        })?;
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;

struct Ready {
    admission: ClientAdmission,
    record: Record,
    frontend: Option<ProcessIdentity>,
    execution_contract_version: u8,
}

pub(crate) fn launch_client(
    request: LaunchRequest,
    events: EventSender,
    authorization: AuthorizationReceiver,
) -> Result<ClientReady, ClientLaunchFailure> {
    let before = || -> io::Result<(Validated, Namespace, UnixListener)> {
        startup(&request, &AtomicBool::new(false))?;
        let validated = validate(&request)?;
        startup(&request, &AtomicBool::new(false))?;
        let (namespace, listener) = Namespace::create(
            request.correlation.clone(),
            process_identity(std::process::id())?,
            request.startup_deadline,
        )?;
        Ok((validated, namespace, listener))
    };
    let (validated, namespace, listener) = before().map_err(|e| ClientLaunchFailure {
        error: SessionError::Failure(e.to_string()),
        cleanup: None,
        observer: None,
    })?;
    let state = Arc::new(Mutex::new(Observation::default()));
    let observer = ObserverHandle {
        state: state.clone(),
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let (identity_tx, identity_rx) = mpsc::sync_channel(1);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let mut environment = request.environment.clone();
    environment.extend([
        (
            wire::ENV.into(),
            namespace.descriptor().to_string_lossy().into_owned(),
        ),
        (
            "ZMUX_LOCAL_ZELLIJ_ROOT".into(),
            validated.profile.to_string_lossy().into_owned(),
        ),
        (
            "ZMUX_LOCAL_ZELLIJ_BIN".into(),
            validated.zellij.to_string_lossy().into_owned(),
        ),
    ]);
    let mut args = vec!["local-zellij".into()];
    match &request.operation {
        LaunchOperation::Start { name, cwd, command } => {
            args.extend([
                "start".into(),
                name.clone(),
                "--headless".into(),
                "--cwd".into(),
                cwd.to_string_lossy().into_owned(),
            ]);
            if let Some(command) = command {
                args.extend(["--command".into(), command.clone()]);
            }
        }
        LaunchOperation::Attach { identity } => args.extend([
            "attach".into(),
            "--headless".into(),
            "--id".into(),
            identity.owner_uuid.to_string(),
        ]),
    }
    let removals: Vec<OsString> = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| {
            key.to_str()
                .is_some_and(|key| key.starts_with("ZMUX_") || key == "TMUX" || key == "TMUX_PANE")
        })
        .collect();
    let launch = Launch {
        program: validated.cli.to_string_lossy().into_owned(),
        args,
        cwd: None,
        size: zmux_session_runtime::default_size(),
    };
    let launcher = process_identity(std::process::id()).map_err(|e| ClientLaunchFailure {
        error: SessionError::Failure(format!("inspect native CLI launcher identity: {e}")),
        cleanup: None,
        observer: Some(observer.clone()),
    })?;
    let configured = validated.cli.clone();
    let engine = request.engine;
    let deadline = request.startup_deadline;
    let cancellation = request.cancellation.clone();
    let view = namespace.view.clone();
    let nonce = namespace.nonce.clone();
    let thread_state = state.clone();
    let thread_stop = stopped.clone();
    let monitor = thread::Builder::new()
        .name("zmux-zellij-observer".into())
        .spawn(move || {
            let outcome = Monitor {
                request,
                validated,
                listener,
                view,
                nonce,
                stopped: thread_stop,
                identities: identity_rx,
                events,
                authorization,
                ready: ready_tx.clone(),
                state: thread_state.clone(),
            }
            .run();
            if let Err(error) = outcome {
                diagnostic(&thread_state, &error);
                let _ = ready_tx.try_send(Err(error.to_string()));
            }
        })
        .map_err(|e| ClientLaunchFailure {
            error: SessionError::Failure(format!("start observer monitor: {e}")),
            cleanup: None,
            observer: Some(observer.clone()),
        })?;
    let resource = Resource {
        stopped,
        monitor: Some(monitor),
        namespace,
        state,
    };
    let session = crate::terminal_engine::spawn_with_policy(
        launch,
        &environment,
        &removals,
        engine,
        ShutdownPolicy::Graceful {
            timeout: CLOSE_BUDGET,
        },
        resource,
    )
    .map_err(|error| ClientLaunchFailure {
        error,
        cleanup: None,
        observer: Some(observer.clone()),
    })?;
    let admitted = (|| -> Result<Ready, SessionError> {
        let identity = process_identity(session.process_id())
            .and_then(|identity| {
                configured_cli_identity(identity, &launcher, &configured, deadline, &cancellation)
            })
            .map_err(|e| SessionError::Failure(format!("inspect retained CLI identity: {e}")))?;
        identity_tx.try_send(identity).map_err(|_| {
            SessionError::Failure("observer rejected retained CLI identity handoff".into())
        })?;
        loop {
            if cancellation.load(Ordering::Acquire) {
                return Err(SessionError::Failure(
                    "foreground Zellij startup cancelled or expired; do not replay".into(),
                ));
            }
            match ready_rx.try_recv() {
                Ok(Ok(ready)) => {
                    if matches!(ready.admission, ClientAdmission::InputReady { .. })
                        && Instant::now() >= deadline
                    {
                        return Err(SessionError::Failure(
                            "frontend input readiness arrived after startup deadline".into(),
                        ));
                    }
                    return Ok(ready);
                }
                Ok(Err(e)) => return Err(SessionError::Failure(e)),
                Err(TryRecvError::Disconnected) => {
                    return Err(SessionError::Failure(
                        "observer ended before transport readiness".into(),
                    ));
                }
                Err(TryRecvError::Empty) => (),
            }
            if Instant::now() >= deadline {
                return Err(SessionError::Failure(
                    "frontend input readiness startup deadline; input unavailable".into(),
                ));
            }
            if session.snapshot()?.state != zmux_session_runtime::State::Running {
                return Err(SessionError::Failure(
                    "retained CLI exited before observer readiness".into(),
                ));
            }
            thread::sleep(POLL);
        }
    })();
    match admitted {
        Ok(ready) => Ok(ClientReady {
            session,
            observer,
            admission: ready.admission,
            record: ready.record,
            frontend: ready.frontend,
            execution_contract_version: ready.execution_contract_version,
        }),
        Err(error) => {
            if let Err(e) = session.request_close() {
                diagnostic(&observer.state, format!("request owned CLI cleanup: {e}"));
            }
            Err(ClientLaunchFailure {
                error,
                cleanup: Some(session),
                observer: Some(observer),
            })
        }
    }
}

fn transfer(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    writing: bool,
    request: &LaunchRequest,
    stopped: &AtomicBool,
    starting: bool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        // Drain available final bytes even if Session cleanup has just ended.
        let result = if writing {
            stream.write(&bytes[offset..])
        } else {
            stream.read(&mut bytes[offset..])
        };
        match result {
            Ok(0) => return Err(fail("observer EOF or zero write; final outcome unknown")),
            Ok(n) => offset += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if starting {
                    startup(request, stopped)?;
                } else if stopped.load(Ordering::Acquire) {
                    return Err(fail(
                        "CLI physically ended without a complete final observer frame; outcome unknown",
                    ));
                }
                thread::sleep(POLL);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn receive(
    stream: &mut UnixStream,
    request: &LaunchRequest,
    stopped: &AtomicBool,
    starting: bool,
) -> io::Result<Frame> {
    if starting {
        startup(request, stopped)?;
    }
    let mut size = [0; 4];
    transfer(stream, &mut size, false, request, stopped, starting)?;
    let size = u32::from_le_bytes(size) as usize;
    if !(1..=wire::FRAME_LIMIT).contains(&size) {
        return Err(fail("observer lifecycle frame exceeds bound"));
    }
    let mut bytes = vec![0; size];
    transfer(stream, &mut bytes, false, request, stopped, starting)?;
    Frame::decode(&bytes)
}
fn send_ack(
    stream: &mut UnixStream,
    ack: Acknowledgment,
    request: &LaunchRequest,
    stopped: &AtomicBool,
) -> io::Result<()> {
    startup(request, stopped)?;
    let bytes = serde_json::to_vec(&ack).map_err(io::Error::other)?;
    if bytes.len() > wire::FRAME_LIMIT {
        return Err(fail("observer acknowledgment exceeds bound"));
    }
    let mut frame = Vec::with_capacity(bytes.len() + 4);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&bytes);
    transfer(stream, &mut frame, true, request, stopped, true)
}
struct Monitor {
    request: LaunchRequest,
    validated: Validated,
    listener: UnixListener,
    view: View,
    nonce: String,
    stopped: Arc<AtomicBool>,
    identities: Receiver<ProcessIdentity>,
    events: EventSender,
    authorization: AuthorizationReceiver,
    ready: SyncSender<Result<Ready, String>>,
    state: Arc<Mutex<Observation>>,
}
impl Monitor {
    fn run(self) -> io::Result<()> {
        let Self {
            request,
            validated,
            listener,
            view,
            nonce,
            stopped,
            identities,
            events,
            authorization,
            ready,
            state,
        } = self;
        let stopped = &stopped;
        let cli = loop {
            startup(&request, stopped)?;
            match identities.try_recv() {
                Ok(identity) => break identity,
                Err(TryRecvError::Disconnected) => {
                    return Err(fail("retained CLI identity was not established"));
                }
                Err(TryRecvError::Empty) => thread::sleep(POLL),
            }
        };
        if cli.parent_pid != std::process::id()
            || cli.executable != validated.cli
            || cli.uid != nix::unistd::geteuid().as_raw()
        {
            return Err(fail(format!(
                "CLI is not the exact directly retained configured child: observed={cli:?}; expected_parent={}; expected_uid={}; expected_executable={}",
                std::process::id(),
                nix::unistd::geteuid().as_raw(),
                validated.cli.display()
            )));
        }
        let mut stream = loop {
            startup(&request, stopped)?;
            view.validate()?;
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                Err(e) => return Err(e),
            }
        };
        stream.set_nonblocking(true)?;
        if wire::peer_identity(&stream)? != cli || process_identity(cli.pid)? != cli {
            return Err(fail(
                "observer peer differs from the retained CLI full identity",
            ));
        }
        drop(listener);
        view.validate()?;
        let mut sequence = 0;
        let mut owner = match &request.operation {
            LaunchOperation::Start { .. } => None,
            LaunchOperation::Attach { identity } => Some(identity.owner_uuid),
        };
        let mut record: Option<Record> = None;
        let mut transport: Option<(ProcessIdentity, uuid::Uuid)> = None;
        let mut handed_off = false;
        let mut completed = None;
        let mut terminal_pending = None;
        loop {
            let frame = receive(&mut stream, &request, stopped, !handed_off)?;
            if frame.version != 4
                || frame.context != request.correlation
                || frame.nonce != nonce
                || frame.sequence != sequence + 1
                || frame.sequence > wire::MESSAGE_LIMIT
            {
                return Err(fail("observer lifecycle correlation/sequence rejected"));
            }
            sequence = frame.sequence;
            match frame.event {
                Event::OwnerReserved { owner_uuid }
                    if matches!(&request.operation, LaunchOperation::Start { .. })
                        && owner.is_none()
                        && sequence == 1 =>
                {
                    if owner_uuid.is_nil() {
                        return Err(fail("observer reserved a nil owner"));
                    }
                    events
                        .try_send(LaunchEvent::OwnerReserved {
                            correlation: request.correlation.clone(),
                            owner_uuid,
                        })
                        .map_err(|_| fail("native checkpoint event queue unavailable"))?;
                    let saved = loop {
                        startup(&request, stopped)?;
                        match authorization.try_recv() {
                            Ok(Authorization::Reservation(saved)) => break saved,
                            Ok(Authorization::Completion(_)) => {
                                return Err(fail(
                                    "unexpected completion authorization before reservation",
                                ));
                            }
                            Err(TryRecvError::Disconnected) => {
                                return Err(fail(
                                    "native checkpoint authorization lost; reservation retained",
                                ));
                            }
                            Err(TryRecvError::Empty) => thread::sleep(POLL),
                        }
                    };
                    if saved.correlation != request.correlation
                        || saved.owner_uuid != owner_uuid
                        || saved.document_revision == 0
                    {
                        return Err(fail(
                            "native durable checkpoint authorization correlation rejected",
                        ));
                    }
                    view.validate()?;
                    send_ack(
                        &mut stream,
                        Acknowledgment {
                            version: 4,
                            context: request.correlation.clone(),
                            nonce: nonce.clone(),
                            sequence: 2,
                            event: "reservation_committed".into(),
                            owner_uuid,
                            document_revision: saved.document_revision,
                            owner_generation: None,
                            receipt_digest: None,
                        },
                        &request,
                        stopped,
                    )?;
                    sequence = 2;
                    owner = Some(owner_uuid);
                }
                Event::OwnerBound { record: bound }
                    if !handed_off && record.is_none() && Some(bound.id) == owner =>
                {
                    if bound.verify_live().is_err()
                        && completion::authenticate(&bound, request.startup_deadline)?.is_none()
                    {
                        return Err(fail(
                            "bound owner is neither live nor authenticated completed",
                        ));
                    }
                    if bound
                        .server
                        .as_ref()
                        .is_none_or(|s| s.executable != validated.zellij)
                        || bound
                            .supervisor
                            .as_ref()
                            .is_none_or(|s| s.executable != validated.cli)
                    {
                        return Err(fail("bound owner configured executable mismatch"));
                    }
                    if let LaunchOperation::Start { name, .. } = &request.operation
                        && bound.name != *name
                    {
                        return Err(fail("bound owner Start name mismatch"));
                    }
                    record = Some(bound);
                }
                Event::TransportConnected {
                    owner_uuid,
                    frontend,
                    attachment_id,
                } if transport.is_none()
                    && terminal_pending.is_none()
                    && !handed_off
                    && Some(owner_uuid) == owner
                    && record.is_some() =>
                {
                    let attachment_id = attachment_id
                        .filter(|id| !id.is_nil())
                        .ok_or_else(|| fail("missing readiness attachment identity"))?;
                    if frontend.parent_pid != cli.pid
                        || frontend.uid != cli.uid
                        || frontend.executable != validated.zellij
                    {
                        return Err(fail("frontend is not the configured bound CLI child"));
                    }
                    let frontend_live = match process_identity(frontend.pid) {
                        Ok(current) if current == frontend => true,
                        Ok(_) => return Err(fail("frontend live identity changed")),
                        Err(_) => false,
                    };
                    let bound = record.as_ref().expect("bound record");
                    if !frontend_live || bound.verify_live().is_err() {
                        // Transport progress may be queued before a finite Child
                        // ends and the CLI tears down its frontend. Retain only
                        // independently authenticated terminal evidence, without
                        // publishing progress or seeding the InputReady binding.
                        let terminal = completion::authenticate(bound, request.startup_deadline)?
                            .ok_or_else(|| {
                            fail("historical transport has no authenticated terminal outcome")
                        })?;
                        terminal.matches(bound)?;
                        terminal_pending = Some(terminal);
                        continue;
                    }
                    startup(&request, stopped)?;
                    view.validate()?;
                    events
                        .try_send(LaunchEvent::TransportConnected {
                            correlation: request.correlation.clone(),
                            frontend: frontend.clone(),
                            attachment_id,
                            execution_contract_version: record
                                .as_ref()
                                .expect("bound record")
                                .execution_binding()?
                                .contract_version(),
                        })
                        .map_err(|_| fail("native transport progress queue unavailable"))?;
                    transport = Some((frontend, attachment_id));
                }
                Event::InputReady {
                    owner_uuid,
                    owner_generation,
                    attachment_id,
                    frontend,
                } if !handed_off
                    && terminal_pending.is_none()
                    && completed.is_none()
                    && Some(owner_uuid) == owner
                    && record.is_some() =>
                {
                    let bound = record.as_ref().expect("bound owner");
                    if transport.as_ref() != Some(&(frontend.clone(), attachment_id))
                        || owner_generation != bound.execution_binding()?.owner_generation()
                        || frontend.parent_pid != cli.pid
                        || frontend.uid != cli.uid
                        || frontend.executable != validated.zellij
                        || process_identity(frontend.pid)? != frontend
                    {
                        return Err(fail(
                            "input readiness differs from exact current frontend/attachment",
                        ));
                    }
                    bound.verify_live()?;
                    startup(&request, stopped)?;
                    view.validate()?;
                    ready
                        .try_send(Ok(Ready {
                            admission: ClientAdmission::InputReady { attachment_id },
                            record: bound.clone(),
                            frontend: Some(frontend),
                            execution_contract_version: bound
                                .execution_binding()?
                                .contract_version(),
                        }))
                        .map_err(|_| fail("native input readiness receiver unavailable"))?;
                    handed_off = true;
                }
                Event::WorkloadFinished {
                    owner_uuid,
                    receipt,
                } if Some(owner_uuid) == owner && record.is_some() && completed.is_none() => {
                    let bound = record.as_ref().expect("bound owner");
                    receipt.matches(bound)?;
                    if terminal_pending
                        .as_ref()
                        .is_some_and(|expected| expected != &receipt)
                    {
                        return Err(fail(
                            "correlated terminal event differs from historical transport proof",
                        ));
                    }
                    let authenticated =
                        completion::authenticate(bound, Instant::now() + Duration::from_secs(2))?;
                    if authenticated.as_ref() != Some(&receipt) {
                        return Err(fail(
                            "completion differs from independent live publisher proof",
                        ));
                    }
                    let deadline = Instant::now() + Duration::from_secs(10);
                    if !handed_off {
                        if stopped.load(Ordering::Acquire)
                            || request.cancellation.load(Ordering::Acquire)
                        {
                            return Err(fail(
                                "terminal completion handoff cancelled; publisher retained",
                            ));
                        }
                        ready
                            .try_send(Ok(Ready {
                                admission: ClientAdmission::TerminalObserved {
                                    receipt: Box::new(receipt.clone()),
                                    deadline,
                                },
                                record: bound.clone(),
                                frontend: transport.as_ref().map(|(frontend, _)| frontend.clone()),
                                execution_contract_version: bound
                                    .execution_binding()?
                                    .contract_version(),
                            }))
                            .map_err(|_| {
                                fail("completed retained Session readiness unavailable")
                            })?;
                        handed_off = true;
                    }
                    events
                        .try_send(LaunchEvent::WorkloadFinished {
                            correlation: request.correlation.clone(),
                            receipt: Box::new(receipt.clone()),
                            deadline,
                        })
                        .map_err(|_| fail("native completion event queue unavailable"))?;
                    let saved = loop {
                        if stopped.load(Ordering::Acquire)
                            || request.cancellation.load(Ordering::Acquire)
                            || Instant::now() >= deadline
                        {
                            return Err(fail(
                                "completion authorization cancelled or deadline; publisher retained",
                            ));
                        }
                        match authorization.try_recv() {
                            Ok(Authorization::Completion(saved)) => break saved,
                            Ok(Authorization::Reservation(_)) => {
                                return Err(fail(
                                    "unexpected reservation authorization after completion",
                                ));
                            }
                            Err(TryRecvError::Disconnected) => {
                                return Err(fail("completion authorization source unavailable"));
                            }
                            Err(TryRecvError::Empty) => thread::sleep(POLL),
                        }
                    };
                    if saved.correlation != request.correlation
                        || saved.owner_generation != receipt.owner_generation()
                        || saved.receipt_digest != receipt.digest()?
                        || saved.document_revision == 0
                        || saved.io_id == 0
                    {
                        return Err(fail(
                            "native completion Saved authorization correlation rejected",
                        ));
                    }
                    view.validate()?;
                    let ack = Acknowledgment {
                        version: 4,
                        context: request.correlation.clone(),
                        nonce: nonce.clone(),
                        sequence: sequence + 1,
                        event: "completion_committed".into(),
                        owner_uuid,
                        document_revision: saved.document_revision,
                        owner_generation: Some(saved.owner_generation),
                        receipt_digest: Some(saved.receipt_digest),
                    };
                    let bytes = serde_json::to_vec(&ack).map_err(io::Error::other)?;
                    let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
                    frame.extend(bytes);
                    let mut offset = 0;
                    while offset < frame.len() {
                        if stopped.load(Ordering::Acquire)
                            || request.cancellation.load(Ordering::Acquire)
                            || Instant::now() >= deadline
                        {
                            return Err(fail("completion acknowledgment deadline or cancellation"));
                        }
                        match stream.write(&frame[offset..]) {
                            Ok(0) => return Err(fail("completion acknowledgment zero write")),
                            Ok(n) => offset += n,
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                            Err(e) => return Err(e),
                        }
                    }
                    sequence += 1;
                    completed = Some(receipt);
                }
                Event::FrontendFinished {
                    owner_uuid,
                    receipt,
                } if (transport.is_some() || completed.is_some()) && Some(owner_uuid) == owner => {
                    let bound = record.as_ref().expect("connected record");
                    let outcome_valid = match &receipt.owner_outcome {
                        Some(OwnerOutcome::WorkloadCompleted(terminal)) => {
                            completed.as_ref() == Some(terminal.as_ref())
                                && completion::authenticate(
                                    bound,
                                    Instant::now() + Duration::from_secs(2),
                                )?
                                .as_ref()
                                    == Some(terminal)
                        }
                        Some(OwnerOutcome::LivePreserved) | None => {
                            completed.is_none()
                                && receipt.original_owner_preserved
                                && bound.verify_live().is_ok()
                        }
                    };
                    let valid = receipt.workers_joined && outcome_valid;
                    state
                        .lock()
                        .map_err(|_| fail("observer final publication poisoned"))?
                        .final_receipt = Some(receipt);
                    if !valid {
                        return Err(fail(
                            "observer final original-owner/physical frontend cleanup unconfirmed",
                        ));
                    }
                    // A final frame is not an exit acknowledgment. Reject any
                    // later bytes; the retained CLI must physically close its
                    // connection and the coordinator must still join Session.
                    loop {
                        let mut extra = [0];
                        match stream.read(&mut extra) {
                            Ok(0) => return Ok(()),
                            Ok(_) => {
                                return Err(fail("unexpected observer bytes after final frame"));
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                if stopped.load(Ordering::Acquire) {
                                    return Err(fail(
                                        "observer final connection closure unconfirmed after CLI cleanup",
                                    ));
                                }
                                thread::sleep(POLL);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
                _ => return Err(fail("observer lifecycle state transition rejected")),
            }
        }
    }
}

use std::os::unix::fs::DirBuilderExt;
