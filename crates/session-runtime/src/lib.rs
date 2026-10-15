//! Unix-only PTY feasibility runtime. Callers enqueue work; a worker owns process I/O.
//! Drop requests cancellation; explicit shutdown waits only on a non-UI caller.

#[cfg(not(unix))]
compile_error!("The initial Zmux PTY prototype supports Linux and macOS only");

pub mod child;
mod terminal;

use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, POLLOUT, poll, pollfd};
use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    io::{self, Read, Write},
    os::fd::{AsRawFd, RawFd},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use terminal::TerminalOwner;
use thiserror::Error;
#[cfg(feature = "ghostty")]
pub use zmux_terminal_adapter::GhosttyLimits;
use zmux_terminal_adapter::{EngineBatch, Size, Viewport, Wheel};

pub const INPUT_CHUNK_BYTES: usize = 4_096;
pub const INPUT_QUEUE_CHUNKS: usize = 16;

pub fn default_size() -> Size {
    Size { rows: 24, cols: 80 }
}
const IO_CHUNK_BYTES: usize = 4_096;
const READ_QUANTA: usize = 4;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_WAIT: Duration = Duration::from_millis(250);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("{0}")]
    Failure(String),
    #[error("session input queue is full; retry explicitly after it drains")]
    QueueFull,
    #[error("input must contain 1–{INPUT_CHUNK_BYTES} bytes")]
    InputSize,
    #[error("session is closed")]
    Closed,
    #[error("session state is being published; retry without blocking")]
    Busy,
    #[error("session shutdown exceeded its deadline")]
    Timeout,
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
    /// The cancellation loop ended; physical cleanup requires a completed join
    /// or is_finished. Exit can be nonzero; this is not a semantic detach receipt.
    Closed,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub state: State,
    pub viewport: Viewport,
    pub bytes_read: u64,
    pub exit_code: Option<u32>,
    pub failure: Option<String>,
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
    Scroll(i32),
    Wheel(Wheel),
}

struct Shared {
    stop: AtomicBool,
    close_deadline: Mutex<Option<Instant>>,
    exited: AtomicBool,
    resize: Mutex<Option<Size>>,
    snapshot: Mutex<Arc<Snapshot>>,
}

pub struct Session {
    input: SyncSender<Input>,
    wake: Mutex<UnixStream>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    process_id: u32,
    shutdown_policy: ShutdownPolicy,
    application_wheel_supported: bool,
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
                let status = child::observe_child(self.pid, true);
                Err(SessionError::Failure(format!(
                    "terminate PTY process group: {error}; signal={signal:?}; owned_child_pid={}; observed_group={group:?}; owned_child_status={status:?}",
                    self.pid
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
            resource,
        )
    }

    fn spawn_configured(
        launch: Launch,
        environment: &[(String, String)],
        removals: &[std::ffi::OsString],
        shutdown_policy: ShutdownPolicy,
        engine: Option<EngineOptions>,
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

    /// Lets an owning coordinator reap completed workers without blocking its command loop.
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn send_input(&self, bytes: &[u8]) -> Result<(), SessionError> {
        if bytes.is_empty() || bytes.len() > INPUT_CHUNK_BYTES {
            return Err(SessionError::InputSize);
        }
        self.ensure_running()?;
        self.enqueue_bytes(bytes, None, None)
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
        self.enqueue_navigation(Input::Scroll(lines.clamp(-1000, 1000)))
    }
    pub fn wheel(&self, event: Wheel) -> Result<(), SessionError> {
        if !event.history && !self.application_wheel_supported {
            return Err(SessionError::Failure("Ghostty application wheel encoding is not yet supported; explicit history navigation is available".into()));
        }
        self.enqueue_navigation(Input::Wheel(event))
    }
    fn enqueue_navigation(&self, event: Input) -> Result<(), SessionError> {
        self.ensure_available()?;
        self.input.try_send(event).map_err(|e| match e {
            TrySendError::Full(_) => SessionError::QueueFull,
            TrySendError::Disconnected(_) => SessionError::Closed,
        })?;
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

    /// This is a nonblocking cancellation request, safe for an eventual native view teardown.
    pub fn request_close(&self) -> Result<(), SessionError> {
        self.shared.stop.store(true, Ordering::Release);
        self.wake()
    }

    /// Tighten cancellation to a shared shutdown deadline without renewing it.
    /// The existing post-exit PTY drain is reserved within that deadline. A final
    /// OS reap can still remain pending; only join/is_finished confirms cleanup.
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
    shared: Arc<Shared>,
    bytes_read: u64,
    exit_code: Option<u32>,
    exit_seen: Option<Instant>,
    eof: bool,
    pending: Vec<u8>,
    written: usize,
    pending_completion: Option<WriteCompletion>,
    pending_deadline: Option<Instant>,
    shutdown_policy: ShutdownPolicy,
    close_deadline: Option<Instant>,
    engine_reply_timeout: Option<Duration>,
    causal_reply: Option<CausalReply>,
}

struct CausalReply {
    batch: EngineBatch,
    written: usize,
    deadline: Instant,
}

impl Worker {
    fn start(
        launch: Launch,
        environment: Vec<(String, String)>,
        removals: Vec<std::ffi::OsString>,
        shutdown_policy: ShutdownPolicy,
        engine: Option<EngineOptions>,
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
        let terminal = TerminalOwner::new(launch.size, engine)?;
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
        let process = ProcessGuard {
            child,
            reaped: false,
            signaled: false,
            pid,
        };
        drop(pair.slave);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            close_deadline: Mutex::new(None),
            exited: AtomicBool::new(false),
            resize: Mutex::new(None),
            snapshot: Mutex::new(Arc::new(Snapshot {
                state: State::Running,
                viewport,
                bytes_read: 0,
                exit_code: None,
                failure: None,
            })),
        });
        let worker = Self {
            master: Some(pair.master),
            io: Some(io),
            wake: wake_rx,
            process,
            terminal,
            input: input_rx,
            shared: Arc::clone(&shared),
            bytes_read: 0,
            exit_code: None,
            exit_seen: None,
            eof: false,
            pending: Vec::new(),
            written: 0,
            pending_completion: None,
            pending_deadline: None,
            shutdown_policy,
            close_deadline: None,
            engine_reply_timeout: engine.map(|options| options.reply_timeout),
            causal_reply: None,
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
            let state = self.run()?;
            if state == State::Exited {
                self.finish_inputs(InputWriteFailure::SessionExited);
                // History retains only terminal state. Retire transport and the
                // physically owned child before publishing completed history.
                drop(self.io.take());
                drop(self.master.take());
                self.process.stop()?;
                self.shared.exited.store(true, Ordering::Release);
                self.publish(state, None);
                self.review_history()?;
            }
            Ok::<State, SessionError>(state)
        }));
        self.shared.stop.store(true, Ordering::Release);
        match result {
            Ok(Ok(state)) => {
                self.finish_inputs(InputWriteFailure::SessionClosed);
                self.publish(state, None);
            }
            Ok(Err(error)) => {
                self.abort_engine();
                self.finish_inputs(InputWriteFailure::WorkerFailed);
                self.publish(State::Failed, Some(error.to_string()));
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
            if self.causal_reply.is_none() && self.close_deadline.is_none() {
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
            if !self.eof && self.causal_reply.is_none() {
                for _ in 0..READ_QUANTA {
                    if self.close_deadline.is_none() && self.shared.stop.load(Ordering::Acquire) {
                        break;
                    }
                    match self.io.as_mut().expect("active PTY I/O").read(&mut buffer) {
                        Ok(0) => {
                            self.eof = true;
                            break;
                        }
                        Ok(count) => {
                            self.bytes_read += count as u64;
                            if self.close_deadline.is_none() {
                                let batch = self.terminal.process(&buffer[..count])?;
                                self.accept_batch(batch)?;
                                // Publish read progress using only the last committed viewport.
                                dirty = true;
                                if self.causal_reply.is_some()
                                    || self.shared.stop.load(Ordering::Acquire)
                                {
                                    break;
                                }
                            }
                        }
                        Err(e) if retryable(&e) => break,
                        // A native Unix PTY reports EIO after its slave closes.
                        Err(e) if e.raw_os_error() == Some(Errno::EIO as i32) => {
                            self.eof = true;
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
            if self.exit_seen.is_some() && self.causal_reply.is_some() {
                return Err(self.reply_failure("child exited before publication"));
            }
            if self.exit_seen.is_some() && self.eof {
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
            if dirty && published.elapsed() >= FRAME_INTERVAL {
                self.publish(State::Running, None);
                dirty = false;
                published = Instant::now();
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
                }
            }
            if dirty {
                self.publish(State::Exited, None);
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
        {
            return Ok(false);
        }
        for _ in 0..INPUT_QUEUE_CHUNKS {
            let Ok(input) = self.input.try_recv() else {
                break;
            };
            match input {
                Input::Bytes {
                    bytes,
                    completion,
                    deadline,
                } => {
                    if completion
                        .as_ref()
                        .is_some_and(|completion| !completion.start())
                    {
                        continue;
                    }
                    self.pending = bytes;
                    self.written = 0;
                    self.pending_completion = completion;
                    self.pending_deadline = deadline;
                    let batch = self.terminal.scroll(0)?;
                    self.accept_batch(batch)?;
                }
                Input::Scroll(lines) => {
                    let batch = self.terminal.scroll(lines)?;
                    self.accept_batch(batch)?;
                }
                Input::Wheel(event) => {
                    let (bytes, batch) = self.terminal.wheel(event)?;
                    self.pending = bytes;
                    self.written = 0;
                    self.accept_batch(batch)?;
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn accept_batch(&mut self, batch: Option<EngineBatch>) -> Result<bool, SessionError> {
        let Some(batch) = batch else {
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
        Ok(true)
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
        for input in self.input.try_iter() {
            if let Input::Bytes { mut completion, .. } = input
                && let Some(completion) = &mut completion
            {
                completion.finish(Some(reason), false);
            }
        }
    }

    fn publish(&self, state: State, failure: Option<String>) {
        let snapshot = Arc::new(Snapshot {
            state,
            viewport: self.terminal.viewport(),
            bytes_read: self.bytes_read,
            exit_code: self.exit_code,
            failure,
        });
        match self.shared.snapshot.lock() {
            Ok(mut slot) => *slot = snapshot,
            Err(error) => eprintln!("zmux snapshot publication failed: {error}"),
        }
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
