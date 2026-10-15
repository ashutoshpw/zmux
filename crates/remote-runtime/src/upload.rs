//! Bounded file transfer through an already authenticated terminal connection.
//! All file/process I/O belongs to the upload worker. Dropping a job only cancels it.
use crate::connection::ManagedConnection;
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, POLLOUT, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;

pub const MAX_UPLOAD_FILES: usize = 16;
pub const MAX_UPLOAD_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_UPLOAD_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 8192;
const MAX_REMOTE_PATH_BYTES: usize = 4096;
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    Preparing,
    Uploading,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct UploadSnapshot {
    pub state: UploadState,
    /// Bytes handed to SSH, not an acknowledgement of remote persistence.
    pub bytes_sent: u64,
    pub total_bytes: u64,
    /// Includes previously completed files when a later file fails or is cancelled.
    pub remote_paths: Vec<String>,
    /// A started remote operation ended without a complete local acknowledgement.
    pub remote_result_uncertain: bool,
    pub error: Option<String>,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct UploadError(pub String);

struct Shared {
    cancelled: AtomicBool,
    snapshot: Mutex<UploadSnapshot>,
}
pub struct UploadJob {
    shared: Arc<Shared>,
    worker: JoinHandle<()>,
}

impl UploadJob {
    /// Starts a worker; opening sources and waiting for SSH never runs on the caller.
    pub fn start(
        connection: Arc<ManagedConnection>,
        paths: Vec<PathBuf>,
        timeout: Duration,
    ) -> Result<Self, UploadError> {
        if paths.is_empty() || paths.len() > MAX_UPLOAD_FILES {
            return Err(UploadError(format!("choose 1–{MAX_UPLOAD_FILES} files")));
        }
        if !(Duration::from_millis(50)..=Duration::from_secs(600)).contains(&timeout) {
            return Err(UploadError(
                "upload deadline must be 50 ms–10 minutes".into(),
            ));
        }
        let shared = Arc::new(Shared {
            cancelled: AtomicBool::new(false),
            snapshot: Mutex::new(UploadSnapshot {
                state: UploadState::Preparing,
                bytes_sent: 0,
                total_bytes: 0,
                remote_paths: vec![],
                remote_result_uncertain: false,
                error: None,
            }),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("zmux-upload".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    upload(&connection, paths, &worker_shared, Instant::now() + timeout)
                }))
                .unwrap_or_else(|_| {
                    Err(UploadError(
                        "upload worker panicked; transfer aborted".into(),
                    ))
                });
                update(&worker_shared, |snapshot| match outcome {
                    Ok(()) => snapshot.state = UploadState::Completed,
                    Err(error) => {
                        snapshot.state = if worker_shared.cancelled.load(Ordering::Acquire) {
                            UploadState::Cancelled
                        } else {
                            UploadState::Failed
                        };
                        snapshot.error = Some(if snapshot.remote_result_uncertain {
                            format!(
                                "{}; remote result is uncertain, verify remote temporary storage",
                                error.0
                            )
                        } else {
                            error.0
                        });
                    }
                });
            })
            .map_err(|e| UploadError(format!("start upload worker: {e}")))?;
        Ok(Self { shared, worker })
    }
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
    }
    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }
    pub fn snapshot(&self) -> Result<UploadSnapshot, UploadError> {
        self.shared
            .snapshot
            .try_lock()
            .map(|s| s.clone())
            .map_err(|e| UploadError(format!("upload snapshot unavailable: {e}")))
    }
}
impl Drop for UploadJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn update(shared: &Shared, f: impl FnOnce(&mut UploadSnapshot)) {
    match shared.snapshot.lock() {
        Ok(mut snapshot) => f(&mut snapshot),
        Err(error) => eprintln!("zmux upload publication failed: {error}"),
    }
}
fn check(shared: &Shared, deadline: Instant) -> Result<(), UploadError> {
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(UploadError("upload cancelled".into()));
    }
    if Instant::now() >= deadline {
        return Err(UploadError("upload deadline exceeded".into()));
    }
    Ok(())
}
fn failure(context: &str, error: impl std::fmt::Display) -> UploadError {
    UploadError(format!("{context}: {error}"))
}

struct Source {
    file: File,
    name: String,
    size: u64,
}
fn upload(
    connection: &ManagedConnection,
    paths: Vec<PathBuf>,
    shared: &Shared,
    deadline: Instant,
) -> Result<(), UploadError> {
    let mut sources = Vec::new();
    let mut total = 0;
    for path in paths {
        check(shared, deadline)?;
        // O_NONBLOCK prevents a FIFO from blocking before the regular-file check.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| failure("open upload source", e))?;
        let metadata = file
            .metadata()
            .map_err(|e| failure("inspect upload source", e))?;
        if !metadata.is_file() {
            return Err(UploadError(
                "uploads currently require regular files; directories and symlinks are unavailable"
                    .into(),
            ));
        }
        if metadata.len() > MAX_UPLOAD_FILE_BYTES {
            return Err(UploadError("file exceeds the 64 MiB upload limit".into()));
        }
        total += metadata.len();
        if total > MAX_UPLOAD_TOTAL_BYTES {
            return Err(UploadError("files exceed the 256 MiB upload limit".into()));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
            .ok_or_else(|| {
                UploadError("upload filename must be UTF-8 without control characters".into())
            })?
            .to_owned();
        sources.push(Source {
            file,
            name,
            size: metadata.len(),
        });
    }
    update(shared, |s| {
        s.total_bytes = total;
        s.state = UploadState::Uploading;
    });
    let mut sent = 0;
    for source in sources {
        upload_one(connection, source, shared, deadline, &mut sent)?;
    }
    check(shared, deadline)
}

/// Quote a path for insertion into a POSIX shell, without submitting a command.
pub fn quote_shell_path(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                if let Err(error) = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL)
                    && error != nix::errno::Errno::ESRCH
                {
                    eprintln!("zmux upload cancellation failed: {error}");
                }
                if let Err(error) = self.0.wait() {
                    eprintln!("zmux upload reap failed: {error}");
                }
            }
        }
    }
}

fn upload_one(
    connection: &ManagedConnection,
    mut source: Source,
    shared: &Shared,
    deadline: Instant,
    sent: &mut u64,
) -> Result<String, UploadError> {
    check(shared, deadline)?;
    // The remote shell owns a fresh private directory. It acknowledges only an exact-length
    // complete file. EOF after cancellation/connection loss cannot acknowledge a partial file.
    let script = format!(
        "umask 077; d=$(mktemp -d \"${{TMPDIR:-/tmp}}/zmux-upload.XXXXXXXXXX\") || exit 70; f=\"$d\"/{}; trap 'rm -f -- \"$f\"; rmdir -- \"$d\"' EXIT HUP INT TERM; cat >\"$f\" || exit 71; n=$(wc -c <\"$f\") || exit 72; [ \"$n\" -eq {} ] || exit 73; trap - EXIT HUP INT TERM || exit 75; printf '%s\\000' \"$f\" || {{ rm -f -- \"$f\"; rmdir -- \"$d\"; exit 74; }}",
        quote_shell_path(&source.name),
        source.size
    );
    let spec = connection
        .operation_spec(&script)
        .map_err(|e| failure("prepare authenticated upload", e))?;
    let (writer, reader) = UnixStream::pair().map_err(|e| failure("create upload input", e))?;
    #[cfg(target_os = "macos")]
    socket2::SockRef::from(&writer)
        .set_nosigpipe(true)
        .map_err(|e| failure("configure upload input", e))?;
    writer
        .set_nonblocking(true)
        .map_err(|e| failure("configure upload input", e))?;
    let mut process = Process(
        Command::new("/usr/bin/ssh")
            .args(&spec.args)
            .process_group(0)
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(reader)))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| failure("start authenticated upload", e))?,
    );
    let mut attempt = Attempt {
        shared,
        acknowledged: false,
    };
    let file_start = *sent;
    let mut acknowledged = None;
    let mut abort = None;
    let mut abort_deadline = None;
    let mut stdout = FileDescriptor::dup(&process.0.stdout.take().expect("piped stdout"))
        .map_err(|e| failure("open upload output", e))?;
    let mut stderr = FileDescriptor::dup(&process.0.stderr.take().expect("piped stderr"))
        .map_err(|e| failure("open upload diagnostics", e))?;
    stdout
        .set_non_blocking(true)
        .map_err(|e| failure("configure upload output", e))?;
    stderr
        .set_non_blocking(true)
        .map_err(|e| failure("configure upload diagnostics", e))?;
    let mut writer = Some(writer);
    let mut remaining = source.size;
    let mut pending = Vec::new();
    let mut offset = 0;
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let mut eof_out = false;
    let mut eof_err = false;
    loop {
        if abort.is_none()
            && let Err(error) = check(shared, deadline)
        {
            abort = Some(error);
            abort_deadline = Some(Instant::now() + Duration::from_millis(250));
            writer.take();
            pending.clear();
        }
        if pending.is_empty() && writer.is_some() {
            let mut buffer = [0; CHUNK];
            let wanted = (remaining.min(CHUNK as u64)) as usize;
            let count = source
                .file
                .read(&mut buffer[..if wanted == 0 { 1 } else { wanted }])
                .map_err(|e| failure("read upload source", e))?;
            if wanted == 0 {
                if count != 0 {
                    return Err(UploadError("upload source grew while transferring".into()));
                }
                writer.take();
            } else if count == 0 {
                return Err(UploadError(
                    "upload source shrank while transferring".into(),
                ));
            } else {
                pending.extend_from_slice(&buffer[..count]);
                remaining -= count as u64;
                offset = 0;
            }
        }
        if let Some(input) = &mut writer
            && !pending.is_empty()
        {
            match input.write(&pending[offset..]) {
                Ok(0) => return Err(UploadError("upload input closed".into())),
                Ok(count) => {
                    offset += count;
                    *sent += count as u64;
                    update(shared, |s| s.bytes_sent = *sent);
                    if offset == pending.len() {
                        pending.clear();
                    }
                }
                Err(e) if retryable(&e) => {}
                Err(e) => return Err(failure("write upload input", e)),
            }
        }
        drain(
            &mut stdout,
            &mut output,
            MAX_REMOTE_PATH_BYTES + 1,
            &mut eof_out,
        )?;
        drain(
            &mut stderr,
            &mut diagnostics,
            MAX_DIAGNOSTIC_BYTES,
            &mut eof_err,
        )?;
        if acknowledged.is_none()
            && let Some(path) = parse_acknowledgement(&output)?
        {
            if *sent - file_start != source.size {
                return Err(UploadError(
                    "remote acknowledged before all source bytes were sent".into(),
                ));
            }
            update(shared, |s| s.remote_paths.push(path.clone()));
            attempt.acknowledged = true;
            acknowledged = Some(path);
        }
        if (acknowledged.is_some() || abort_deadline.is_some_and(|at| Instant::now() >= at))
            && let Some(error) = abort.take()
        {
            return Err(error);
        }
        if let Some(status) = process
            .0
            .try_wait()
            .map_err(|e| failure("poll authenticated upload", e))?
        {
            if !status.success() {
                return Err(UploadError(format!(
                    "remote upload failed ({status}): {}",
                    String::from_utf8_lossy(&diagnostics)
                )));
            }
            if eof_out && eof_err {
                if remaining != 0 || writer.is_some() {
                    return Err(UploadError(
                        "remote upload ended before all input was sent".into(),
                    ));
                }
                if let Some(error) = abort {
                    return Err(error);
                }
                return parse_acknowledgement(&output)?
                    .ok_or_else(|| UploadError("remote upload acknowledgement is missing".into()));
            }
        }
        // filedescriptor uses select(2) on macOS; unlike POSIX poll it rejects -1
        // placeholders. Submit only live descriptors on every platform.
        let mut fds = Vec::with_capacity(3);
        if !eof_out {
            fds.push(pollfd {
                fd: stdout.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if !eof_err {
            fds.push(pollfd {
                fd: stderr.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if let Some(writer) = &writer
            && !pending.is_empty()
        {
            fds.push(pollfd {
                fd: writer.as_raw_file_descriptor(),
                events: POLLOUT,
                revents: 0,
            });
        }
        // Bound cancellation latency even when a server stops consuming input.
        if let Err(error) = poll(&mut fds, Some(Duration::from_millis(10)))
            && !matches!(&error, filedescriptor::Error::Poll(e) if e.kind() == io::ErrorKind::Interrupted)
        {
            return Err(failure("poll upload I/O", error));
        }
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
fn drain(
    pipe: &mut FileDescriptor,
    output: &mut Vec<u8>,
    limit: usize,
    eof: &mut bool,
) -> Result<(), UploadError> {
    if *eof {
        return Ok(());
    }
    for _ in 0..4 {
        let mut buffer = [0; 4096];
        match pipe.read(&mut buffer) {
            Ok(0) => {
                *eof = true;
                break;
            }
            Ok(count) => {
                if output.len() + count > limit {
                    return Err(UploadError(
                        "remote upload response exceeded its bound".into(),
                    ));
                }
                output.extend_from_slice(&buffer[..count]);
            }
            Err(e) if retryable(&e) => break,
            Err(e) => return Err(failure("read upload response", e)),
        }
    }
    Ok(())
}

// A path is reported as soon as the complete acknowledgement is received, even if
// cancellation wins before the SSH process exits. Unknown remote outcome stays explicit.
struct Attempt<'a> {
    shared: &'a Shared,
    acknowledged: bool,
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.acknowledged {
            update(self.shared, |s| s.remote_result_uncertain = true);
        }
    }
}
fn parse_acknowledgement(bytes: &[u8]) -> Result<Option<String>, UploadError> {
    let Some(end) = bytes.iter().position(|b| *b == 0) else {
        return Ok(None);
    };
    if end != bytes.len() - 1 {
        return Err(UploadError(
            "remote upload acknowledgement is invalid".into(),
        ));
    }
    let path = std::str::from_utf8(&bytes[..end])
        .map_err(|_| UploadError("remote path is not UTF-8".into()))?;
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return Err(UploadError("remote upload returned an invalid path".into()));
    }
    Ok(Some(path.into()))
}
