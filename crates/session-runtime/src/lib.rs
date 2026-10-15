//! Unix-only PTY feasibility runtime. Callers enqueue work; a worker owns process I/O.
//! Drop requests cancellation; explicit shutdown waits only on a non-UI caller.

#[cfg(not(unix))]
compile_error!("The initial Zmux PTY prototype supports Linux and macOS only");

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
use thiserror::Error;
use zmux_terminal_adapter::{Size, Terminal, Viewport, Wheel};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Exited,
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
        if self
            .child
            .try_wait()
            .map_err(|e| SessionError::Failure(format!("poll canceled PTY process: {e}")))?
            .is_some()
        {
            self.reaped = true;
        }
        Ok(self.reaped)
    }
    fn signal(&mut self) -> Result<(), SessionError> {
        if self.signaled || self.poll_exit()? {
            return Ok(());
        }
        // The leader remains unreaped, so its PID cannot be reused. Signal the group once.
        match killpg(Pid::from_raw(self.pid as i32), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => {
                self.signaled = true;
                Ok(())
            }
            Err(Errno::EPERM) if self.poll_exit()? => Ok(()),
            Err(error) => Err(SessionError::Failure(format!(
                "terminate PTY process group: {error}"
            ))),
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
    /// Its final release happens on the session worker, never the UI/core executor.
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
        let terminal =
            Terminal::new(launch.size).map_err(|e| SessionError::Failure(e.to_string()))?;
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
        for name in removals {
            command.env_remove(name);
        }
        for (name, value) in environment {
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
            exited: AtomicBool::new(false),
            resize: Mutex::new(None),
            snapshot: Mutex::new(Arc::new(Snapshot {
                state: State::Running,
                viewport: terminal.viewport(),
                bytes_read: 0,
                exit_code: None,
                failure: None,
            })),
        });
        let (input_tx, input_rx) = mpsc::sync_channel(INPUT_QUEUE_CHUNKS);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name(format!("zmux-session-{pid}"))
            .spawn(move || {
                let _resource = resource;
                let mut worker = Worker {
                    master: pair.master,
                    io,
                    wake: wake_rx,
                    process,
                    terminal,
                    input: input_rx,
                    shared: worker_shared,
                    bytes_read: 0,
                    exit_code: None,
                    exit_seen: None,
                    eof: false,
                    pending: Vec::new(),
                    written: 0,
                    pending_completion: None,
                    pending_deadline: None,
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let state = worker.run()?;
                    if state == State::Exited {
                        worker.finish_inputs(InputWriteFailure::SessionExited);
                        worker.shared.exited.store(true, Ordering::Release);
                        worker.publish(state, None);
                        worker.review_history()?;
                    }
                    Ok::<State, SessionError>(state)
                }));
                worker.shared.stop.store(true, Ordering::Release);
                match result {
                    Ok(Ok(state)) => {
                        worker.finish_inputs(InputWriteFailure::SessionClosed);
                        worker.publish(state, None);
                    }
                    Ok(Err(error)) => {
                        worker.finish_inputs(InputWriteFailure::WorkerFailed);
                        worker.publish(State::Failed, Some(error.to_string()));
                    }
                    Err(_) => {
                        // Dropping guards reports queued input as not written
                        // and active input as unknown before process cleanup.
                        drop(worker.pending_completion.take());
                        for input in worker.input.try_iter() {
                            drop(input);
                        }
                        // Do not enter a possibly corrupted parser after a worker panic.
                        match worker.shared.snapshot.lock() {
                            Ok(mut slot) => {
                                let mut last = (**slot).clone();
                                last.state = State::Failed;
                                last.failure = Some(
                                    "session worker panicked; session recovery required".into(),
                                );
                                *slot = Arc::new(last);
                            }
                            Err(error) => eprintln!("zmux panic publication failed: {error}"),
                        }
                    }
                }
                // Explicitly report cleanup failures rather than hiding them inside Drop.
                let shared = Arc::clone(&worker.shared);
                let Worker {
                    master,
                    io,
                    mut process,
                    ..
                } = worker;
                drop(io);
                drop(master);
                if let Err(error) = process.stop() {
                    match shared.snapshot.lock() {
                        Ok(mut slot) => {
                            let mut last = (**slot).clone();
                            last.state = State::Failed;
                            last.failure = Some(error.to_string());
                            *slot = Arc::new(last);
                        }
                        Err(error) => eprintln!("zmux cleanup publication failed: {error}"),
                    }
                }
            })
            .map_err(|e| SessionError::Failure(format!("start session worker: {e}")))?;
        Ok(Self {
            input: input_tx,
            wake: Mutex::new(wake_tx),
            shared,
            worker: Some(worker),
            process_id: pid,
        })
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

// Field order is intentional: release both master descriptors before ProcessGuard waits.
struct Worker {
    master: Box<dyn MasterPty + Send>,
    io: FileDescriptor,
    wake: UnixStream,
    process: ProcessGuard,
    terminal: Terminal,
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
}

impl Worker {
    fn run(&mut self) -> Result<State, SessionError> {
        let mut published = Instant::now();
        let mut dirty = false;
        loop {
            if self.shared.stop.load(Ordering::Acquire) {
                // Darwin can keep an output-blocked child in exit until its PTY master closes.
                // Signal now; Worker fields drop master/io before ProcessGuard reaps the child.
                self.process.signal()?;
                return Ok(State::Closed);
            }
            let size = self
                .shared
                .resize
                .lock()
                .map_err(|_| SessionError::Failure("resize lock poisoned".into()))?
                .take();
            if let Some(size) = size {
                self.master
                    .resize(pty_size(size))
                    .map_err(|e| SessionError::Failure(format!("resize PTY: {e}")))?;
                self.terminal
                    .resize(size)
                    .map_err(|e| SessionError::Failure(e.to_string()))?;
                dirty = true;
            }
            dirty |= self.load_input();
            if self
                .pending_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                if let Some(mut completion) = self.pending_completion.take() {
                    completion.finish(Some(InputWriteFailure::Deadline), false);
                }
                self.pending.clear();
                self.pending_deadline = None;
            }
            if !self.pending.is_empty() {
                match self.io.write(&self.pending[self.written..]) {
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
            let mut buffer = [0; IO_CHUNK_BYTES];
            if !self.eof {
                for _ in 0..READ_QUANTA {
                    match self.io.read(&mut buffer) {
                        Ok(0) => {
                            self.eof = true;
                            break;
                        }
                        Ok(count) => {
                            self.bytes_read += count as u64;
                            self.terminal
                                .process(&buffer[..count])
                                .map_err(|e| SessionError::Failure(e.to_string()))?;
                            dirty = true;
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
            if !self.process.reaped
                && let Some(status) = self
                    .process
                    .child
                    .try_wait()
                    .map_err(|e| SessionError::Failure(format!("poll PTY process: {e}")))?
            {
                self.process.reaped = true;
                self.exit_code = Some(status.exit_code());
                self.exit_seen = Some(Instant::now());
            }
            if self.process.reaped && self.eof {
                return Ok(State::Exited);
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
            dirty |= self.load_input();
            let mut fds = [
                pollfd {
                    fd: if self.eof {
                        self.wake.as_raw_file_descriptor()
                    } else {
                        self.io.as_raw_file_descriptor()
                    },
                    events: if self.eof { 0 } else { POLLIN }
                        | if self.pending.is_empty() { 0 } else { POLLOUT },
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
                let Ok(input) = self.input.try_recv() else {
                    break;
                };
                match input {
                    Input::Scroll(lines) => {
                        self.terminal.scroll(lines);
                        dirty = true;
                    }
                    Input::Wheel(event) => {
                        // Completed processes cannot consume mouse/application input.
                        self.terminal.scroll(event.lines);
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

    fn load_input(&mut self) -> bool {
        if !self.pending.is_empty() {
            return false;
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
                    self.terminal.scroll(0);
                    self.pending = bytes;
                    self.written = 0;
                    self.pending_completion = completion;
                    self.pending_deadline = deadline;
                }
                Input::Scroll(lines) => self.terminal.scroll(lines),
                Input::Wheel(event) => {
                    self.pending = self.terminal.wheel(event);
                    self.written = 0;
                }
            }
            return true;
        }
        false
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
