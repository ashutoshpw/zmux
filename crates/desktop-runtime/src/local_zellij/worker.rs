use super::{
    AuthorizationReceiver, ClientLaunchFailure, ClientReady, EventSender, LaunchEvent,
    LaunchOperation, LaunchRequest, Observation, ObserverHandle,
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
    Record,
    observer_protocol::{self as wire, Acknowledgment, Event, Frame},
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
struct Ready {
    record: Record,
    frontend: ProcessIdentity,
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
            .map_err(|e| SessionError::Failure(format!("inspect retained CLI identity: {e}")))?;
        identity_tx.try_send(identity).map_err(|_| {
            SessionError::Failure("observer rejected retained CLI identity handoff".into())
        })?;
        loop {
            if cancellation.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(SessionError::Failure(
                    "foreground Zellij startup cancelled or expired; do not replay".into(),
                ));
            }
            match ready_rx.try_recv() {
                Ok(Ok(ready)) => return Ok(ready),
                Ok(Err(e)) => return Err(SessionError::Failure(e)),
                Err(TryRecvError::Disconnected) => {
                    return Err(SessionError::Failure(
                        "observer ended before transport readiness".into(),
                    ));
                }
                Err(TryRecvError::Empty) => (),
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
            record: ready.record,
            frontend: ready.frontend,
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
            return Err(fail(
                "CLI is not the exact directly retained configured child",
            ));
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
        let mut connected = false;
        loop {
            let frame = receive(&mut stream, &request, stopped, !connected)?;
            if frame.version != 1
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
                            Ok(saved) => break saved,
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
                            version: 1,
                            context: request.correlation.clone(),
                            nonce: nonce.clone(),
                            sequence: 2,
                            event: "reservation_committed".into(),
                            owner_uuid,
                            document_revision: saved.document_revision,
                        },
                        &request,
                        stopped,
                    )?;
                    sequence = 2;
                    owner = Some(owner_uuid);
                }
                Event::OwnerBound { record: bound }
                    if !connected && record.is_none() && Some(bound.id) == owner =>
                {
                    bound.verify_live()?;
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
                } if !connected && Some(owner_uuid) == owner && record.is_some() => {
                    if frontend.parent_pid != cli.pid
                        || frontend.uid != cli.uid
                        || frontend.executable != validated.zellij
                        || process_identity(frontend.pid)? != frontend
                    {
                        return Err(fail("frontend is not the live bound CLI child"));
                    }
                    record.as_ref().expect("checked record").verify_live()?;
                    startup(&request, stopped)?;
                    view.validate()?;
                    ready
                        .try_send(Ok(Ready {
                            record: record.as_ref().expect("checked record").clone(),
                            frontend,
                        }))
                        .map_err(|_| {
                            fail("native retained-Session readiness receiver unavailable")
                        })?;
                    connected = true;
                }
                Event::FrontendFinished {
                    owner_uuid,
                    receipt,
                } if connected && Some(owner_uuid) == owner => {
                    let preserved = record.as_ref().expect("connected record").verify_live();
                    let valid = receipt.workers_joined
                        && receipt.original_owner_preserved
                        && preserved.is_ok();
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
