//! Unix-only PTY feasibility runtime. Callers enqueue work; a worker owns process I/O.
//! Drop requests cancellation; explicit shutdown waits only on a non-UI caller.

#[cfg(not(unix))]
compile_error!("The initial Zmux PTY prototype supports Linux and macOS only");

pub mod child;
pub mod shell_integration;
mod terminal;

use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, POLLOUT, poll, pollfd};
use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use shell_integration::{OpenRequestRegistration, OpenUrl};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::fd::{AsRawFd, RawFd},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use terminal::TerminalOwner;
use thiserror::Error;
use zmux_program_status::SessionEpoch;
#[cfg(feature = "ghostty")]
pub use zmux_terminal_adapter::GhosttyLimits;
use zmux_terminal_adapter::{
    EngineBatch, MAX_PROGRAM_OPEN_REQUESTS, Pointer, Size, Viewport, Wheel,
};
pub use zmux_terminal_adapter::{Key, KeyAction, KeyEvent, PASTE_BYTES};

pub const INPUT_CHUNK_BYTES: usize = 4_096;
pub const INPUT_QUEUE_CHUNKS: usize = 16;
/// Maximum payload admitted to the bounded input channel. The active write and
/// deferred PTY ordering buffers are accounted separately.
pub const MAX_QUEUED_INPUT_PAYLOAD_BYTES: usize = INPUT_QUEUE_CHUNKS * PASTE_BYTES;

pub fn default_size() -> Size {
    Size { rows: 24, cols: 80 }
}
const IO_CHUNK_BYTES: usize = 4_096;
const READ_QUANTA: usize = 4;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_WAIT: Duration = Duration::from_millis(250);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const EXIT_REAP_TIMEOUT: Duration = Duration::from_millis(200);
const DEFAULT_CAUSAL_REPLY_TIMEOUT: Duration = Duration::from_secs(2);
// The spill can hold one channel's worth of admitted events plus one refill.
const MAX_DEFERRED_INPUT_EVENTS: usize = INPUT_QUEUE_CHUNKS * 2;
// Preserve one channel payload, one active paste, and one raw input chunk worth
// of PTY output while earlier writes drain; larger interleavings fail closed.
const MAX_DEFERRED_OUTPUT_BYTES: usize =
    MAX_QUEUED_INPUT_PAYLOAD_BYTES + PASTE_BYTES + INPUT_CHUNK_BYTES;
const MAX_DEFERRED_ACTIONS: usize = MAX_DEFERRED_INPUT_EVENTS * 2 + 1;
static NEXT_SESSION_EPOCH: AtomicU64 = AtomicU64::new(1);

fn next_session_epoch() -> Result<SessionEpoch, SessionError> {
    NEXT_SESSION_EPOCH
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |epoch| {
            epoch.checked_add(1)
        })
        .map(SessionEpoch)
        .map_err(|_| SessionError::Failure("session epoch space exhausted".into()))
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Failure(String),
    #[error("session input queue is full; retry explicitly after it drains")]
    QueueFull,
    #[error("input must contain 1–{INPUT_CHUNK_BYTES} bytes")]
    InputSize,
    #[error("text paste must contain 1–{PASTE_BYTES} UTF-8 bytes")]
    PasteSize,
    #[error("session is closed")]
    Closed,
    #[error("session state is being published; retry without blocking")]
    Busy,
    #[error("session shutdown exceeded its deadline")]
    Timeout,
    #[error("session status checkpoint was cancelled")]
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub size: Size,
}

/// How a physically owned PTY process is asked to stop. Graceful mode is for
/// supervisors that must retire their own children before the PTY is released.
#[derive(Clone, Copy, Debug, Default)]
pub enum ShutdownPolicy {
    #[default]
    Kill,
    Graceful {
        timeout: Duration,
    },
}

/// Session selection is local to this crate's feature. Dependency feature
/// unification must not implicitly enable a backend in the worker.
#[derive(Clone, Copy, Debug, Default)]
pub enum EngineKind {
    #[default]
    Vt100,
    #[cfg(feature = "ghostty")]
    Ghostty(GhosttyLimits),
}

/// Explicit optional backend policy. Mapping caps and reply lifetime are caller
/// policy, not default product budgets or a total RSS guarantee. Raw bytes stay
/// exact; typed key/paste/application-wheel fidelity remains a separate gate.
#[derive(Clone, Copy, Debug)]
pub struct EngineOptions {
    pub kind: EngineKind,
    /// Bounds a staged causal reply including any earlier worker-owned user suffix.
    pub reply_timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Exited,
    /// The cancellation loop ended; successful shutdown must confirm cleanup.
    /// Worker termination alone is insufficient. Exit can be nonzero.
    Closed,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub state: State,
    pub viewport: Viewport,
    /// PTY output bytes physically read so far. This can lead the viewport
    /// while output waits behind earlier input and terminal parsing is deferred.
    pub bytes_read: u64,
    pub exit_code: Option<u32>,
    pub failure: Option<String>,
}

/// A worker-ordered terminal status checkpoint. Poll this handle from a
/// coordinator loop; dropping it cancels work that has not reached the worker.
pub struct ProgramStatusCheckpoint {
    receiver: Receiver<Result<Arc<Snapshot>, SessionError>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ProgramStatusCheckpoint {
    /// Returns `None` while the worker has not reached the ordering barrier.
    /// Timeout and worker shutdown are reported as explicit errors.
    pub fn try_recv(&mut self) -> Option<Result<Arc<Snapshot>, SessionError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) if Instant::now() >= self.deadline => {
                self.cancel();
                Some(Err(SessionError::Timeout))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(SessionError::Closed)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl Drop for ProgramStatusCheckpoint {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputWriteStatus {
    Complete,
    NotWritten,
    Partial,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputWriteFailure {
    Deadline,
    SessionClosed,
    SessionExited,
    WorkerFailed,
    WorkerStopped,
}

/// Completion of one bounded input chunk. Complete means acceptance by the
/// local PTY, not consumption by the child or a remote application.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InputWriteResult {
    pub status: InputWriteStatus,
    /// Confirmed accepted bytes. For Unknown this is only a lower bound;
    /// zero does not prove that replay is safe.
    pub bytes_written: usize,
    pub total_bytes: usize,
    pub reason: Option<InputWriteFailure>,
}

const WRITE_QUEUED: u8 = 0;
const WRITE_ACTIVE: u8 = 1;
const WRITE_FINISHED: u8 = 2;

/// A receive-only handle; callers poll without blocking their executor. An
/// admission error has no handle and did not enqueue any input.
pub struct InputReceipt {
    receiver: Receiver<InputWriteResult>,
    queued_completion: Option<WriteCompletion>,
    deadline: Instant,
}
impl InputReceipt {
    pub fn try_recv(&mut self) -> Result<InputWriteResult, TryRecvError> {
        let mut result = self.receiver.try_recv();
        if matches!(result, Err(TryRecvError::Empty)) && Instant::now() >= self.deadline {
            if let Some(completion) = &mut self.queued_completion {
                completion.expire_queued();
            }
            result = self.receiver.try_recv();
        }
        if result.is_ok() {
            self.queued_completion = None;
        }
        result
    }
}

struct WriteCompletion {
    sender: Option<SyncSender<InputWriteResult>>,
    state: Arc<AtomicU8>,
    total: usize,
    written: usize,
    owns_input: bool,
}
impl WriteCompletion {
    fn expire_queued(&mut self) {
        // Starting the write and cancelling queued input race on one state
        // transition. A successful cancellation proves no write can follow.
        if self
            .state
            .compare_exchange(
                WRITE_QUEUED,
                WRITE_FINISHED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
            && let Some(sender) = self.sender.take()
        {
            let _ = sender.try_send(InputWriteResult {
                status: InputWriteStatus::NotWritten,
                bytes_written: 0,
                total_bytes: self.total,
                reason: Some(InputWriteFailure::Deadline),
            });
        }
    }
    fn start(&self) -> bool {
        self.state
            .compare_exchange(
                WRITE_QUEUED,
                WRITE_ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
    fn finish(&mut self, reason: Option<InputWriteFailure>, unknown: bool) {
        let previous = self.state.swap(WRITE_FINISHED, Ordering::AcqRel);
        if previous == WRITE_FINISHED {
            self.sender = None;
            return;
        }
        let status = if unknown {
            InputWriteStatus::Unknown
        } else if reason.is_none() {
            InputWriteStatus::Complete
        } else if self.written == 0 {
            InputWriteStatus::NotWritten
        } else {
            InputWriteStatus::Partial
        };
        if let Some(sender) = self.sender.take() {
            let _ = sender.try_send(InputWriteResult {
                status,
                bytes_written: self.written,
                total_bytes: self.total,
                reason,
            });
        }
    }
}
impl Drop for WriteCompletion {
    fn drop(&mut self) {
        if self.owns_input {
            let active = self.state.load(Ordering::Acquire) == WRITE_ACTIVE;
            self.finish(Some(InputWriteFailure::WorkerStopped), active);
        }
    }
}

enum Input {
    Bytes {
        bytes: Vec<u8>,
        completion: Option<WriteCompletion>,
        deadline: Option<Instant>,
    },
    Key(KeyEvent),
    Paste(String),
    Scroll(i32),
    Wheel(Wheel),
    Pointer(Pointer),
    ProgramStatusCheckpoint {
        reply: SyncSender<Result<Arc<Snapshot>, SessionError>>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    },
}

enum DeferredAction {
    Input(Input),
    Output {
        bytes: Vec<u8>,
        offset: usize,
        ordering_deadline: Option<Instant>,
    },
}

fn finish_input(input: Input, reason: InputWriteFailure) {
    match input {
        Input::Bytes { mut completion, .. } => {
            if let Some(completion) = &mut completion {
                completion.finish(Some(reason), false);
            }
        }
        Input::ProgramStatusCheckpoint {
            reply, cancelled, ..
        } => {
            if !cancelled.load(Ordering::Acquire) {
                let _ = reply.try_send(Err(SessionError::Closed));
            }
        }
        Input::Key(_)
        | Input::Paste(_)
        | Input::Scroll(_)
        | Input::Wheel(_)
        | Input::Pointer(_) => {}
    }
}

struct Shared {
    stop: AtomicBool,
    // Serializes queue admission against the worker's final pre-read check.
    input_admission: Mutex<()>,
    close_deadline: Arc<Mutex<Option<Instant>>>,
    exited: AtomicBool,
    cleanup_failed: AtomicBool,
    resize: Mutex<Option<Size>>,
    snapshot: Mutex<Arc<Snapshot>>,
    open_urls: Mutex<VecDeque<OpenUrl>>,
}

pub struct Session {
    input: SyncSender<Input>,
    wake: Mutex<UnixStream>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    process_id: u32,
    shutdown_policy: ShutdownPolicy,
    application_wheel_supported: bool,
    open_request_registration: Option<OpenRequestRegistration>,
}

// Borrows a live PTY only long enough for FileDescriptor to duplicate its descriptor.
struct MasterFd<'a>(&'a dyn MasterPty);
impl AsRawFd for MasterFd<'_> {
    fn as_raw_fd(&self) -> RawFd {
        self.0
            .as_raw_fd()
            .expect("validated native master descriptor")
    }
}

struct ProcessGuard {
    child: Box<dyn Child + Send + Sync>,
    reaped: bool,
    signaled: bool,
    pid: u32,
    close_deadline: Arc<Mutex<Option<Instant>>>,
    exit_reap_deadline: Option<Instant>,
}
impl ProcessGuard {
    fn poll_exit(&mut self) -> Result<bool, SessionError> {
        if self.reaped {
            return Ok(true);
        }
        child::observe_child(self.pid, false)
            .map(|status| status.is_some())
            .map_err(|e| SessionError::Failure(format!("observe canceled PTY process: {e}")))
    }
    fn signal(&mut self) -> Result<(), SessionError> {
        if self.signaled || self.poll_exit()? {
            return Ok(());
        }
        self.send_signal(Signal::SIGKILL)?;
        self.signaled = true;
        Ok(())
    }
    fn send_signal(&mut self, signal: Signal) -> Result<(), SessionError> {
        if self.poll_exit()? {
            return Ok(());
        }
        // The leader remains unreaped, so its PID cannot be reused. Signal the group once.
        match killpg(Pid::from_raw(self.pid as i32), signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(Errno::EPERM) if self.poll_exit()? => Ok(()),
            Err(error) => {
                // Retain the failed signal and independent owned-child state.
                // Observation errors grant no authority or successful cleanup.
                let group = nix::unistd::getpgid(Some(Pid::from_raw(self.pid as i32)));
                let mut reap = None;
                let mut reap_attempts = 0;
                let reap_started = Instant::now();
                if error == Errno::EPERM && matches!(group, Err(Errno::ESRCH)) {
                    // Darwin can remove a PTY group during hangup before its
                    // retained leader becomes waitable. Do not signal again or
                    // infer exit from ESRCH: only an actual child reap succeeds.
                    // Neither Drop retries nor later close requests renew time.
                    let deadline = *self
                        .exit_reap_deadline
                        .get_or_insert(reap_started + EXIT_REAP_TIMEOUT);
                    loop {
                        reap_attempts += 1;
                        match self.child.try_wait() {
                            Ok(Some(status)) => {
                                self.reaped = true;
                                eprintln!(
                                    "zmux PTY cleanup confirmed direct-child reap: pid={}; group_signal=EPERM; getpgid=ESRCH; status={status:?}; attempts={reap_attempts}; elapsed={:?}",
                                    self.pid,
                                    reap_started.elapsed()
                                );
                                return Ok(());
                            }
                            result => {
                                let failed = result.is_err();
                                reap = Some(result);
                                if failed {
                                    break;
                                }
                            }
                        }
                        let close_deadline = self.close_deadline.lock().map_err(|_| {
                            SessionError::Failure("PTY cleanup deadline poisoned".into())
                        })?;
                        let deadline = close_deadline.map_or(deadline, |close| close.min(deadline));
                        drop(close_deadline);
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        thread::sleep(remaining.min(Duration::from_millis(2)));
                    }
                }
                let status = child::observe_child(self.pid, true);
                Err(SessionError::Failure(format!(
                    "terminate PTY process group: {error}; signal={signal:?}; owned_child_pid={}; observed_group={group:?}; owned_child_status={status:?}; owned_child_reap={reap:?}; reap_attempts={reap_attempts}; reap_elapsed={:?}",
                    self.pid,
                    reap_started.elapsed()
                )))
            }
        }
    }
    fn stop(&mut self) -> Result<(), SessionError> {
        if self.reaped {
            return Ok(());
        }
        self.signal()?;
        if self.reaped {
            return Ok(());
        }
        self.child
            .wait()
            .map_err(|e| SessionError::Failure(format!("reap PTY process: {e}")))?;
        self.reaped = true;
        Ok(())
    }
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("zmux session cleanup failed: {error}");
        }
    }
}

impl Session {
    pub fn spawn(launch: Launch) -> Result<Self, SessionError> {
        Self::spawn_with_environment(launch, &[])
    }

    /// Application-owned terminal context overrides any inherited parent-terminal context.
    pub fn spawn_with_environment(
        launch: Launch,
        environment: &[(String, String)],
    ) -> Result<Self, SessionError> {
        Self::spawn_with_resource(launch, environment, ())
    }

    /// Keep a production-owned resource alive through PTY and child cleanup.
    /// Once the worker starts, release happens there even on startup failure.
    /// OS thread-creation failure releases the captured resource on the caller.
    /// Startup and explicit shutdown belong on background callers, never AppKit/core.
    pub fn spawn_with_resource(
        launch: Launch,
        environment: &[(String, String)],
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        Self::spawn_with_resource_and_removals(launch, environment, &[], resource)
    }

    /// Apply removals before owned overrides, without mutating process-global state.
    /// Native hosts use this to isolate a subprocess from inherited terminal context.
    pub fn spawn_with_resource_and_removals(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        Self::spawn_with_shutdown_policy(
            launch,
            environment,
            removals,
            ShutdownPolicy::Kill,
            resource,
        )
    }

    /// An owning launch coordinator must retain this Session until is_finished
    /// even if its own join deadline expires. Never call shutdown on a UI/core
    /// thread or treat a timed-out join as completed physical cleanup.
    pub fn spawn_with_shutdown_policy(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        shutdown_policy: ShutdownPolicy,
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        Self::spawn_configured(
            launch,
            environment,
            removals,
            shutdown_policy,
            None,
            None,
            resource,
        )
    }

    /// Opt into a backend without changing app/default engine selection. Queue
    /// acceptance is not worker admission: while a causal reply is staged,
    /// external chunks remain queued; the current worker-owned chunk finishes
    /// before that reply. All physical writes use the same worker/PTY descriptor.
    pub fn spawn_with_engine(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        shutdown_policy: ShutdownPolicy,
        engine: EngineOptions,
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        Self::spawn_configured(
            launch,
            environment,
            removals,
            shutdown_policy,
            Some(engine),
            None,
            resource,
        )
    }

    /// Spawn with a private opener registration owned by this exact PTY session.
    /// The registration is never included in snapshots or child argv; the worker
    /// installs its nonce directly into the child environment.
    pub fn spawn_with_open_requests(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        shutdown_policy: ShutdownPolicy,
        engine: Option<EngineOptions>,
        registration: Option<OpenRequestRegistration>,
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        Self::spawn_configured(
            launch,
            environment,
            removals,
            shutdown_policy,
            engine,
            registration,
            resource,
        )
    }

    fn spawn_configured(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        shutdown_policy: ShutdownPolicy,
        engine: Option<EngineOptions>,
        registration: Option<OpenRequestRegistration>,
        resource: impl Send + 'static,
    ) -> Result<Self, SessionError> {
        if engine.is_some_and(|options| {
            options.reply_timeout.is_zero() || options.reply_timeout > Duration::from_secs(30)
        }) {
            return Err(SessionError::Failure(
                "engine reply timeout must be positive and at most 30s".into(),
            ));
        }
        let application_wheel_supported = matches!(
            engine.map(|options| options.kind),
            None | Some(EngineKind::Vt100)
        );
        // Launch configuration crosses the thread boundary; the terminal never does.
        let environment = environment.to_vec();
        let removals = removals.to_vec();
        let (input_tx, input_rx) = mpsc::sync_channel(INPUT_QUEUE_CHUNKS);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("zmux-session".into())
            .spawn(move || {
                let _resource = resource;
                let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Worker::start(
                        launch,
                        environment,
                        removals,
                        shutdown_policy,
                        engine,
                        registration,
                        input_rx,
                    )
                }));
                match started {
                    Ok(Ok((worker, ready))) => {
                        if startup_tx.send(Ok(ready)).is_err() {
                            worker.shared.stop.store(true, Ordering::Release);
                        }
                        worker.execute();
                    }
                    Ok(Err(error)) => {
                        let _ = startup_tx.send(Err(error));
                    }
                    Err(_) => {
                        let _ = startup_tx.send(Err(SessionError::Failure(
                            "session worker panicked during startup".into(),
                        )));
                    }
                }
            })
            .map_err(|e| SessionError::Failure(format!("start session worker: {e}")))?;
        match startup_rx.recv() {
            Ok(Ok(ready)) => Ok(Self {
                input: input_tx,
                wake: Mutex::new(ready.wake),
                shared: ready.shared,
                worker: Some(worker),
                process_id: ready.process_id,
                shutdown_policy,
                application_wheel_supported,
                open_request_registration: registration,
            }),
            result => {
                // A startup failure is not a running Session. Join its ended worker
                // so startup resources have been released before returning the error.
                worker.join().map_err(|_| {
                    SessionError::Failure("session startup cleanup panicked".into())
                })?;
                match result {
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(SessionError::Failure(
                        "session worker stopped before startup acknowledgement".into(),
                    )),
                    Ok(Ok(_)) => unreachable!(),
                }
            }
        }
    }

    /// Reports worker termination without blocking the coordinator's command loop.
    /// Call shutdown to join and check cleanup; this alone is not cleanup proof.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Private in-memory owner binding for this PTY's explicit browser requests.
    pub fn open_request_registration(&self) -> Option<OpenRequestRegistration> {
        self.open_request_registration
    }

    /// Drain URL-only requests accepted by the worker after terminal commit.
    /// Closed sessions fail closed even if a request was waiting in the queue.
    pub fn take_open_urls(&self) -> Result<Vec<OpenUrl>, SessionError> {
        self.ensure_running()?;
        let mut queue = self.shared.open_urls.try_lock().map_err(lock_error)?;
        let mut urls = Vec::new();
        urls.try_reserve_exact(queue.len()).map_err(|_| {
            SessionError::Failure("cannot reserve bounded open-request drain".into())
        })?;
        urls.extend(queue.drain(..));
        Ok(urls)
    }

    /// Explicit raw byte injection; semantic key/paste APIs encode on the worker.
    pub fn send_input(&self, bytes: &[u8]) -> Result<(), SessionError> {
        if bytes.is_empty() || bytes.len() > INPUT_CHUNK_BYTES {
            return Err(SessionError::InputSize);
        }
        self.ensure_running()?;
        self.enqueue_bytes(bytes, None, None)
    }

    /// Enqueue once; the sole worker encodes only after earlier causal stages
    /// commit. Queue acceptance is not physical delivery and never permits replay.
    pub fn send_key(&self, event: KeyEvent) -> Result<(), SessionError> {
        if !event.valid() {
            return Err(SessionError::Failure(
                "invalid semantic key modifiers".into(),
            ));
        }
        self.ensure_running()?;
        self.enqueue_event(Input::Key(event))
    }
    /// One bounded clipboard paste stays one queue event and bracketed unit.
    /// Mode selection and optional candidate sanitization happen on the worker.
    pub fn send_paste(&self, text: &str) -> Result<(), SessionError> {
        if text.is_empty() || text.len() > PASTE_BYTES {
            return Err(SessionError::PasteSize);
        }
        self.ensure_running()?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|_| SessionError::Failure("cannot reserve bounded paste storage".into()))?;
        owned.push_str(text);
        self.enqueue_event(Input::Paste(owned))
    }

    /// Enqueue once in the ordinary input queue and report actual PTY write
    /// completion. The absolute deadline bounds writing, including queue time.
    /// Never replay a partial, unknown, or missing result automatically.
    pub fn send_input_with_receipt(
        &self,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<InputReceipt, SessionError> {
        if bytes.is_empty() || bytes.len() > INPUT_CHUNK_BYTES {
            return Err(SessionError::InputSize);
        }
        self.ensure_running()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let state = Arc::new(AtomicU8::new(WRITE_QUEUED));
        let queued_completion = WriteCompletion {
            sender: Some(sender.clone()),
            state: Arc::clone(&state),
            total: bytes.len(),
            written: 0,
            owns_input: false,
        };
        let completion = WriteCompletion {
            sender: Some(sender),
            state,
            total: bytes.len(),
            written: 0,
            owns_input: true,
        };
        self.enqueue_bytes(bytes, Some(completion), Some(deadline))?;
        Ok(InputReceipt {
            receiver,
            queued_completion: Some(queued_completion),
            deadline,
        })
    }

    fn enqueue_bytes(
        &self,
        bytes: &[u8],
        completion: Option<WriteCompletion>,
        deadline: Option<Instant>,
    ) -> Result<(), SessionError> {
        let admission = self
            .shared
            .input_admission
            .lock()
            .map_err(|_| SessionError::Failure("input admission lock poisoned".into()))?;
        self.input
            .try_send(Input::Bytes {
                bytes: bytes.to_vec(),
                completion,
                deadline,
            })
            .map_err(|e| match e {
                TrySendError::Full(_) => SessionError::QueueFull,
                TrySendError::Disconnected(_) => SessionError::Closed,
            })?;
        drop(admission);
        // Enqueueing is the acknowledgment. A failed wake must not invite a duplicate resend;
        // the worker also checks queued work on its bounded idle timeout.
        if let Err(error) = self.wake() {
            eprintln!("zmux input accepted; scheduling wake failed: {error}");
        }
        Ok(())
    }

    /// History navigation shares the bounded input queue so typing and scrolling
    /// preserve order. Only the session worker mutates the terminal parser.
    pub fn scroll(&self, lines: i32) -> Result<(), SessionError> {
        self.enqueue_event(Input::Scroll(lines.clamp(-1000, 1000)))
    }
    pub fn pointer(&self, event: Pointer) -> Result<(), SessionError> {
        self.ensure_running()?;
        if !self.application_wheel_supported {
            return Err(SessionError::Failure(
                "Ghostty pointer reporting is not yet supported".into(),
            ));
        }
        self.enqueue_event(Input::Pointer(event))
    }
    pub fn wheel(&self, event: Wheel) -> Result<(), SessionError> {
        if !event.history && !self.application_wheel_supported {
            return Err(SessionError::Failure("Ghostty application wheel encoding is not yet supported; explicit history navigation is available".into()));
        }
        self.enqueue_event(Input::Wheel(event))
    }
    fn enqueue_event(&self, event: Input) -> Result<(), SessionError> {
        self.ensure_available()?;
        let admission = self
            .shared
            .input_admission
            .lock()
            .map_err(|_| SessionError::Failure("input admission lock poisoned".into()))?;
        self.input.try_send(event).map_err(|e| match e {
            TrySendError::Full(_) => SessionError::QueueFull,
            TrySendError::Disconnected(_) => SessionError::Closed,
        })?;
        drop(admission);
        if let Err(error) = self.wake() {
            eprintln!("zmux navigation accepted; scheduling wake failed: {error}");
        }
        Ok(())
    }

    /// Resize is coalesced; only the worker calls the PTY and parser resize APIs.
    pub fn resize(&self, size: Size) -> Result<(), SessionError> {
        size.validate()
            .map_err(|e| SessionError::Failure(e.to_string()))?;
        self.ensure_running()?;
        *self.shared.resize.try_lock().map_err(lock_error)? = Some(size);
        self.wake()
    }

    pub fn snapshot(&self) -> Result<Arc<Snapshot>, SessionError> {
        self.shared
            .snapshot
            .try_lock()
            .map(|s| Arc::clone(&s))
            .map_err(lock_error)
    }

    /// Request a status checkpoint ordered with PTY reads by the session worker.
    /// The returned handle is nonblocking; its deadline bounds queueing and
    /// parsing work, and dropping it cancels an unserviced request.
    pub fn request_program_status_checkpoint(
        &self,
        deadline: Instant,
    ) -> Result<ProgramStatusCheckpoint, SessionError> {
        self.ensure_running()?;
        if deadline <= Instant::now() {
            return Err(SessionError::Timeout);
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let admission = self.shared.input_admission.try_lock().map_err(lock_error)?;
        self.input
            .try_send(Input::ProgramStatusCheckpoint {
                reply,
                cancelled: Arc::clone(&cancelled),
                deadline,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => SessionError::QueueFull,
                TrySendError::Disconnected(_) => SessionError::Closed,
            })?;
        drop(admission);
        if let Err(error) = self.wake() {
            eprintln!(
                "program status checkpoint accepted; worker will poll its bounded queue: {error}"
            );
        }
        Ok(ProgramStatusCheckpoint {
            receiver,
            cancelled,
            deadline,
        })
    }

    /// This is a nonblocking cancellation request, safe for an eventual native view teardown.
    pub fn request_close(&self) -> Result<(), SessionError> {
        self.shared.stop.store(true, Ordering::Release);
        self.wake()
    }

    /// Tighten cancellation to a shared shutdown deadline without renewing it.
    /// The existing post-exit PTY drain is reserved within that deadline. A final
    /// OS reap can remain pending. is_finished reports worker termination only;
    /// successful shutdown joins the worker and confirms cleanup.
    pub fn request_close_until(&self, deadline: Instant) -> Result<(), SessionError> {
        let mut current = self
            .shared
            .close_deadline
            .lock()
            .map_err(|_| SessionError::Failure("close deadline poisoned".into()))?;
        *current = Some(current.map_or(deadline, |previous| previous.min(deadline)));
        drop(current);
        self.request_close()
    }

    /// Explicit joining is for CLI/worker owners, never the AppKit main thread.
    pub fn shutdown(&mut self, timeout: Duration) -> Result<(), SessionError> {
        self.request_close()?;
        let deadline = Instant::now() + timeout;
        if let Some(worker) = self.worker.as_ref() {
            while !worker.is_finished() {
                if Instant::now() >= deadline {
                    return Err(SessionError::Timeout);
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| SessionError::Failure("session worker panicked".into()))?;
        }
        if self.shared.cleanup_failed.load(Ordering::Acquire) {
            let failure = self
                .shared
                .snapshot
                .lock()
                .map_err(|_| SessionError::Failure("cleanup failure snapshot poisoned".into()))?
                .failure
                .clone()
                .unwrap_or_else(|| "PTY cleanup failed without a retained diagnostic".into());
            return Err(SessionError::Failure(failure));
        }
        if matches!(self.shutdown_policy, ShutdownPolicy::Graceful { .. })
            && let Some(failure) = &self
                .shared
                .snapshot
                .lock()
                .map_err(|_| {
                    SessionError::Failure("session snapshot poisoned after shutdown".into())
                })?
                .failure
        {
            return Err(SessionError::Failure(failure.clone()));
        }
        Ok(())
    }

    fn ensure_running(&self) -> Result<(), SessionError> {
        if self.shared.exited.load(Ordering::Acquire) {
            return Err(SessionError::Closed);
        }
        self.ensure_available()
    }
    fn ensure_available(&self) -> Result<(), SessionError> {
        if self.shared.stop.load(Ordering::Acquire)
            || self.worker.as_ref().is_none_or(JoinHandle::is_finished)
        {
            return Err(SessionError::Closed);
        }
        Ok(())
    }

    fn wake(&self) -> Result<(), SessionError> {
        match self.wake.try_lock().map_err(lock_error)?.write(&[1]) {
            Ok(_) => Ok(()),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::BrokenPipe =>
            {
                Ok(())
            }
            Err(e) => Err(SessionError::Failure(format!("wake session: {e}"))),
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Err(error) = self.request_close() {
            eprintln!("zmux cancellation wake failed: {error}");
        }
    }
}

// Only owned/shared values cross startup acknowledgement; no terminal handle.
struct StartupReady {
    wake: UnixStream,
    shared: Arc<Shared>,
    process_id: u32,
}

// Field order is intentional: release both master descriptors before ProcessGuard waits.
struct Worker {
    master: Option<Box<dyn MasterPty + Send>>,
    io: Option<FileDescriptor>,
    wake: UnixStream,
    process: ProcessGuard,
    terminal: TerminalOwner,
    input: Receiver<Input>,
    deferred: VecDeque<DeferredAction>,
    deferred_input_events: usize,
    deferred_output_bytes: usize,
    shared: Arc<Shared>,
    bytes_read: u64,
    exit_code: Option<u32>,
    exit_seen: Option<Instant>,
    eof: bool,
    pending: Vec<u8>,
    written: usize,
    pending_completion: Option<WriteCompletion>,
    pending_deadline: Option<Instant>,
    pending_checkpoint: Option<PendingProgramStatusCheckpoint>,
    checkpoint_read_drained: bool,
    shutdown_policy: ShutdownPolicy,
    close_deadline: Option<Instant>,
    engine_reply_timeout: Option<Duration>,
    causal_reply: Option<CausalReply>,
    open_request_registration: Option<OpenRequestRegistration>,
}

struct CausalReply {
    batch: EngineBatch,
    written: usize,
    deadline: Instant,
}

struct PendingProgramStatusCheckpoint {
    reply: SyncSender<Result<Arc<Snapshot>, SessionError>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl Worker {
    fn start(
        launch: Launch,
        environment: Vec<(String, String)>,
        removals: Vec<std::ffi::OsString>,
        shutdown_policy: ShutdownPolicy,
        engine: Option<EngineOptions>,
        open_request_registration: Option<OpenRequestRegistration>,
        input_rx: Receiver<Input>,
    ) -> Result<(Self, StartupReady), SessionError> {
        if let ShutdownPolicy::Graceful { timeout } = shutdown_policy
            && (timeout.is_zero() || timeout > Duration::from_secs(30))
        {
            return Err(SessionError::Failure(
                "graceful shutdown timeout must be positive and at most 30s".into(),
            ));
        }
        // portable-pty may fall back to a home directory on a failed chdir. Restore
        // must fail visibly rather than launching a shell in an unexpected location.
        if let Some(cwd) = &launch.cwd {
            let metadata = std::fs::metadata(cwd).map_err(|error| {
                SessionError::Failure(format!(
                    "working directory {} is unavailable: {error}",
                    cwd.display()
                ))
            })?;
            if !metadata.is_dir() {
                return Err(SessionError::Failure(format!(
                    "working directory {} is not a directory",
                    cwd.display()
                )));
            }
        }
        let expected_open_nonce =
            open_request_registration.map(|registration| registration.nonce().adapter_value());
        let terminal = TerminalOwner::new(
            launch.size,
            engine,
            expected_open_nonce,
            next_session_epoch()?,
        )?;
        // Export the first frame before a child can produce any terminal output.
        let viewport = terminal.viewport();
        let pair = native_pty_system()
            .openpty(pty_size(launch.size))
            .map_err(|e| SessionError::Failure(format!("open PTY: {e}")))?;
        if pair.master.as_raw_fd().is_none() {
            return Err(SessionError::Failure("PTY has no native descriptor".into()));
        }
        let mut io = FileDescriptor::dup(&MasterFd(&*pair.master))
            .map_err(|e| SessionError::Failure(format!("duplicate PTY: {e}")))?;
        io.set_non_blocking(true)
            .map_err(|e| SessionError::Failure(format!("configure PTY: {e}")))?;
        // Linux standard-library sends suppress SIGPIPE. Darwin requires the
        // socket option explicitly, including for a native host with SIG_DFL.
        // Never change process-wide signal disposition from this library.
        let (wake_tx, wake_rx) = UnixStream::pair()
            .map_err(|e| SessionError::Failure(format!("create wake channel: {e}")))?;
        #[cfg(target_os = "macos")]
        socket2::SockRef::from(&wake_tx)
            .set_nosigpipe(true)
            .map_err(|e| SessionError::Failure(format!("configure wake SIGPIPE handling: {e}")))?;
        wake_tx
            .set_nonblocking(true)
            .map_err(|e| SessionError::Failure(e.to_string()))?;
        wake_rx
            .set_nonblocking(true)
            .map_err(|e| SessionError::Failure(e.to_string()))?;
        let mut command = CommandBuilder::new(&launch.program);
        command.args(&launch.args);
        command.env("TERM", "xterm-256color");
        for name in &removals {
            command.env_remove(name);
        }
        for (name, value) in &environment {
            command.env(name, value);
        }
        // This reserved nonce is always scrubbed from inherited/user context;
        // only the matching private session registration may install it.
        command.env_remove("ZMUX_OPEN_URL_NONCE");
        if let Some(registration) = open_request_registration {
            command.env("ZMUX_OPEN_URL_NONCE", registration.nonce().protocol_value());
        }
        command.env_remove("ZMUX_SOCKET_PASSWORD");
        if let Some(cwd) = launch.cwd {
            command.cwd(cwd);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| SessionError::Failure(format!("launch PTY command: {e}")))?;
        let pid = child
            .process_id()
            .ok_or_else(|| SessionError::Failure("PTY child has no PID".into()))?;
        let close_deadline = Arc::new(Mutex::new(None));
        let process = ProcessGuard {
            child,
            reaped: false,
            signaled: false,
            pid,
            close_deadline: Arc::clone(&close_deadline),
            exit_reap_deadline: None,
        };
        drop(pair.slave);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            input_admission: Mutex::new(()),
            close_deadline,
            exited: AtomicBool::new(false),
            cleanup_failed: AtomicBool::new(false),
            resize: Mutex::new(None),
            snapshot: Mutex::new(Arc::new(Snapshot {
                state: State::Running,
                viewport,
                bytes_read: 0,
                exit_code: None,
                failure: None,
            })),
            open_urls: Mutex::new(VecDeque::with_capacity(MAX_PROGRAM_OPEN_REQUESTS)),
        });
        let mut deferred = VecDeque::new();
        deferred
            .try_reserve_exact(MAX_DEFERRED_ACTIONS)
            .map_err(|_| {
                SessionError::Failure("cannot reserve bounded PTY ordering queue".into())
            })?;
        let worker = Self {
            master: Some(pair.master),
            io: Some(io),
            wake: wake_rx,
            process,
            terminal,
            input: input_rx,
            deferred,
            deferred_input_events: 0,
            deferred_output_bytes: 0,
            shared: Arc::clone(&shared),
            bytes_read: 0,
            exit_code: None,
            exit_seen: None,
            eof: false,
            pending: Vec::new(),
            written: 0,
            pending_completion: None,
            pending_deadline: None,
            pending_checkpoint: None,
            checkpoint_read_drained: false,
            shutdown_policy,
            close_deadline: None,
            engine_reply_timeout: Some(engine.map_or(DEFAULT_CAUSAL_REPLY_TIMEOUT, |options| {
                options.reply_timeout
            })),
            causal_reply: None,
            open_request_registration,
        };
        Ok((
            worker,
            StartupReady {
                wake: wake_tx,
                shared,
                process_id: pid,
            },
        ))
    }

    fn execute(mut self) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut state = self.run()?;
            if state == State::Exited {
                let batch = self.terminal.attached_child_exited()?;
                if !batch.replies().is_empty() {
                    self.abort_engine();
                    return Err(SessionError::Failure(
                        "child-exit lifecycle unexpectedly requested a PTY reply".into(),
                    ));
                }
                if !self.commit_batch(&batch)? {
                    state = State::Closed;
                }
            }
            if state == State::Exited {
                self.finish_inputs(InputWriteFailure::SessionExited);
                // History retains only terminal state. Retire transport and the
                // physically owned child before publishing completed history.
                drop(self.io.take());
                drop(self.master.take());
                self.process.stop()?;
                self.shared.exited.store(true, Ordering::Release);
                self.publish(state, None, true)?;
                self.review_history()?;
            }
            Ok::<State, SessionError>(state)
        }));
        self.shared.stop.store(true, Ordering::Release);
        match result {
            Ok(Ok(state)) => {
                self.finish_inputs(InputWriteFailure::SessionClosed);
                let publication = if state == State::Closed {
                    self.terminal
                        .terminal_closed()
                        .and_then(|viewport| self.publish_viewport(state, None, viewport))
                } else {
                    self.publish(state, None, true)
                };
                if let Err(error) = publication {
                    eprintln!("zmux session final snapshot publication failed: {error}");
                    if let Err(failure) =
                        self.publish(State::Failed, Some(error.to_string()), false)
                    {
                        eprintln!("zmux session failure publication failed: {failure}");
                    }
                }
            }
            Ok(Err(error)) => {
                self.abort_engine();
                self.finish_inputs(InputWriteFailure::WorkerFailed);
                if let Err(publish_error) =
                    self.publish(State::Failed, Some(error.to_string()), false)
                {
                    eprintln!("zmux failed snapshot publication: {publish_error}");
                }
            }
            Err(_) => {
                self.abort_engine();
                // Dropping guards reports queued input as not written
                // and active input as unknown before process cleanup.
                drop(self.pending_completion.take());
                for input in self.input.try_iter() {
                    drop(input);
                }
                // Do not enter a possibly corrupted parser after a worker panic.
                match self.shared.snapshot.lock() {
                    Ok(mut slot) => {
                        let mut last = (**slot).clone();
                        last.state = State::Failed;
                        last.failure =
                            Some("session worker panicked; session recovery required".into());
                        *slot = Arc::new(last);
                    }
                    Err(error) => eprintln!("zmux panic publication failed: {error}"),
                }
            }
        }
        // Explicitly report cleanup failures rather than hiding them inside Drop.
        let shared = Arc::clone(&self.shared);
        let Worker {
            master,
            io,
            mut process,
            ..
        } = self;
        drop(io);
        drop(master);
        if let Err(error) = process.stop() {
            shared.cleanup_failed.store(true, Ordering::Release);
            match shared.snapshot.lock() {
                Ok(mut slot) => {
                    let mut last = (**slot).clone();
                    last.state = State::Failed;
                    let cleanup = format!("PTY cleanup failed: {error}");
                    last.failure = Some(match last.failure.take() {
                        Some(primary) => format!("{primary}; {cleanup}"),
                        None => cleanup,
                    });
                    *slot = Arc::new(last);
                }
                Err(error) => eprintln!("zmux cleanup publication failed: {error}"),
            }
        }
    }

    fn run(&mut self) -> Result<State, SessionError> {
        let mut published = Instant::now();
        let mut dirty = false;
        loop {
            if self.shared.stop.load(Ordering::Acquire) {
                self.abort_engine();
                match self.shutdown_policy {
                    ShutdownPolicy::Kill => {
                        // Darwin can keep an output-blocked child in exit until
                        // PTY master close. Existing immediate cleanup is unchanged.
                        self.process.signal()?;
                        return Ok(State::Closed);
                    }
                    ShutdownPolicy::Graceful { timeout } => {
                        // The terminal was aborted above; retire any pre-close
                        // dirty frame so the bounded drain uses idle polling.
                        dirty = false;
                        if self.close_deadline.is_none() {
                            self.finish_inputs(InputWriteFailure::SessionClosed);
                            self.process.send_signal(Signal::SIGTERM)?;
                            self.close_deadline = Some(Instant::now() + timeout);
                        }
                        if let Some(deadline) =
                            *self.shared.close_deadline.lock().map_err(|_| {
                                SessionError::Failure("close deadline poisoned".into())
                            })?
                        {
                            let graceful_deadline = deadline
                                .checked_sub(DRAIN_TIMEOUT)
                                .unwrap_or_else(Instant::now);
                            self.close_deadline = self
                                .close_deadline
                                .map(|current| current.min(graceful_deadline));
                        }
                        if self
                            .close_deadline
                            .is_some_and(|deadline| Instant::now() >= deadline)
                            && !self.process.poll_exit()?
                        {
                            self.process.signal()?;
                            return Err(SessionError::Failure("graceful shutdown deadline; forced retained child cleanup, supervisor outcome is unknown".into()));
                        }
                    }
                }
            }
            if self.causal_reply.is_none()
                && self.close_deadline.is_none()
                && self.pending.is_empty()
            {
                if !self.deferred.is_empty() {
                    dirty |= self.advance_deferred()?;
                } else {
                    let size = self
                        .shared
                        .resize
                        .lock()
                        .map_err(|_| SessionError::Failure("resize lock poisoned".into()))?
                        .take();
                    if let Some(size) = size {
                        // Stage engine geometry before the irreversible kernel resize.
                        // Failure aborts; no rollback-atomicity claim is made.
                        let staged = self.terminal.stages_mutations();
                        let batch = if staged {
                            self.terminal.resize(size)?
                        } else {
                            None
                        };
                        self.master
                        .as_ref()
                        .expect("active PTY master")
                        .resize(pty_size(size))
                        .map_err(|error| {
                            SessionError::Failure(format!(
                                "resize PTY after engine stage: {error}; session recovery required"
                            ))
                        })?;
                        let batch = if staged {
                            batch
                        } else {
                            self.terminal.resize(size)?
                        };
                        dirty |= self.accept_batch(batch)?;
                    }
                    dirty |= self.load_input()?;
                }
            }
            if self.service_pending_checkpoint()? {
                dirty = false;
                published = Instant::now();
            }
            // A zero-reply commit can observe cancellation during resize or
            // input's scroll-to-bottom. Reenter close handling before any user
            // write/deadline path; an already closing graceful worker still drains.
            if self.close_deadline.is_none() && self.shared.stop.load(Ordering::Acquire) {
                continue;
            }
            if self
                .pending_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                if let Some(mut completion) = self.pending_completion.take() {
                    completion.finish(Some(InputWriteFailure::Deadline), false);
                }
                self.pending.clear();
                self.pending_deadline = None;
                if self.causal_reply.is_some() {
                    return Err(SessionError::Failure("user input deadline blocked a staged terminal reply; session recovery required".into()));
                }
            }
            if self
                .causal_reply
                .as_ref()
                .is_some_and(|reply| Instant::now() >= reply.deadline)
            {
                return Err(self.reply_failure("deadline"));
            }
            if self
                .deferred_ordering_deadline()
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(SessionError::Failure(
                    "causal PTY ordering deadline; session recovery required".into(),
                ));
            }
            if self.causal_reply.is_some() && self.process.poll_exit()? {
                return Err(self.reply_failure("child exited before complete physical delivery"));
            }
            if !self.pending.is_empty() && !self.shared.stop.load(Ordering::Acquire) {
                match self
                    .io
                    .as_mut()
                    .expect("active PTY I/O")
                    .write(&self.pending[self.written..])
                {
                    Ok(0) => {
                        return Err(SessionError::Failure(
                            "PTY input write returned zero bytes".into(),
                        ));
                    }
                    Ok(count) => {
                        if self.checkpoint_barrier_pending() {
                            self.checkpoint_read_drained = false;
                        }
                        self.written += count;
                        if let Some(completion) = &mut self.pending_completion {
                            completion.written = self.written;
                        }
                        if self.written == self.pending.len() {
                            if let Some(mut completion) = self.pending_completion.take() {
                                completion.finish(None, false);
                            }
                            self.pending.clear();
                            self.pending_deadline = None;
                        }
                    }
                    Err(e) if retryable(&e) => {}
                    Err(e) => return Err(SessionError::Failure(format!("write PTY input: {e}"))),
                }
            }
            dirty |= self.advance_reply()?;
            let mut buffer = [0; IO_CHUNK_BYTES];
            if !self.eof
                && self.causal_reply.is_none()
                && (self.deferred.is_empty() || !self.pending.is_empty())
            {
                for _ in 0..READ_QUANTA {
                    if self.close_deadline.is_none() && self.shared.stop.load(Ordering::Acquire) {
                        break;
                    }
                    let admission_lock = Arc::clone(&self.shared);
                    let _admission = admission_lock.input_admission.lock().map_err(|_| {
                        SessionError::Failure("input admission lock poisoned".into())
                    })?;
                    if self.close_deadline.is_none() && !self.checkpoint_barrier_pending() {
                        self.capture_queued_inputs()?;
                    }
                    match self.io.as_mut().expect("active PTY I/O").read(&mut buffer) {
                        Ok(0) => {
                            self.eof = true;
                            if self.checkpoint_barrier_pending() {
                                self.checkpoint_read_drained = true;
                            }
                            break;
                        }
                        Ok(count) => {
                            if self.checkpoint_barrier_pending() {
                                self.checkpoint_read_drained = false;
                            }
                            self.bytes_read += count as u64;
                            if self.close_deadline.is_none() {
                                self.defer_output(&buffer[..count])?;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            if self.checkpoint_barrier_pending() {
                                self.checkpoint_read_drained = true;
                            }
                            break;
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => break,
                        // A native Unix PTY reports EIO after its slave closes.
                        Err(e) if e.raw_os_error() == Some(Errno::EIO as i32) => {
                            self.eof = true;
                            if self.checkpoint_barrier_pending() {
                                self.checkpoint_read_drained = true;
                            }
                            break;
                        }
                        Err(e) => {
                            return Err(SessionError::Failure(format!("read PTY output: {e}")));
                        }
                    }
                }
            }
            if self.exit_seen.is_none()
                && let Some(status) = child::observe_child(self.process.pid, false)
                    .map_err(|e| SessionError::Failure(format!("observe PTY process: {e}")))?
            {
                self.exit_code = Some(match status {
                    child::ExitObservation::Exited(code) => code as u32,
                    child::ExitObservation::Signaled(_) => 1,
                    _ => {
                        return Err(SessionError::Failure(
                            "unexpected PTY stop observation".into(),
                        ));
                    }
                });
                self.exit_seen = Some(Instant::now());
            }
            if self.service_pending_checkpoint()? {
                dirty = false;
                published = Instant::now();
            }
            if self.exit_seen.is_some() && self.causal_reply.is_some() {
                return Err(self.reply_failure("child exited before publication"));
            }
            if self.exit_seen.is_some() && self.eof && self.deferred.is_empty() {
                return Ok(if self.close_deadline.is_some() {
                    State::Closed
                } else {
                    State::Exited
                });
            }
            if self.exit_seen.is_some_and(|t| t.elapsed() >= DRAIN_TIMEOUT) {
                return Err(SessionError::Failure(
                    "PTY output did not close after process exit; inherited descriptors may remain"
                        .into(),
                ));
            }
            // Graceful close aborts the staged terminal but still drains PTY
            // bytes for deadline and cleanup accounting. Do not refresh it.
            if dirty
                && self.close_deadline.is_none()
                && self.causal_reply.is_none()
                && published.elapsed() >= FRAME_INTERVAL
            {
                self.publish(State::Running, None, true)?;
                dirty = false;
                published = Instant::now();
            } else if self.close_deadline.is_none()
                && self.causal_reply.is_none()
                && self.deferred_ordering_deadline().is_some()
                && published.elapsed() >= FRAME_INTERVAL
            {
                // Keep physical PTY-read progress observable at the existing
                // frame cadence without refreshing or committing the engine.
                self.publish_deferred_read_progress()?;
                published = Instant::now();
            }
            if self.close_deadline.is_none()
                && self.causal_reply.is_none()
                && self.pending.is_empty()
                && !self.deferred.is_empty()
            {
                continue;
            }
            if self.close_deadline.is_none() {
                dirty |= self.load_input()?;
            }
            let mut fds = [
                pollfd {
                    fd: if self.eof {
                        self.wake.as_raw_file_descriptor()
                    } else {
                        self.io
                            .as_ref()
                            .expect("active PTY I/O")
                            .as_raw_file_descriptor()
                    },
                    events: if self.eof || self.causal_reply.is_some() {
                        0
                    } else {
                        POLLIN
                    } | if self.pending.is_empty() && self.causal_reply.is_none() {
                        0
                    } else {
                        POLLOUT
                    },
                    revents: 0,
                },
                pollfd {
                    fd: self.wake.as_raw_file_descriptor(),
                    events: POLLIN,
                    revents: 0,
                },
            ];
            let timeout = if dirty {
                FRAME_INTERVAL.saturating_sub(published.elapsed())
            } else {
                IDLE_WAIT
            };
            let timeout = self.pending_deadline.map_or(timeout, |deadline| {
                timeout.min(deadline.saturating_duration_since(Instant::now()))
            });
            let timeout = self.causal_reply.as_ref().map_or(timeout, |reply| {
                timeout.min(reply.deadline.saturating_duration_since(Instant::now()))
            });
            let timeout = self
                .deferred_ordering_deadline()
                .map_or(timeout, |deadline| {
                    timeout.min(deadline.saturating_duration_since(Instant::now()))
                });
            let timeout = self
                .pending_checkpoint
                .as_ref()
                .map_or(timeout, |checkpoint| {
                    timeout.min(
                        checkpoint
                            .deadline
                            .saturating_duration_since(Instant::now()),
                    )
                });
            let timeout = self
                .close_deadline
                .filter(|_| self.exit_seen.is_none())
                .map_or(timeout, |deadline| {
                    timeout.min(deadline.saturating_duration_since(Instant::now()))
                });
            match poll(&mut fds, Some(timeout)) {
                Ok(_) => {}
                Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(e) => return Err(SessionError::Failure(format!("poll session I/O: {e}"))),
            }
            let mut wake = [0; 128];
            match self.wake.read(&mut wake) {
                Ok(_) => {}
                Err(e) if retryable(&e) => {}
                Err(e) => return Err(SessionError::Failure(format!("read session wake: {e}"))),
            }
        }
    }

    // Keep the parser on its worker after child exit so completed output can be
    // inspected. Poll only the wake socket; a hung-up PTY must not busy-spin.
    fn review_history(&mut self) -> Result<(), SessionError> {
        while !self.shared.stop.load(Ordering::Acquire) {
            let mut dirty = false;
            for _ in 0..INPUT_QUEUE_CHUNKS {
                if self.shared.stop.load(Ordering::Acquire) {
                    break;
                }
                let Ok(input) = self.input.try_recv() else {
                    break;
                };
                match input {
                    Input::Scroll(lines) => {
                        self.history_scroll(lines)?;
                        dirty = true;
                    }
                    Input::Wheel(event) => {
                        // Completed processes cannot consume mouse/application input.
                        self.history_scroll(event.lines)?;
                        dirty = true;
                    }
                    Input::Bytes { mut completion, .. } => {
                        if let Some(completion) = &mut completion {
                            completion.finish(Some(InputWriteFailure::SessionExited), false);
                        }
                    }
                    Input::ProgramStatusCheckpoint {
                        reply,
                        cancelled,
                        deadline,
                    } => {
                        let _ = self.process_program_status_checkpoint(
                            reply,
                            cancelled,
                            deadline,
                            State::Exited,
                        )?;
                    }
                    Input::Key(_) | Input::Paste(_) | Input::Pointer(_) => {}
                }
            }
            if dirty {
                self.publish(State::Exited, None, true)?;
            }
            let mut fds = [pollfd {
                fd: self.wake.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            }];
            match poll(&mut fds, Some(IDLE_WAIT)) {
                Ok(_) => {}
                Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {
                    continue;
                }
                Err(error) => {
                    return Err(SessionError::Failure(format!("poll history wake: {error}")));
                }
            }
            let mut wake = [0; 128];
            match self.wake.read(&mut wake) {
                Ok(_) => {}
                Err(error) if retryable(&error) => {}
                Err(error) => {
                    return Err(SessionError::Failure(format!("read history wake: {error}")));
                }
            }
        }
        Ok(())
    }

    fn load_input(&mut self) -> Result<bool, SessionError> {
        if !self.pending.is_empty()
            || self.causal_reply.is_some()
            || self.shared.stop.load(Ordering::Acquire)
            || self.checkpoint_barrier_pending()
        {
            return Ok(false);
        }
        let Ok(input) = self.input.try_recv() else {
            return Ok(false);
        };
        self.process_input(input)
    }

    fn process_input(&mut self, input: Input) -> Result<bool, SessionError> {
        match input {
            Input::ProgramStatusCheckpoint {
                reply,
                cancelled,
                deadline,
            } => self.schedule_program_status_checkpoint(reply, cancelled, deadline),
            Input::Bytes {
                bytes,
                completion,
                deadline,
            } => {
                if completion
                    .as_ref()
                    .is_some_and(|completion| !completion.start())
                {
                    return Ok(true);
                }
                self.pending = bytes;
                self.written = 0;
                self.pending_completion = completion;
                self.pending_deadline = deadline;
                let batch = self.terminal.scroll(0)?;
                self.accept_batch(batch)
            }
            Input::Scroll(lines) => {
                let batch = self.terminal.scroll(lines)?;
                self.accept_batch(batch)
            }
            Input::Key(event) => {
                self.pending = self.terminal.encode_key(event)?;
                self.written = 0;
                let batch = self.terminal.scroll(0)?;
                self.accept_batch(batch)
            }
            Input::Paste(text) => {
                self.pending = self.terminal.encode_paste(&text)?;
                self.written = 0;
                let batch = self.terminal.scroll(0)?;
                self.accept_batch(batch)
            }
            Input::Pointer(event) => {
                let (bytes, batch) = self.terminal.pointer(event)?;
                self.pending = bytes;
                self.written = 0;
                self.accept_batch(Some(batch))
            }
            Input::Wheel(event) => {
                let (bytes, batch) = self.terminal.wheel(event)?;
                self.pending = bytes;
                self.written = 0;
                self.accept_batch(Some(batch))
            }
        }
    }

    fn process_program_status_checkpoint(
        &mut self,
        reply: SyncSender<Result<Arc<Snapshot>, SessionError>>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        state: State,
    ) -> Result<bool, SessionError> {
        if cancelled.load(Ordering::Acquire) {
            let _ = reply.try_send(Err(SessionError::Cancelled));
            return Ok(false);
        }
        if Instant::now() >= deadline {
            let _ = reply.try_send(Err(SessionError::Timeout));
            return Ok(false);
        }
        if let Err(error) = self.publish(state, None, true) {
            let detail = error.to_string();
            let _ = reply.try_send(Err(SessionError::Failure(detail)));
            return Err(error);
        }
        if cancelled.load(Ordering::Acquire) {
            let _ = reply.try_send(Err(SessionError::Cancelled));
            return Ok(true);
        }
        let snapshot = self
            .shared
            .snapshot
            .lock()
            .map_err(|_| SessionError::Failure("session snapshot lock poisoned".into()))?
            .clone();
        let _ = reply.try_send(Ok(snapshot));
        Ok(true)
    }

    fn schedule_program_status_checkpoint(
        &mut self,
        reply: SyncSender<Result<Arc<Snapshot>, SessionError>>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<bool, SessionError> {
        if cancelled.load(Ordering::Acquire) {
            let _ = reply.try_send(Err(SessionError::Cancelled));
            return Ok(false);
        }
        if Instant::now() >= deadline {
            let _ = reply.try_send(Err(SessionError::Timeout));
            return Ok(false);
        }
        if self.pending_checkpoint.is_some() {
            return Err(SessionError::Failure(
                "overlapping worker status checkpoints; session recovery required".into(),
            ));
        }
        self.pending_checkpoint = Some(PendingProgramStatusCheckpoint {
            reply,
            cancelled,
            deadline,
        });
        self.checkpoint_read_drained = false;
        Ok(false)
    }

    fn service_pending_checkpoint(&mut self) -> Result<bool, SessionError> {
        let Some(checkpoint) = self.pending_checkpoint.as_ref() else {
            return Ok(false);
        };
        if checkpoint.cancelled.load(Ordering::Acquire) {
            let checkpoint = self
                .pending_checkpoint
                .take()
                .expect("pending checkpoint was just observed");
            let _ = checkpoint.reply.try_send(Err(SessionError::Cancelled));
            self.checkpoint_read_drained = false;
            return Ok(false);
        }
        if Instant::now() >= checkpoint.deadline {
            let checkpoint = self
                .pending_checkpoint
                .take()
                .expect("pending checkpoint was just observed");
            let _ = checkpoint.reply.try_send(Err(SessionError::Timeout));
            self.checkpoint_read_drained = false;
            return Ok(false);
        }
        if (!self.checkpoint_read_drained && !self.eof)
            || !self.deferred.is_empty()
            || !self.pending.is_empty()
            || self.causal_reply.is_some()
        {
            return Ok(false);
        }
        let checkpoint = self
            .pending_checkpoint
            .take()
            .expect("pending checkpoint was just observed");
        self.checkpoint_read_drained = false;
        self.process_program_status_checkpoint(
            checkpoint.reply,
            checkpoint.cancelled,
            checkpoint.deadline,
            // The run loop still owns final drain. Only execute() publishes
            // Exited after committing attached-child-exit status retirement.
            State::Running,
        )
    }

    fn checkpoint_barrier_pending(&self) -> bool {
        self.pending_checkpoint.is_some()
            || self.deferred.iter().any(|action| {
                matches!(
                    action,
                    DeferredAction::Input(Input::ProgramStatusCheckpoint { .. })
                )
            })
    }

    /// Snapshot all input admitted before one PTY read. The caller holds the
    /// admission lock through that read, so later input remains behind its
    /// resulting output in this ordered, bounded queue.
    fn capture_queued_inputs(&mut self) -> Result<(), SessionError> {
        loop {
            let input = match self.input.try_recv() {
                Ok(input) => input,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
            };
            let checkpoint = matches!(&input, Input::ProgramStatusCheckpoint { .. });
            if self.deferred_input_events >= MAX_DEFERRED_INPUT_EVENTS
                || self.deferred.len() >= MAX_DEFERRED_ACTIONS
                || self.deferred.try_reserve(1).is_err()
            {
                finish_input(input, InputWriteFailure::WorkerFailed);
                return Err(SessionError::Failure(
                    "deferred PTY input queue overflow; session recovery required".into(),
                ));
            }
            self.deferred.push_back(DeferredAction::Input(input));
            self.deferred_input_events += 1;
            if checkpoint {
                return Ok(());
            }
        }
    }

    fn defer_output(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
        if bytes.len() > MAX_DEFERRED_OUTPUT_BYTES.saturating_sub(self.deferred_output_bytes) {
            return Err(SessionError::Failure(
                "deferred PTY output exceeded its bounded ordering buffer; session recovery required"
                    .into(),
            ));
        }
        let ordering_deadline = if !self.pending.is_empty()
            || self
                .deferred
                .iter()
                .any(|action| matches!(action, DeferredAction::Input(_)))
        {
            Some(
                Instant::now()
                    + self.engine_reply_timeout.ok_or_else(|| {
                        SessionError::Failure("missing explicit engine reply lifetime".into())
                    })?,
            )
        } else {
            None
        };
        match self.deferred.back_mut() {
            Some(DeferredAction::Output { bytes: queued, .. }) => {
                queued.try_reserve(bytes.len()).map_err(|_| {
                    SessionError::Failure(
                        "cannot reserve deferred PTY output; session recovery required".into(),
                    )
                })?;
                queued.extend_from_slice(bytes);
            }
            _ => {
                if self.deferred.len() >= MAX_DEFERRED_ACTIONS {
                    return Err(SessionError::Failure(
                        "deferred PTY action queue overflow; session recovery required".into(),
                    ));
                }
                let mut queued = Vec::new();
                queued.try_reserve_exact(bytes.len()).map_err(|_| {
                    SessionError::Failure(
                        "cannot reserve deferred PTY output; session recovery required".into(),
                    )
                })?;
                queued.extend_from_slice(bytes);
                self.deferred.push_back(DeferredAction::Output {
                    bytes: queued,
                    offset: 0,
                    ordering_deadline,
                });
            }
        }
        self.deferred_output_bytes += bytes.len();
        Ok(())
    }

    fn advance_deferred(&mut self) -> Result<bool, SessionError> {
        match self.deferred.front() {
            Some(DeferredAction::Input(_)) => {
                let Some(DeferredAction::Input(input)) = self.deferred.pop_front() else {
                    unreachable!("deferred input was just observed")
                };
                self.deferred_input_events =
                    self.deferred_input_events.checked_sub(1).ok_or_else(|| {
                        SessionError::Failure("deferred input count underflow".into())
                    })?;
                if self.exit_seen.is_some() {
                    match input {
                        Input::Scroll(lines) => self.history_scroll(lines)?,
                        Input::Wheel(event) => self.history_scroll(event.lines)?,
                        checkpoint @ Input::ProgramStatusCheckpoint { .. } => {
                            return self.process_input(checkpoint);
                        }
                        other => finish_input(other, InputWriteFailure::SessionExited),
                    }
                    Ok(true)
                } else {
                    self.process_input(input)
                }
            }
            Some(DeferredAction::Output { .. }) => {
                if self.checkpoint_barrier_pending() && !self.eof {
                    self.checkpoint_read_drained = false;
                }
                let mut chunk = [0; IO_CHUNK_BYTES];
                let (length, finished) = match self.deferred.front_mut() {
                    Some(DeferredAction::Output {
                        bytes,
                        offset,
                        ordering_deadline,
                    }) => {
                        let length = (bytes.len() - *offset).min(IO_CHUNK_BYTES);
                        chunk[..length].copy_from_slice(&bytes[*offset..*offset + length]);
                        *offset += length;
                        *ordering_deadline = None;
                        (length, *offset == bytes.len())
                    }
                    _ => unreachable!("deferred output was just observed"),
                };
                self.deferred_output_bytes = self
                    .deferred_output_bytes
                    .checked_sub(length)
                    .ok_or_else(|| {
                        SessionError::Failure("deferred output count underflow".into())
                    })?;
                if finished {
                    self.deferred.pop_front();
                }
                let batch = self.terminal.process(&chunk[..length])?;
                self.accept_batch(batch)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn deferred_ordering_deadline(&self) -> Option<Instant> {
        self.deferred.iter().find_map(|action| match action {
            DeferredAction::Input(_) => None,
            DeferredAction::Output {
                ordering_deadline, ..
            } => *ordering_deadline,
        })
    }

    fn accept_batch(&mut self, batch: Option<EngineBatch>) -> Result<bool, SessionError> {
        let Some(batch) = batch else {
            self.collect_open_requests()?;
            return Ok(true);
        };
        if batch.replies().is_empty() {
            return self.commit_batch(&batch);
        }
        let timeout = self.engine_reply_timeout.ok_or_else(|| {
            SessionError::Failure("missing explicit engine reply lifetime".into())
        })?;
        self.causal_reply = Some(CausalReply {
            batch,
            written: 0,
            deadline: Instant::now() + timeout,
        });
        Ok(false)
    }

    fn advance_reply(&mut self) -> Result<bool, SessionError> {
        if !self.pending.is_empty() || self.shared.stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        let Some(reply) = self.causal_reply.as_mut() else {
            return Ok(false);
        };
        if Instant::now() >= reply.deadline {
            return Err(self.reply_failure("deadline"));
        }
        match self
            .io
            .as_mut()
            .expect("active PTY I/O")
            .write(&reply.batch.replies()[reply.written..])
        {
            Ok(0) => return Err(self.reply_failure("zero-byte write")),
            Ok(count) => reply.written += count,
            Err(error) if retryable(&error) => return Ok(false),
            Err(error) => return Err(self.reply_failure(&format!("physical write: {error}"))),
        }
        if reply.written != reply.batch.replies().len() {
            return Ok(false);
        }
        if self.shared.stop.load(Ordering::Acquire) {
            return Ok(false);
        }
        if Instant::now() >= reply.deadline {
            return Err(self.reply_failure("deadline after physical write"));
        }
        let reply = self
            .causal_reply
            .take()
            .expect("validated outstanding reply");
        self.commit_batch(&reply.batch)
    }

    // All worker commit entrypoints share the observed-cancellation fence,
    // including zero-reply resize/scroll/output and completed-history batches.
    fn commit_batch(&mut self, batch: &EngineBatch) -> Result<bool, SessionError> {
        if self.shared.stop.load(Ordering::Acquire) {
            self.abort_engine();
            return Ok(false);
        }
        self.terminal.commit(batch)?;
        self.collect_open_requests()?;
        Ok(true)
    }

    /// Move only requests belonging to this live PTY into its URL-only drain.
    /// Parsing/queue failures and either bounded queue overflowing fail closed.
    fn collect_open_requests(&mut self) -> Result<(), SessionError> {
        let requests = match self.terminal.take_open_requests() {
            Ok(requests) => requests,
            Err(_) => {
                if let Ok(mut queue) = self.shared.open_urls.lock() {
                    queue.clear();
                }
                return Err(SessionError::Failure(
                    "terminal open-request parser overflow or failure; session recovery required"
                        .into(),
                ));
            }
        };
        let Some(registration) = self.open_request_registration else {
            return Ok(());
        };
        let mut accepted = Vec::new();
        accepted.try_reserve_exact(requests.len()).map_err(|_| {
            SessionError::Failure("cannot reserve bounded open-request batch".into())
        })?;
        for request in requests {
            if registration.nonce().matches_request(&request) {
                let source = request.url();
                let mut url = String::new();
                url.try_reserve_exact(source.len()).map_err(|_| {
                    SessionError::Failure("cannot reserve bounded open-request URL".into())
                })?;
                url.push_str(source);
                accepted.push(OpenUrl::new(url));
            }
        }
        if accepted.is_empty() {
            return Ok(());
        }
        let mut queue = self
            .shared
            .open_urls
            .lock()
            .map_err(|_| SessionError::Failure("open-request queue poisoned".into()))?;
        if queue.len().saturating_add(accepted.len()) > MAX_PROGRAM_OPEN_REQUESTS {
            queue.clear();
            return Err(SessionError::Failure(
                "session open-request queue overflow; session recovery required".into(),
            ));
        }
        if queue.try_reserve(accepted.len()).is_err() {
            queue.clear();
            return Err(SessionError::Failure(
                "cannot reserve bounded session open-request queue".into(),
            ));
        }
        queue.extend(accepted);
        Ok(())
    }

    fn reply_failure(&self, reason: &str) -> SessionError {
        let (written, total) = self
            .causal_reply
            .as_ref()
            .map_or((0, 0), |reply| (reply.written, reply.batch.replies().len()));
        SessionError::Failure(format!(
            "terminal causal reply {reason}: {written}/{total} bytes physically accepted; unpublished frame discarded, explicit session recovery required"
        ))
    }

    fn abort_engine(&mut self) {
        self.causal_reply = None;
        self.terminal.abort();
    }

    fn history_scroll(&mut self, lines: i32) -> Result<(), SessionError> {
        if let Some(batch) = self.terminal.scroll(lines)? {
            if !batch.replies().is_empty() {
                self.terminal.abort();
                return Err(SessionError::Failure(
                    "completed history produced a terminal reply without a live PTY".into(),
                ));
            }
            self.commit_batch(&batch)?;
        }
        Ok(())
    }

    fn finish_inputs(&mut self, reason: InputWriteFailure) {
        if let Some(mut completion) = self.pending_completion.take() {
            completion.finish(Some(reason), false);
        }
        self.pending.clear();
        self.pending_deadline = None;
        for action in self.deferred.drain(..) {
            if let DeferredAction::Input(input) = action {
                finish_input(input, reason);
            }
        }
        self.deferred_input_events = 0;
        self.deferred_output_bytes = 0;
        if let Some(checkpoint) = self.pending_checkpoint.take()
            && !checkpoint.cancelled.load(Ordering::Acquire)
        {
            let _ = checkpoint.reply.try_send(Err(SessionError::Closed));
        }
        self.checkpoint_read_drained = false;
        for input in self.input.try_iter() {
            finish_input(input, reason);
        }
    }

    fn publish(
        &mut self,
        state: State,
        failure: Option<String>,
        refresh_viewport: bool,
    ) -> Result<(), SessionError> {
        if refresh_viewport {
            self.terminal.refresh_viewport()?;
        }
        self.publish_viewport(state, failure, self.terminal.viewport())
    }

    fn publish_viewport(
        &mut self,
        state: State,
        failure: Option<String>,
        viewport: Viewport,
    ) -> Result<(), SessionError> {
        let snapshot = Arc::new(Snapshot {
            state,
            viewport,
            bytes_read: self.bytes_read,
            exit_code: self.exit_code,
            failure,
        });
        match self.shared.snapshot.lock() {
            Ok(mut slot) => *slot = snapshot,
            Err(_) => {
                return Err(SessionError::Failure(
                    "session snapshot publication lock poisoned".into(),
                ));
            }
        }
        Ok(())
    }

    fn publish_deferred_read_progress(&mut self) -> Result<(), SessionError> {
        let mut slot = self
            .shared
            .snapshot
            .lock()
            .map_err(|_| SessionError::Failure("session snapshot lock poisoned".into()))?;
        if self.bytes_read <= slot.bytes_read {
            return Ok(());
        }
        let committed = &**slot;
        *slot = Arc::new(Snapshot {
            state: committed.state,
            viewport: committed.viewport.clone(),
            bytes_read: self.bytes_read,
            exit_code: committed.exit_code,
            failure: committed.failure.clone(),
        });
        Ok(())
    }
}

fn lock_error<T>(error: TryLockError<T>) -> SessionError {
    match error {
        TryLockError::WouldBlock => SessionError::Busy,
        TryLockError::Poisoned(_) => {
            SessionError::Failure("session state lock poisoned; recovery required".into())
        }
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        os::fd::AsRawFd,
        os::unix::{fs::DirBuilderExt, net::UnixListener},
        path::PathBuf,
    };

    const FINAL_DRAIN_OUTPUT: &[u8] =
        b"\x1b]133;A\x07B_PROMPT_READY\r\n\x1b]7501;state=working:id=final\x07";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FinalDrainFixtureCommand {
        CaptureOutput,
        LoadCheckpoint,
        Execute,
    }

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty() {
        checkpoint_with_preexisting_output(false);
    }

    #[test]
    fn checkpoint_during_final_drain_keeps_pre_retirement_state() {
        checkpoint_with_preexisting_output(true);
    }

    fn checkpoint_with_preexisting_output(child_exits: bool) {
        let directory = TestDirectory(
            // Darwin's per-user TMPDIR can exceed sockaddr_un's path limit.
            PathBuf::from("/tmp")
                .canonicalize()
                .expect("resolve short fixture root")
                .join(format!("zmux-checkpoint-order-{}", uuid::Uuid::new_v4())),
        );
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory.0)
            .expect("create checkpoint fixture directory");
        let socket_path = directory.0.join("child-ready.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind child readiness socket");
        let script_path = directory.0.join("child.py");
        fs::write(
            &script_path,
            r#"import os,socket,sys,tty
tty.setraw(0)
payload=b'\x1b]133;A\x07B_PROMPT_READY\r\n\x1b]7501;state=working:id=final\x07'
while payload:
    payload=payload[os.write(1,payload):]
notice=socket.socket(socket.AF_UNIX)
notice.connect(sys.argv[1])
notice.sendall(b'wrote')
if sys.argv[2]=='exit':
    notice.recv(4)
    os._exit(0)
notice.close()
os.read(0,1)
"#,
        )
        .expect("write checkpoint child script");

        let (input, input_receiver) = mpsc::sync_channel(INPUT_QUEUE_CHUNKS);
        let launch = Launch {
            program: "python3".into(),
            args: vec![
                "-u".into(),
                script_path.to_string_lossy().into_owned(),
                socket_path.to_string_lossy().into_owned(),
                if child_exits { "exit" } else { "live" }.into(),
            ],
            cwd: None,
            size: default_size(),
        };
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let (fixture_command, fixture_commands) = mpsc::sync_channel(3);
        let (captured_output_sender, captured_output_receiver) = mpsc::sync_channel(1);
        let (checkpoint_loaded_sender, checkpoint_loaded_receiver) = mpsc::sync_channel(1);
        let worker_thread = thread::spawn(move || {
            let (mut worker, startup) = Worker::start(
                launch,
                Vec::new(),
                Vec::new(),
                ShutdownPolicy::Kill,
                None,
                None,
                input_receiver,
            )
            .expect("start real PTY worker");
            startup_sender.send(startup).expect("return worker startup");
            if child_exits {
                assert_eq!(
                    fixture_commands.recv().expect("request PTY output capture"),
                    FinalDrainFixtureCommand::CaptureOutput
                );
                let captured = capture_preexisting_pty_output(&mut worker, FINAL_DRAIN_OUTPUT);
                captured_output_sender
                    .send(captured)
                    .expect("return captured PTY output");
                assert_eq!(
                    fixture_commands
                        .recv()
                        .expect("request checkpoint scheduling"),
                    FinalDrainFixtureCommand::LoadCheckpoint
                );
                worker
                    .load_input()
                    .expect("load admitted checkpoint before worker execution");
                checkpoint_loaded_sender
                    .send(())
                    .expect("return checkpoint scheduling result");
            }
            assert_eq!(
                fixture_commands.recv().expect("release session worker"),
                FinalDrainFixtureCommand::Execute
            );
            worker.execute();
        });
        let startup = startup_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("worker did not complete PTY startup");

        let mut notification_fds = [pollfd {
            fd: listener.as_raw_fd(),
            events: POLLIN,
            revents: 0,
        }];
        assert!(matches!(
            poll(&mut notification_fds, Some(Duration::from_secs(5))),
            Ok(1)
        ));
        let (mut child_notice, _) = listener.accept().expect("accept child readiness notice");
        let mut notice = [0; 5];
        child_notice
            .read_exact(&mut notice)
            .expect("read child readiness notice");
        assert_eq!(&notice, b"wrote");
        let process_id = startup.process_id;
        let session_epoch = startup
            .shared
            .snapshot
            .lock()
            .expect("read initial Session snapshot")
            .viewport
            .program_status
            .session_epoch;
        if child_exits {
            fixture_command
                .send(FinalDrainFixtureCommand::CaptureOutput)
                .expect("capture child output before attached-child exit");
            let captured = captured_output_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("worker did not capture the pre-exit PTY output");
            assert_eq!(captured, FINAL_DRAIN_OUTPUT);
            assert!(
                child::observe_child(process_id, false)
                    .expect("observe live PTY child")
                    .is_none()
            );
            child_notice
                .write_all(b"exit")
                .expect("allow retained PTY child to exit");
            let deadline = Instant::now() + Duration::from_secs(5);
            let exit = loop {
                if let Some(exit) =
                    child::observe_child(process_id, false).expect("observe retained PTY child")
                {
                    break exit;
                }
                assert!(Instant::now() < deadline, "PTY child did not exit");
                thread::yield_now();
            };
            assert_eq!(exit, child::ExitObservation::Exited(0));
        }

        // Keep the public Session live while a checkpoint waits behind real PTY
        // bytes and, for final drain, an already exited attached child.
        let mut session = Session {
            input,
            wake: Mutex::new(startup.wake),
            shared: startup.shared,
            worker: Some(worker_thread),
            process_id,
            shutdown_policy: ShutdownPolicy::Kill,
            application_wheel_supported: true,
            open_request_registration: None,
        };
        let mut checkpoint = session
            .request_program_status_checkpoint(Instant::now() + Duration::from_secs(5))
            .expect("admit public checkpoint request");
        if child_exits {
            fixture_command
                .send(FinalDrainFixtureCommand::LoadCheckpoint)
                .expect("schedule checkpoint after attached-child exit");
            checkpoint_loaded_receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("worker did not admit the pending checkpoint");
            assert!(
                checkpoint.try_recv().is_none(),
                "checkpoint was serviced before the held worker executed"
            );
        }
        fixture_command
            .send(FinalDrainFixtureCommand::Execute)
            .expect("start session worker");

        let snapshot = checkpoint
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("checkpoint did not complete before its test deadline")
            .expect("checkpoint failed");
        assert!(snapshot.bytes_read > 0);
        assert!(snapshot.viewport.contents.contains("B_PROMPT_READY"));
        assert_eq!(
            snapshot.viewport.program_status.session_epoch,
            session_epoch
        );
        assert_eq!(snapshot.viewport.program_status.command_epoch.0, 1);
        assert_eq!(snapshot.state, State::Running);
        assert!(
            snapshot
                .viewport
                .program_status
                .records
                .iter()
                .any(|record| {
                    record.id.as_ref() == "final"
                        && record.state == zmux_program_status::State::Working
                })
        );
        assert!(
            snapshot
                .viewport
                .program_status
                .transitions
                .iter()
                .any(|event| {
                    event.id.as_ref() == "final"
                        && event.cause == zmux_program_status::TransitionCause::Report
                        && event.command_epoch.0 == 1
                        && event.after_record.as_ref().is_some_and(|record| {
                            record.state == zmux_program_status::State::Working
                        })
                }),
            "checkpoint did not retain the parsed working transition"
        );
        if child_exits {
            assert_eq!(snapshot.bytes_read, FINAL_DRAIN_OUTPUT.len() as u64);
            let deadline = Instant::now() + Duration::from_secs(5);
            let final_snapshot = loop {
                match session.snapshot() {
                    Ok(current) if current.state == State::Exited => break current,
                    Ok(_) | Err(SessionError::Busy) => {}
                    Err(error) => panic!("read final Session snapshot: {error}"),
                }
                assert!(
                    Instant::now() < deadline,
                    "final drain did not retire child"
                );
                thread::yield_now();
            };
            assert_eq!(final_snapshot.exit_code, Some(0));
            assert_eq!(final_snapshot.bytes_read, FINAL_DRAIN_OUTPUT.len() as u64);
            assert_eq!(
                final_snapshot.viewport.program_status.session_epoch,
                session_epoch
            );
            assert_eq!(final_snapshot.viewport.program_status.command_epoch.0, 1);
            assert!(final_snapshot.viewport.program_status.records.is_empty());
            assert!(
                final_snapshot
                    .viewport
                    .program_status
                    .transitions
                    .iter()
                    .any(|event| {
                        event.id.as_ref() == "final"
                            && event.cause == zmux_program_status::TransitionCause::ChildExit
                            && event.command_epoch.0 == 1
                    })
            );
        }

        session
            .shutdown(Duration::from_secs(3))
            .expect("stop and join checkpoint worker");
    }

    fn capture_preexisting_pty_output(worker: &mut Worker, expected: &[u8]) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut captured = Vec::with_capacity(expected.len());
        while captured.len() < expected.len() {
            let remaining = expected.len() - captured.len();
            let mut ready = [pollfd {
                fd: worker
                    .io
                    .as_ref()
                    .expect("active PTY I/O")
                    .as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            }];
            assert_eq!(
                poll(
                    &mut ready,
                    Some(deadline.saturating_duration_since(Instant::now()))
                )
                .expect("wait for child PTY output"),
                1,
                "child did not produce its complete PTY payload before the fixture deadline"
            );
            let mut buffer = vec![0; remaining.min(IO_CHUNK_BYTES)];
            let count = worker
                .io
                .as_mut()
                .expect("active PTY I/O")
                .read(&mut buffer)
                .expect("read child PTY output before attached-child exit");
            assert_ne!(count, 0, "PTY closed before the child exited");
            captured.extend_from_slice(&buffer[..count]);
            worker.bytes_read += count as u64;
            worker
                .defer_output(&buffer[..count])
                .expect("retain real PTY output in the worker ordering buffer");
        }
        captured
    }
}
