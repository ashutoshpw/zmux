//! Original MIT Zmux optional frontend lifecycle integration for pinned Zellij.
//! This control channel never reads, queues, writes or classifies terminal input.
use nix::{
    poll::{poll, PollFd, PollFlags},
    sys::socket::{self, AddressFamily, SockAddr, SockFlag, SockType, UnixAddr},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        io::{AsRawFd, FromRawFd},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
};
use uuid::Uuid;

pub const ENV: &str = "ZMUX_ZELLIJ_FRONTEND_READY";
pub const CAPABILITY_ARGUMENT: &str = "--zmux-input-readiness-protocol";
pub const CAPABILITY_JSON: &str = "{\"capability\":\"zmux.zellij.frontend.input_ready\",\"version\":1,\"transport\":\"unix\",\"max_frame_bytes\":1024}";
const FRAME_LIMIT: usize = 1024;
const DESCRIPTOR_LIMIT: usize = 8192;

/// Must be the root executable's first operation, before logger/threads/commands.
/// Ownership is passed explicitly; no descendant inherits the descriptor variable.
pub fn capture_environment() -> Option<PathBuf> {
    let descriptor = std::env::var_os(ENV).map(PathBuf::from);
    std::env::remove_var(ENV);
    descriptor
}
pub fn capability_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    args.next().as_deref() == Some(std::ffi::OsStr::new(CAPABILITY_ARGUMENT))
        && args.next().is_none()
}
fn failure(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn unix_error(error: nix::Error) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    version: u8,
    attachment_id: Uuid,
    owner_uuid: Uuid,
    owner_generation: Uuid,
    parent_pid: u32,
    parent_uid: u32,
    socket: PathBuf,
    namespace_identity: (u64, u64),
    socket_identity: (u64, u64),
    nonce: String,
    deadline_monotonic_ms: u64,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Event {
    Challenge,
    InputReady,
    Accepted,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    event: Event,
    attachment_id: Uuid,
    nonce: String,
    sequence: u8,
}

fn monotonic_ms() -> io::Result<u64> {
    let now = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC).map_err(unix_error)?;
    u64::try_from(now.tv_sec())
        .ok()
        .and_then(|s| s.checked_mul(1000))
        .and_then(|s| s.checked_add(u64::try_from(now.tv_nsec()).ok()? / 1_000_000))
        .ok_or_else(|| failure("invalid monotonic clock"))
}
fn budget(descriptor: &Descriptor, cancelled: &AtomicBool) -> io::Result<i32> {
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::from(io::ErrorKind::Interrupted));
    }
    let left = descriptor
        .deadline_monotonic_ms
        .checked_sub(monotonic_ms()?)
        .filter(|left| (1..=10_000).contains(left))
        .ok_or_else(|| failure("frontend readiness fixed startup deadline"))?;
    Ok(left.min(20) as i32)
}
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}
fn private(path: &Path, kind: u8) -> io::Result<fs::Metadata> {
    let m = path.symlink_metadata()?;
    let type_ok = match kind {
        0 => m.is_dir(),
        1 => m.file_type().is_socket(),
        _ => m.is_file() && m.nlink() == 1 && m.len() <= DESCRIPTOR_LIMIT as u64,
    };
    let permission_ok = match kind {
        0 => m.mode() & 0o777 == 0o700,
        1 => m.mode() & 0o777 == 0o600,
        _ => m.mode() & 0o077 == 0,
    };
    if !type_ok || m.uid() != nix::unistd::geteuid().as_raw() || !permission_ok {
        return Err(failure(
            "frontend readiness private namespace type/UID/mode changed",
        ));
    }
    Ok(m)
}
impl Descriptor {
    fn read(path: &Path) -> io::Result<Self> {
        let before = private(path, 2)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?;
        let opened = file.metadata()?;
        if identity(&before) != identity(&opened)
            || !opened.is_file()
            || opened.nlink() != 1
            || opened.uid() != nix::unistd::geteuid().as_raw()
            || opened.mode() & 0o077 != 0
        {
            return Err(failure("frontend readiness descriptor replaced"));
        }
        let mut bytes = Vec::new();
        file.take((DESCRIPTOR_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > DESCRIPTOR_LIMIT {
            return Err(failure("frontend readiness descriptor exceeds 8 KiB"));
        }
        let descriptor: Self = serde_json::from_slice(&bytes)
            .map_err(|e| failure(format!("frontend readiness descriptor: {e}")))?;
        if descriptor.version != 1
            || descriptor.attachment_id.is_nil()
            || descriptor.owner_uuid.is_nil()
            || descriptor.owner_generation.is_nil()
            || descriptor.parent_pid != nix::unistd::getppid().as_raw() as u32
            || descriptor.parent_uid != nix::unistd::geteuid().as_raw()
            || descriptor.parent_pid == 0
            || descriptor.nonce.len() != 64
            || !descriptor
                .nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || descriptor.namespace_identity.1 == 0
            || descriptor.socket_identity.1 == 0
        {
            return Err(failure(
                "frontend readiness descriptor correlation rejected",
            ));
        }
        descriptor.namespace()?;
        if path
            != descriptor
                .socket
                .parent()
                .ok_or_else(|| failure("readiness socket root missing"))?
                .join("r.json")
            || path.canonicalize()? != path
            || identity(&private(path, 2)?) != identity(&opened)
        {
            return Err(failure("frontend readiness descriptor path replaced"));
        }
        budget(&descriptor, &AtomicBool::new(false))?;
        Ok(descriptor)
    }
    fn namespace(&self) -> io::Result<()> {
        let base = Path::new("/tmp").canonicalize()?;
        let m = base.symlink_metadata()?;
        if !m.is_dir()
            || (m.uid() != 0 && m.uid() != self.parent_uid)
            || (m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0)
        {
            return Err(failure("frontend readiness unsafe canonical runtime base"));
        }
        let root = self
            .socket
            .parent()
            .ok_or_else(|| failure("frontend readiness root missing"))?;
        let name = root
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| failure("frontend readiness root name"))?;
        if root.parent() != Some(base.as_path())
            || name.len() != 33
            || !name.starts_with('a')
            || !name.as_bytes()[1..]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            || self.socket.file_name() != Some(std::ffi::OsStr::new("r"))
            || self.socket.as_os_str().len() > 99
            || root.canonicalize()? != root
            || identity(&private(root, 0)?) != self.namespace_identity
            || identity(&private(&self.socket, 1)?) != self.socket_identity
        {
            return Err(failure(
                "frontend readiness namespace/socket identity changed",
            ));
        }
        Ok(())
    }
    fn matches(&self, frame: &Frame, event: Event, sequence: u8) -> io::Result<()> {
        if frame.version != 1
            || frame.event != event
            || frame.sequence != sequence
            || frame.attachment_id != self.attachment_id
            || frame.nonce != self.nonce
        {
            return Err(failure(
                "frontend readiness frame correlation/sequence rejected",
            ));
        }
        Ok(())
    }
}

fn peer(stream: &UnixStream) -> io::Result<(u32, u32)> {
    #[cfg(target_os = "linux")]
    {
        let credentials = socket::getsockopt(stream.as_raw_fd(), socket::sockopt::PeerCredentials)
            .map_err(unix_error)?;
        return Ok((
            u32::try_from(credentials.pid()).map_err(|_| failure("invalid readiness peer PID"))?,
            credentials.uid(),
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let (uid, _) = nix::unistd::getpeereid(stream.as_raw_fd()).map_err(unix_error)?;
        let mut pid: libc::pid_t = 0;
        let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: public LOCAL_PEERPID ABI, initialized exact pid_t output and
        // socklen_t size; descriptor is borrowed and remains owned by stream.
        let status = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut length,
            )
        };
        if status != 0 {
            return Err(io::Error::last_os_error());
        }
        if length as usize != std::mem::size_of::<libc::pid_t>() || pid <= 0 {
            return Err(failure("invalid readiness LOCAL_PEERPID result"));
        }
        return Ok((pid as u32, uid.as_raw()));
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = stream;
        Err(failure("frontend readiness unsupported peer platform"))
    }
}
fn wait(
    stream: &UnixStream,
    events: PollFlags,
    descriptor: &Descriptor,
    cancelled: &AtomicBool,
) -> io::Result<bool> {
    loop {
        let timeout = budget(descriptor, cancelled)?;
        let mut descriptors = [PollFd::new(stream.as_raw_fd(), events)];
        match poll(&mut descriptors, timeout) {
            Ok(0) => return Ok(false),
            Ok(_) => {
                let actual = descriptors[0]
                    .revents()
                    .ok_or_else(|| failure("invalid readiness poll result"))?;
                if actual.contains(PollFlags::POLLNVAL) {
                    return Err(failure("invalid readiness socket"));
                }
                return Ok(actual.intersects(events | PollFlags::POLLERR | PollFlags::POLLHUP));
            }
            Err(nix::errno::Errno::EINTR) => (),
            Err(error) => return Err(unix_error(error)),
        }
    }
}
fn connect(descriptor: &Descriptor, cancelled: &AtomicBool) -> io::Result<UnixStream> {
    budget(descriptor, cancelled)?;
    let fd = socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .map_err(unix_error)?;
    // SAFETY: socket returned a new uniquely owned FD, transferred exactly once.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    nix::fcntl::fcntl(
        fd,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    )
    .map_err(unix_error)?;
    stream.set_nonblocking(true)?;
    let address = SockAddr::Unix(UnixAddr::new(&descriptor.socket).map_err(unix_error)?);
    match socket::connect(stream.as_raw_fd(), &address) {
        Ok(()) => (),
        Err(
            nix::errno::Errno::EINPROGRESS | nix::errno::Errno::EALREADY | nix::errno::Errno::EINTR,
        ) => {
            while !wait(&stream, PollFlags::POLLOUT, descriptor, cancelled)? {}
            if let Some(error) = stream.take_error()? {
                return Err(error);
            }
        }
        Err(error) => return Err(unix_error(error)),
    }
    #[cfg(target_os = "macos")]
    {
        let enabled: libc::c_int = 1;
        // SAFETY: public SO_NOSIGPIPE ABI with an exact initialized int value.
        if unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&enabled as *const libc::c_int).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    descriptor.namespace()?;
    if peer(&stream)? != (descriptor.parent_pid, descriptor.parent_uid)
        || nix::unistd::getppid().as_raw() as u32 != descriptor.parent_pid
    {
        return Err(failure(
            "frontend readiness socket is not the live direct parent",
        ));
    }
    Ok(stream)
}
fn transfer(
    stream: &mut UnixStream,
    bytes: &mut [u8],
    writing: bool,
    d: &Descriptor,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        budget(d, cancel)?;
        let result = if writing {
            stream.write(&bytes[offset..])
        } else {
            stream.read(&mut bytes[offset..])
        };
        match result {
            Ok(0) => return Err(failure("frontend readiness control EOF/zero write")),
            Ok(count) => offset += count,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                wait(
                    stream,
                    if writing {
                        PollFlags::POLLOUT
                    } else {
                        PollFlags::POLLIN
                    },
                    d,
                    cancel,
                )?;
            }
            Err(e) => return Err(e),
        }
    }
    budget(d, cancel)?;
    Ok(())
}
fn receive(stream: &mut UnixStream, d: &Descriptor, cancel: &AtomicBool) -> io::Result<Frame> {
    let mut header = [0u8; 4];
    transfer(stream, &mut header, false, d, cancel)?;
    let size = u32::from_le_bytes(header) as usize;
    if !(1..=FRAME_LIMIT).contains(&size) {
        return Err(failure("frontend readiness frame exceeds 1 KiB"));
    }
    let mut body = vec![0u8; size];
    transfer(stream, &mut body, false, d, cancel)?;
    serde_json::from_slice(&body)
        .map_err(|e| failure(format!("frontend readiness strict frame: {e}")))
}
fn run(d: Descriptor, cancelled: &AtomicBool, ready: mpsc::Receiver<()>) -> io::Result<()> {
    let mut stream = connect(&d, cancelled)?;
    d.matches(&receive(&mut stream, &d, cancelled)?, Event::Challenge, 1)?;
    loop {
        let timeout = budget(&d, cancelled)?;
        match ready.recv_timeout(std::time::Duration::from_millis(timeout as u64)) {
            Ok(()) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::from(io::ErrorKind::Interrupted))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut unexpected = [0u8; 1];
                match stream.read(&mut unexpected) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                    Ok(0) => return Err(failure("readiness parent closed before input-ready")),
                    Ok(_) => return Err(failure("unexpected readiness frame before input-ready")),
                    Err(e) => return Err(e),
                }
            }
        }
    }
    d.namespace()?;
    if peer(&stream)? != (d.parent_pid, d.parent_uid)
        || nix::unistd::getppid().as_raw() as u32 != d.parent_pid
    {
        return Err(failure("frontend readiness direct parent identity changed"));
    }
    let frame = Frame {
        version: 1,
        event: Event::InputReady,
        attachment_id: d.attachment_id,
        nonce: d.nonce.clone(),
        sequence: 2,
    };
    let body = serde_json::to_vec(&frame).map_err(|e| failure(e.to_string()))?;
    if body.len() > FRAME_LIMIT {
        return Err(failure("frontend readiness outgoing frame bound"));
    }
    let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
    bytes.extend(body);
    transfer(&mut stream, &mut bytes, true, &d, cancelled)?;
    d.matches(&receive(&mut stream, &d, cancelled)?, Event::Accepted, 3)?;
    d.namespace()?;
    if peer(&stream)? != (d.parent_pid, d.parent_uid)
        || nix::unistd::getppid().as_raw() as u32 != d.parent_pid
    {
        return Err(failure("frontend readiness parent changed after accepted"));
    }
    let mut extra = [0u8; 1];
    match stream.read(&mut extra) {
        Ok(0) => (),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
        Err(error) => return Err(error),
        Ok(_) => return Err(failure("unexpected trailing frontend readiness bytes")),
    }
    budget(&d, cancelled)?;
    stream.shutdown(std::net::Shutdown::Both)?;
    Ok(())
}

enum Stage {
    Disabled,
    Queries,
    Render,
    Sent,
}
pub(crate) struct Reporter {
    stage: Stage,
    sender: Option<SyncSender<()>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl Reporter {
    pub(crate) fn new(
        path: Option<PathBuf>,
        failure_handler: impl Fn(String) + Send + 'static,
    ) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let Some(path) = path else {
            return Ok(Self {
                stage: Stage::Disabled,
                sender: None,
                cancelled,
                worker: None,
            });
        };
        let descriptor = Descriptor::read(&path)?;
        let (sender, ready) = mpsc::sync_channel(1);
        let stop = cancelled.clone();
        let worker = thread::Builder::new()
            .name("zellij-input-ready".into())
            .spawn(move || {
                let outcome = run(descriptor, &stop, ready);
                if let Err(error) = &outcome {
                    if !stop.load(Ordering::Acquire) {
                        failure_handler(format!("frontend input readiness failed: {error}"));
                    }
                }
                outcome
            })?;
        Ok(Self {
            stage: Stage::Queries,
            sender: Some(sender),
            cancelled,
            worker: Some(worker),
        })
    }
    pub(crate) fn queries_closed(&mut self, parser_still_active: bool) -> io::Result<()> {
        match self.stage {
            Stage::Disabled => Ok(()),
            Stage::Queries if !parser_still_active => {
                self.stage = Stage::Render;
                Ok(())
            }
            _ => Err(failure(
                "frontend readiness startup parser transition rejected",
            )),
        }
    }
    pub(crate) fn render_flushed(&mut self) -> io::Result<()> {
        match self.stage {
            Stage::Disabled | Stage::Sent => Ok(()),
            Stage::Queries => Err(failure(
                "frontend render cannot establish readiness before query closure",
            )),
            Stage::Render => {
                self.sender
                    .as_ref()
                    .ok_or_else(|| failure("readiness reporter queue absent"))?
                    .try_send(())
                    .map_err(|error| match error {
                        TrySendError::Full(_) => failure("frontend readiness queue full"),
                        TrySendError::Disconnected(_) => {
                            failure("frontend readiness reporter unavailable")
                        }
                    })?;
                self.stage = Stage::Sent;
                Ok(())
            }
        }
    }
}
impl Drop for Reporter {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(Ok(())) => (),
                Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => (),
                Ok(Err(error)) => {
                    log::error!("frontend readiness reporter joined with failure: {error}")
                }
                Err(_) => log::error!("frontend readiness reporter panicked during join"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{
        io::BufRead,
        os::unix::{fs::PermissionsExt, net::UnixListener},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    // This reexec is the real reporter's child process, with a real parent-owned
    // listener. No production identity predicate or reporter transport is mocked.
    #[test]
    #[ignore = "owned child fixture; invoked only by these tests"]
    fn child() {
        if let Some(path) = std::env::var_os("ZMUX_READY_TEST_SERVER") {
            let listener = UnixListener::bind(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            println!("server");
            io::stdout().flush().unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut byte = [0];
            assert_eq!(
                stream.read(&mut byte).unwrap(),
                0,
                "untrusted peer must receive no frame"
            );
            return;
        }
        let Some(path) = std::env::var_os("ZMUX_READY_TEST_DESCRIPTOR") else {
            panic!("child fixture requires its owned parent");
        };
        let (failed, failure) = mpsc::sync_channel(1);
        let reporter = Reporter::new(Some(PathBuf::from(path)), move |message| {
            failed.try_send(message).expect("one failure callback");
        });
        if std::env::var_os("ZMUX_READY_TEST_REJECT").is_some() {
            assert!(reporter.is_err(), "invalid descriptor was accepted");
            return;
        }
        let mut reporter = reporter.expect("valid descriptor");
        let mut commands = io::stdin().lock();
        loop {
            let mut line = String::new();
            assert!(commands.read_line(&mut line).unwrap() > 0);
            match line.trim() {
                "early" => assert!(reporter.render_flushed().is_err()),
                "active" => assert!(reporter.queries_closed(true).is_err()),
                "query" => reporter.queries_closed(false).unwrap(),
                "render" => reporter.render_flushed().unwrap(),
                command if command.starts_with("error ") => {
                    let expected = command.strip_prefix("error ").unwrap();
                    let message = failure.recv_timeout(Duration::from_secs(2)).unwrap();
                    assert!(
                        message.contains(expected),
                        "expected {expected:?}, got {message:?}"
                    );
                }
                "exit" => break,
                other => panic!("unknown parent test command: {other}"),
            }
            println!("step");
            io::stdout().flush().unwrap();
        }
        drop(reporter);
    }

    struct Fixture {
        child: Option<Child>,
        server: Option<Child>,
        listener: UnixListener,
        root: PathBuf,
        descriptor: Value,
        deadline: Instant,
    }
    impl Fixture {
        fn new(remaining_ms: u64, change: impl FnOnce(&mut Value)) -> Self {
            let root = Path::new("/tmp")
                .canonicalize()
                .unwrap()
                .join(format!("a{}", Uuid::new_v4().simple()));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let listener = UnixListener::bind(root.join("r")).unwrap();
            fs::set_permissions(root.join("r"), fs::Permissions::from_mode(0o600)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut descriptor = json!({
                "version": 1,
                "attachment_id": Uuid::new_v4(),
                "owner_uuid": Uuid::new_v4(),
                "owner_generation": Uuid::new_v4(),
                "parent_pid": std::process::id(),
                "parent_uid": nix::unistd::geteuid().as_raw(),
                "socket": root.join("r"),
                "namespace_identity": identity(&fs::metadata(&root).unwrap()),
                "socket_identity": identity(&fs::symlink_metadata(root.join("r")).unwrap()),
                "nonce": "a".repeat(64),
                "deadline_monotonic_ms": monotonic_ms().unwrap() + remaining_ms,
            });
            change(&mut descriptor);
            let path = root.join("r.json");
            fs::write(&path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            Self {
                child: None,
                server: None,
                listener,
                root,
                descriptor,
                deadline: Instant::now() + Duration::from_secs(4),
            }
        }
        fn spawn(&mut self, rejected: bool) {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "input_readiness::tests::child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ZMUX_READY_TEST_DESCRIPTOR", self.root.join("r.json"))
                .env_remove(ENV)
                .env_remove("ZMUX_READY_TEST_SERVER")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if rejected {
                command.env("ZMUX_READY_TEST_REJECT", "1");
            } else {
                command.env_remove("ZMUX_READY_TEST_REJECT");
            }
            let child = command.spawn().unwrap();
            nix::fcntl::fcntl(
                child.stdout.as_ref().unwrap().as_raw_fd(),
                nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
            )
            .unwrap();
            self.child = Some(child);
        }
        fn poll(&self, fd: i32, events: PollFlags) -> bool {
            assert!(Instant::now() < self.deadline, "owned fixture deadline");
            let ms = self
                .deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(20) as i32;
            poll(&mut [PollFd::new(fd, events)], ms).unwrap() > 0
        }
        fn accept(&self) -> UnixStream {
            loop {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        assert_eq!(
                            peer(&stream).unwrap(),
                            (
                                self.child.as_ref().unwrap().id(),
                                nix::unistd::geteuid().as_raw()
                            )
                        );
                        stream.set_nonblocking(true).unwrap();
                        return stream;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.poll(self.listener.as_raw_fd(), PollFlags::POLLIN);
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            }
        }
        fn step(&mut self, command: &str) {
            writeln!(
                self.child.as_mut().unwrap().stdin.as_mut().unwrap(),
                "{command}"
            )
            .unwrap();
            let mut line = Vec::new();
            loop {
                let mut byte = [0];
                match self
                    .child
                    .as_mut()
                    .unwrap()
                    .stdout
                    .as_mut()
                    .unwrap()
                    .read(&mut byte)
                {
                    Ok(1) => {
                        line.push(byte[0]);
                        if byte[0] == b'\n' {
                            if line.ends_with(b"step\n") {
                                return;
                            }
                            line.clear(); // libtest's bounded startup output
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.poll(
                            self.child
                                .as_ref()
                                .unwrap()
                                .stdout
                                .as_ref()
                                .unwrap()
                                .as_raw_fd(),
                            PollFlags::POLLIN,
                        );
                    }
                    result => panic!("fixture step EOF/read: {result:?}"),
                }
                assert!(line.len() < 1024, "bounded fixture output");
            }
        }
        fn frame(&self, event: &str, sequence: u8) -> Value {
            json!({"version":1, "event":event, "sequence":sequence,
                "attachment_id":self.descriptor["attachment_id"], "nonce":self.descriptor["nonce"]})
        }
        fn write(&self, stream: &mut UnixStream, bytes: &[u8]) {
            let mut offset = 0;
            while offset < bytes.len() {
                match stream.write(&bytes[offset..]) {
                    Ok(0) => panic!("fixture zero write"),
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.poll(stream.as_raw_fd(), PollFlags::POLLOUT);
                    }
                    Err(e) => panic!("fixture write: {e}"),
                }
            }
        }
        fn send(&self, stream: &mut UnixStream, value: Value) {
            let body = serde_json::to_vec(&value).unwrap();
            // Deliberately fragment every frame across header and body writes.
            self.write(stream, &(body.len() as u32).to_le_bytes()[..2]);
            self.write(stream, &(body.len() as u32).to_le_bytes()[2..]);
            for chunk in body.chunks(7) {
                self.write(stream, chunk);
            }
        }
        fn read_exact(&self, stream: &mut UnixStream, bytes: &mut [u8]) {
            let mut offset = 0;
            while offset < bytes.len() {
                match stream.read(&mut bytes[offset..]) {
                    Ok(0) => panic!("fixture premature control EOF"),
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.poll(stream.as_raw_fd(), PollFlags::POLLIN);
                    }
                    Err(e) => panic!("fixture read: {e}"),
                }
            }
        }
        fn ready(&self, stream: &mut UnixStream) {
            let mut header = [0; 4];
            self.read_exact(stream, &mut header);
            let size = u32::from_le_bytes(header) as usize;
            assert!((1..=1024).contains(&size));
            let mut body = vec![0; size];
            self.read_exact(stream, &mut body);
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                self.frame("input_ready", 2)
            );
        }
        fn no_ready(&self, stream: &UnixStream) {
            // An event wait is a bounded negative observation, never readiness.
            assert_eq!(
                poll(
                    &mut [PollFd::new(stream.as_raw_fd(), PollFlags::POLLIN)],
                    50
                )
                .unwrap(),
                0
            );
        }
        fn eof(&self, stream: &mut UnixStream) {
            loop {
                let mut byte = [0];
                match stream.read(&mut byte) {
                    Ok(0) => return,
                    Ok(_) => panic!("unexpected extra control frame"),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.poll(stream.as_raw_fd(), PollFlags::POLLIN);
                    }
                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return,
                    Err(e) => panic!("fixture EOF: {e}"),
                }
            }
        }
        fn finish(&mut self, rejected: bool) {
            if !rejected {
                writeln!(self.child.as_mut().unwrap().stdin.as_mut().unwrap(), "exit").unwrap();
            }
            loop {
                if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                    assert!(status.success(), "owned child failed: {status}");
                    self.child.take();
                    return;
                }
                let stdout = self.child.as_mut().unwrap().stdout.as_mut().unwrap();
                let mut bytes = [0; 1024];
                let _ = stdout.read(&mut bytes);
                self.poll(
                    self.child
                        .as_ref()
                        .unwrap()
                        .stdout
                        .as_ref()
                        .unwrap()
                        .as_raw_fd(),
                    PollFlags::POLLIN,
                );
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                // Only this exact, unreaped owned Child; no supplied/adopted PID.
                let _ = child.kill();
                let _ = child.wait();
            }
            if let Some(mut server) = self.server.take() {
                let _ = server.kill();
                let _ = server.wait();
            }
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn real_reporter_requires_query_closure_then_render_and_acknowledged_eof() {
        let mut fixture = Fixture::new(5000, |_| {});
        fixture.spawn(false);
        let mut stream = fixture.accept();
        fixture.send(&mut stream, fixture.frame("challenge", 1));
        fixture.step("early");
        fixture.step("active");
        fixture.no_ready(&stream);
        fixture.step("query");
        fixture.no_ready(&stream);
        fixture.step("render");
        fixture.step("render"); // repeat render cannot emit a duplicate
        fixture.ready(&mut stream);
        fixture.no_ready(&stream); // reporter must wait for accepted3
        fixture.send(&mut stream, fixture.frame("accepted", 3));
        fixture.eof(&mut stream);
        assert!(
            fixture
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_none(),
            "reporter EOF must not terminate frontend"
        );
        fixture.finish(false);
    }

    #[test]
    fn real_reporter_rejects_bad_challenge_and_ack_correlation() {
        for phase in ["challenge", "accepted"] {
            for field in [
                "nonce",
                "attachment_id",
                "sequence",
                "unknown",
                "version",
                "event",
            ] {
                let mut fixture = Fixture::new(5000, |_| {});
                fixture.spawn(false);
                let mut stream = fixture.accept();
                if phase == "accepted" {
                    fixture.send(&mut stream, fixture.frame("challenge", 1));
                    fixture.step("query");
                    fixture.step("render");
                    fixture.ready(&mut stream);
                }
                let mut frame = fixture.frame(phase, if phase == "challenge" { 1 } else { 3 });
                frame[field] = match field {
                    "nonce" => json!("b".repeat(64)),
                    "attachment_id" => json!(Uuid::new_v4()),
                    "sequence" => json!(9),
                    "version" => json!(2),
                    "event" => json!("input_ready"),
                    _ => json!(true),
                };
                fixture.send(&mut stream, frame);
                fixture.step(if field == "unknown" {
                    "error strict frame"
                } else {
                    "error correlation/sequence rejected"
                });
                fixture.eof(&mut stream);
                fixture.finish(false);
            }
        }
    }

    #[test]
    fn real_reporter_rejects_malformed_frames_and_parent_eof() {
        for case in ["duplicate", "oversized", "truncated", "eof"] {
            let mut fixture = Fixture::new(5000, |_| {});
            fixture.spawn(false);
            let mut stream = fixture.accept();
            match case {
                "duplicate" => {
                    let text = serde_json::to_string(&fixture.frame("challenge", 1)).unwrap();
                    let body = format!("{{\"version\":1,{}", &text[1..]);
                    fixture.write(&mut stream, &(body.len() as u32).to_le_bytes());
                    fixture.write(&mut stream, body.as_bytes());
                }
                "oversized" => fixture.write(&mut stream, &1025u32.to_le_bytes()),
                "truncated" => {
                    fixture.write(&mut stream, &5u32.to_le_bytes());
                    fixture.write(&mut stream, b"{");
                }
                _ => (),
            }
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            fixture.step(match case {
                "duplicate" => "error strict frame",
                "oversized" => "error frame exceeds 1 KiB",
                _ => "error control EOF/zero write",
            });
            fixture.eof(&mut stream);
            fixture.finish(false);
        }
    }

    #[test]
    fn real_reporter_deadline_and_cancellation_close_without_ready() {
        for cancel in [true, false] {
            let mut fixture = Fixture::new(if cancel { 5000 } else { 1500 }, |_| {});
            fixture.spawn(false);
            let mut stream = fixture.accept();
            fixture.send(&mut stream, fixture.frame("challenge", 1));
            fixture.step("query");
            if cancel {
                fixture.finish(false);
            } else {
                fixture.step("error fixed startup deadline");
                fixture.finish(false);
            }
            fixture.eof(&mut stream);
        }
        // Same original budget applies while waiting for accepted3.
        let mut fixture = Fixture::new(1500, |_| {});
        fixture.spawn(false);
        let mut stream = fixture.accept();
        fixture.send(&mut stream, fixture.frame("challenge", 1));
        fixture.step("query");
        fixture.step("render");
        fixture.ready(&mut stream);
        fixture.step("error fixed startup deadline");
        fixture.eof(&mut stream);
        fixture.finish(false);
    }

    #[test]
    fn real_reporter_rechecks_namespace_and_rejects_pre_ready_control() {
        for replace in [true, false] {
            let mut fixture = Fixture::new(5000, |_| {});
            fixture.spawn(false);
            let mut stream = fixture.accept();
            fixture.send(&mut stream, fixture.frame("challenge", 1));
            if replace {
                fixture.step("query");
                fs::remove_file(fixture.root.join("r")).unwrap();
                let _replacement = UnixListener::bind(fixture.root.join("r")).unwrap();
                fs::set_permissions(fixture.root.join("r"), fs::Permissions::from_mode(0o600))
                    .unwrap();
                fixture.step("render");
                fixture.step("error namespace/socket identity changed");
            } else {
                fixture.send(&mut stream, fixture.frame("accepted", 3));
                fixture.step("error unexpected readiness frame before input-ready");
            }
            fixture.eof(&mut stream);
            fixture.finish(false);
        }
        let mut fixture = Fixture::new(1500, |_| {});
        fixture.spawn(false);
        let mut stream = fixture.accept();
        fixture.write(&mut stream, &[1, 0]); // partial header held open
        fixture.step("error fixed startup deadline");
        fixture.eof(&mut stream);
        fixture.finish(false);
    }

    #[test]
    fn real_reporter_rejects_socket_peer_that_is_not_its_direct_parent() {
        let mut fixture = Fixture::new(5000, |_| {});
        // This retained sibling creates the replacement listener itself. Merely
        // accepting an inherited parent listener would not test kernel peer PID.
        fs::remove_file(fixture.root.join("r")).unwrap();
        let server = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "input_readiness::tests::child",
                "--ignored",
                "--nocapture",
            ])
            .env("ZMUX_READY_TEST_SERVER", fixture.root.join("r"))
            .env_remove(ENV)
            .env_remove("ZMUX_READY_TEST_DESCRIPTOR")
            .env_remove("ZMUX_READY_TEST_REJECT")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        nix::fcntl::fcntl(
            server.stdout.as_ref().unwrap().as_raw_fd(),
            nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
        )
        .unwrap();
        fixture.server = Some(server);
        let mut output = Vec::new();
        loop {
            let mut bytes = [0; 256];
            match fixture
                .server
                .as_mut()
                .unwrap()
                .stdout
                .as_mut()
                .unwrap()
                .read(&mut bytes)
            {
                Ok(0) => panic!("server fixture ended before bind"),
                Ok(n) => {
                    output.extend_from_slice(&bytes[..n]);
                    assert!(output.len() < 1024);
                    if output.windows(7).any(|w| w == b"server\n") {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    fixture.poll(
                        fixture
                            .server
                            .as_ref()
                            .unwrap()
                            .stdout
                            .as_ref()
                            .unwrap()
                            .as_raw_fd(),
                        PollFlags::POLLIN,
                    );
                }
                Err(e) => panic!("server fixture output: {e}"),
            }
        }
        fixture.descriptor["socket_identity"] = json!(identity(
            &fs::symlink_metadata(fixture.root.join("r")).unwrap()
        ));
        fs::write(
            fixture.root.join("r.json"),
            serde_json::to_vec(&fixture.descriptor).unwrap(),
        )
        .unwrap();
        fixture.spawn(false);
        fixture.step("error socket is not the live direct parent");
        fixture.finish(false);
        loop {
            if let Some(status) = fixture.server.as_mut().unwrap().try_wait().unwrap() {
                assert!(status.success());
                fixture.server.take();
                break;
            }
            fixture.poll(
                fixture
                    .server
                    .as_ref()
                    .unwrap()
                    .stdout
                    .as_ref()
                    .unwrap()
                    .as_raw_fd(),
                PollFlags::POLLIN,
            );
        }
    }

    #[test]
    fn real_reporter_rejects_descriptor_parent_nonce_inode_and_permissions() {
        for case in [
            "parent",
            "uid",
            "nonce",
            "inode",
            "unknown",
            "deadline",
            "mode",
            "symlink",
            "oversized",
            "duplicate",
        ] {
            let mut fixture = Fixture::new(5000, |d| match case {
                "parent" => d["parent_pid"] = json!(std::process::id() + 1),
                "uid" => d["parent_uid"] = json!(nix::unistd::geteuid().as_raw() + 1),
                "nonce" => d["nonce"] = json!("A".repeat(64)),
                "inode" => {
                    d["socket_identity"][1] = json!(d["socket_identity"][1].as_u64().unwrap() + 1)
                }
                "unknown" => d["extra"] = json!(true),
                "deadline" => d["deadline_monotonic_ms"] = json!(monotonic_ms().unwrap() + 20000),
                _ => (),
            });
            let path = fixture.root.join("r.json");
            match case {
                "mode" => {
                    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755)).unwrap()
                }
                "symlink" => {
                    fs::rename(&path, fixture.root.join("actual")).unwrap();
                    std::os::unix::fs::symlink("actual", &path).unwrap();
                }
                "oversized" => fs::write(&path, vec![b' '; 8193]).unwrap(),
                "duplicate" => {
                    let text = fs::read_to_string(&path).unwrap();
                    fs::write(&path, format!("{{\"version\":1,{}", &text[1..])).unwrap();
                }
                _ => (),
            }
            fixture.spawn(true);
            fixture.finish(true);
        }
    }
}
