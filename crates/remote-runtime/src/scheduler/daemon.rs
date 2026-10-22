//! Foreground Unix-domain socket service and one-request stdio RPC command.

use super::{
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, SchedulerError, SchedulerErrorCode, SchedulerRequest,
    SchedulerResponse, SchedulerService, service::default_root,
};
use nix::unistd::geteuid;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const START_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_WORKERS: usize = 16;

/// Run the service in the foreground until SIGINT or SIGTERM.
pub fn run_serve() -> Result<(), String> {
    let service = SchedulerService::open_from_environment().map_err(|error| error.message)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, shutdown.clone())
        .map_err(|error| format!("register SIGINT handler: {error}"))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, shutdown.clone())
        .map_err(|error| format!("register SIGTERM handler: {error}"))?;
    serve_with_shutdown(service, shutdown)
}

/// Run one stdin/stdout request through the foreground daemon. No daemon is
/// started as a side effect; unavailable service is an explicit response.
pub fn run_rpc() -> Result<(), String> {
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|error| format!("read scheduler request: {error}"))?;
    let request = match parse_request_frame(&input) {
        Ok(request) => request,
        Err(error) => {
            let response = SchedulerResponse::error(
                None,
                SchedulerError::new(SchedulerErrorCode::InvalidRequest, error),
            );
            return write_response(&response);
        }
    };
    let socket = default_root()
        .map_err(|error| error.message)?
        .join("control.sock");
    let mut stream = match connect_private_socket(&socket) {
        Ok(stream) => stream,
        Err(error) => {
            let response = SchedulerResponse::error(
                request.request_id,
                SchedulerError {
                    code: SchedulerErrorCode::ServiceUnavailable,
                    message: format!("scheduler daemon is unavailable: {error}"),
                    retryable: true,
                    outcome_unknown: false,
                    current_version: None,
                },
            );
            return write_response(&response);
        }
    };
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("configure scheduler request timeout: {error}"))?;
    stream
        .write_all(&input)
        .and_then(|_| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|error| format!("send scheduler request: {error}"))?;
    stream
        .set_nonblocking(true)
        .map_err(|error| format!("configure scheduler response socket: {error}"))?;
    let response_bytes = read_bounded_until(
        &mut stream,
        MAX_RESPONSE_BYTES + 1,
        Instant::now() + REQUEST_TIMEOUT,
    )
    .map_err(|error| format!("read scheduler response: {error}"))?;
    let response = parse_response_frame(&response_bytes)
        .map_err(|error| format!("invalid scheduler daemon response: {error}"))?;
    write_response(&response)
}

pub(crate) fn serve_with_shutdown(
    service: SchedulerService,
    shutdown: Arc<AtomicBool>,
) -> Result<(), String> {
    let root = service.store_root().to_path_buf();
    let _daemon_lock = acquire_daemon_lock(&root.join("daemon.lock"))?;
    service
        .recover_on_start()
        .map_err(|error| format!("recover scheduler state: {}", error.message))?;
    let socket_path = root.join("control.sock");
    remove_stale_socket(&socket_path)?;
    if socket_path.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err("scheduler control socket path exceeds the Unix-domain path limit".into());
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("bind scheduler control socket: {error}"))?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("secure scheduler control socket: {error}"))?;
    let _socket_guard = SocketGuard::new(&socket_path)
        .map_err(|error| format!("inspect scheduler control socket: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("configure scheduler control socket: {error}"))?;
    eprintln!("zmux scheduler serving for uid {}", geteuid().as_raw());
    let active_workers = Arc::new(AtomicUsize::new(0));

    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if active_workers.fetch_add(1, Ordering::AcqRel) >= MAX_REQUEST_WORKERS {
                    active_workers.fetch_sub(1, Ordering::AcqRel);
                    reject_busy(stream);
                } else {
                    let service = service.clone();
                    let worker_counter = active_workers.clone();
                    if let Err(error) = thread::Builder::new()
                        .name("zmux-scheduler-request".into())
                        .spawn(move || {
                            handle_connection(&service, stream);
                            worker_counter.fetch_sub(1, Ordering::AcqRel);
                        })
                    {
                        active_workers.fetch_sub(1, Ordering::AcqRel);
                        eprintln!("zmux scheduler request worker failed: {error}");
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("accept scheduler request: {error}")),
        }
        if let Err(error) = service.dispatch_due_once() {
            eprintln!("zmux scheduler dispatch check failed: {}", error.message);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn handle_connection(service: &SchedulerService, mut stream: UnixStream) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let bytes = match read_bounded_until(&mut stream, MAX_REQUEST_BYTES, deadline) {
        Ok(bytes) => bytes,
        Err(message) => {
            let response = SchedulerResponse::error(
                None,
                SchedulerError::new(SchedulerErrorCode::InvalidRequest, message),
            );
            let _ = write_socket_frame(&mut stream, &response, deadline);
            return;
        }
    };
    let response = match parse_request_frame(&bytes) {
        Ok(request) => service.handle(request),
        Err(message) => SchedulerResponse::error(
            None,
            SchedulerError::new(SchedulerErrorCode::InvalidRequest, message),
        ),
    };
    if let Err(error) = write_socket_frame(&mut stream, &response, deadline) {
        eprintln!("zmux scheduler response write failed: {error}");
    }
}

fn reject_busy(mut stream: UnixStream) {
    let _ = stream.set_nonblocking(true);
    // A connected Unix socket closed with unread input may reset the peer and
    // discard our Busy frame. Drain currently queued request bytes first; a
    // slow client still gets bounded service and never occupies a worker.
    let mut discarded = [0u8; 4_096];
    let mut discarded_bytes = 0usize;
    while discarded_bytes <= MAX_REQUEST_BYTES {
        match stream.read(&mut discarded) {
            Ok(0) => break,
            Ok(count) => discarded_bytes = discarded_bytes.saturating_add(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(_) => return,
        }
    }
    let response = SchedulerResponse::error(
        None,
        SchedulerError {
            code: SchedulerErrorCode::Busy,
            message: "scheduler request worker capacity is full".into(),
            retryable: true,
            outcome_unknown: false,
            current_version: None,
        },
    );
    let _ = write_socket_frame(
        &mut stream,
        &response,
        Instant::now() + Duration::from_millis(100),
    );
}

fn read_bounded_until(
    reader: &mut impl Read,
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4_096];
    loop {
        if Instant::now() >= deadline {
            return Err("scheduler frame deadline expired".into());
        }
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(count) => {
                if bytes.len() + count > limit {
                    return Err("scheduler frame exceeded its byte limit".into());
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("scheduler frame deadline expired".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("scheduler frame read failed: {error}")),
        }
    }
}

fn write_socket_frame(
    writer: &mut impl Write,
    response: &SchedulerResponse,
    deadline: Instant,
) -> io::Result<()> {
    let body = serde_json::to_vec(response).map_err(io::Error::other)?;
    let body = if body.len() <= MAX_RESPONSE_BYTES {
        body
    } else {
        serde_json::to_vec(&SchedulerResponse::error(
            response.request_id,
            SchedulerError::new(
                SchedulerErrorCode::Internal,
                "scheduler response exceeds the 1 MiB limit",
            ),
        ))
        .map_err(io::Error::other)?
    };
    let mut frame = body;
    frame.push(b'\n');
    let mut written = 0;
    while written < frame.len() {
        match writer.write(&frame[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "scheduler socket closed",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "scheduler response deadline expired",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn parse_request_frame(bytes: &[u8]) -> Result<SchedulerRequest, String> {
    if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
        return Err("scheduler request is empty or exceeds the 16 KiB limit".into());
    }
    if bytes.last() != Some(&b'\n') || bytes[..bytes.len() - 1].contains(&b'\n') {
        return Err("scheduler request must be exactly one newline-terminated JSON object".into());
    }
    serde_json::from_slice(&bytes[..bytes.len() - 1])
        .map_err(|error| format!("invalid scheduler request JSON: {error}"))
}

fn parse_response_frame(bytes: &[u8]) -> Result<SchedulerResponse, String> {
    if bytes.is_empty() || bytes.len() > MAX_RESPONSE_BYTES + 1 || bytes.last() != Some(&b'\n') {
        return Err("scheduler response is empty, oversized, or missing its terminator".into());
    }
    if bytes[..bytes.len() - 1].contains(&b'\n') {
        return Err("scheduler response contains more than one JSON line".into());
    }
    serde_json::from_slice(&bytes[..bytes.len() - 1]).map_err(|error| error.to_string())
}

fn write_response(response: &SchedulerResponse) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    write_frame(&mut stdout, response).map_err(|error| format!("write scheduler response: {error}"))
}

fn write_frame(writer: &mut impl Write, response: &SchedulerResponse) -> io::Result<()> {
    let body = serde_json::to_vec(response).map_err(io::Error::other)?;
    if body.len() > MAX_RESPONSE_BYTES {
        let fallback = SchedulerResponse::error(
            response.request_id,
            SchedulerError::new(
                SchedulerErrorCode::Internal,
                "scheduler response exceeds the 1 MiB limit",
            ),
        );
        let body = serde_json::to_vec(&fallback).map_err(io::Error::other)?;
        writer.write_all(&body)?;
        return writer.write_all(b"\n");
    }
    writer.write_all(&body)?;
    writer.write_all(b"\n")
}

fn connect_private_socket(path: &Path) -> io::Result<UnixStream> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "scheduler control socket is not private to this account",
        ));
    }
    UnixStream::connect(path)
}

fn acquire_daemon_lock(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| format!("open scheduler daemon lock: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect scheduler daemon lock: {error}"))?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err("scheduler daemon lock is not a private regular file".into());
    }
    let deadline = Instant::now() + START_LOCK_TIMEOUT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(fs::TryLockError::WouldBlock) => {
                return Err("another scheduler daemon already owns this state root".into());
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(format!("lock scheduler daemon: {error}"));
            }
        }
    }
}

fn remove_stale_socket(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect scheduler control socket: {error}")),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != geteuid().as_raw() {
        return Err("scheduler control path exists and is not an owned Unix socket".into());
    }
    match UnixStream::connect(path) {
        Ok(_) => return Err("scheduler daemon is already accepting requests".into()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) => {}
        Err(error) => return Err(format!("probe existing scheduler socket: {error}")),
    }
    let current = fs::symlink_metadata(path)
        .map_err(|error| format!("recheck stale scheduler socket: {error}"))?;
    if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
        return Err("scheduler control socket changed during stale-socket cleanup".into());
    }
    fs::remove_file(path).map_err(|error| format!("remove stale scheduler socket: {error}"))
}

struct SocketGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl SocketGuard {
    fn new(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != geteuid().as_raw() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "bound scheduler control endpoint is not an owned socket",
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path)
            && metadata.file_type().is_socket()
            && metadata.dev() == self.dev
            && metadata.ino() == self.ino
            && let Err(error) = fs::remove_file(&self.path)
        {
            eprintln!("zmux scheduler socket cleanup failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_bounded_until;
    use std::{
        io::{self, Read},
        time::{Duration, Instant},
    };

    struct StreamingReader;

    impl Read for StreamingReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_millis(1));
            bytes.fill(b'x');
            Ok(bytes.len())
        }
    }

    #[test]
    fn bounded_reader_enforces_deadline_while_data_keeps_arriving() {
        let deadline = Instant::now() + Duration::from_millis(20);
        let error = read_bounded_until(&mut StreamingReader, 1024 * 1024, deadline)
            .expect_err("a continuous stream must not extend its absolute deadline");
        assert_eq!(error, "scheduler frame deadline expired");
    }
}
