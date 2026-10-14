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
    path::PathBuf,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;
use zmux_terminal_adapter::{Size, Terminal, Viewport};

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

struct Shared {
    stop: AtomicBool,
    resize: Mutex<Option<Size>>,
    snapshot: Mutex<Arc<Snapshot>>,
}

pub struct Session {
    input: SyncSender<Vec<u8>>,
    wake: Mutex<FileDescriptor>,
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
        let (mut wake_tx, mut wake_rx) = filedescriptor::socketpair()
            .map_err(|e| SessionError::Failure(format!("create wake channel: {e}")))?;
        wake_tx
            .set_non_blocking(true)
            .map_err(|e| SessionError::Failure(e.to_string()))?;
        wake_rx
            .set_non_blocking(true)
            .map_err(|e| SessionError::Failure(e.to_string()))?;
        let mut command = CommandBuilder::new(&launch.program);
        command.args(&launch.args);
        command.env("TERM", "xterm-256color");
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
                };
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker.run()));
                worker.shared.stop.store(true, Ordering::Release);
                match result {
                    Ok(Ok(state)) => worker.publish(state, None),
                    Ok(Err(error)) => worker.publish(State::Failed, Some(error.to_string())),
                    Err(_) => {
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
        self.input.try_send(bytes.to_vec()).map_err(|e| match e {
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
    wake: FileDescriptor,
    process: ProcessGuard,
    terminal: Terminal,
    input: Receiver<Vec<u8>>,
    shared: Arc<Shared>,
    bytes_read: u64,
    exit_code: Option<u32>,
    exit_seen: Option<Instant>,
    eof: bool,
    pending: Vec<u8>,
    written: usize,
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
            self.load_input();
            if !self.pending.is_empty() {
                match self.io.write(&self.pending[self.written..]) {
                    Ok(0) => {
                        return Err(SessionError::Failure(
                            "PTY input write returned zero bytes".into(),
                        ));
                    }
                    Ok(count) => {
                        self.written += count;
                        if self.written == self.pending.len() {
                            self.pending.clear();
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
            self.load_input();
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

    fn load_input(&mut self) {
        if self.pending.is_empty()
            && let Ok(input) = self.input.try_recv()
        {
            self.pending = input;
            self.written = 0;
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
