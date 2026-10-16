use super::observer_protocol::FrontendReceipt;
use super::{
    LIMIT, fail,
    files::{self, LocalZellij, Record},
    process::{ForegroundTerminal, OwnedChild},
    supervisor,
};
use nix::{
    sys::socket::{getsockopt, sockopt},
    unistd::geteuid,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        net::{UnixListener, UnixStream},
    },
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ExitObservation, ProcessIdentity, process_identity};

pub(super) fn peer(stream: &UnixStream) -> io::Result<ProcessIdentity> {
    #[cfg(target_os = "linux")]
    let (pid, uid) = {
        let credentials = getsockopt(stream, sockopt::PeerCredentials)?;
        (credentials.pid(), credentials.uid())
    };
    #[cfg(target_os = "macos")]
    let (pid, uid) = {
        let pid = getsockopt(stream, sockopt::LocalPeerPid)?;
        let (uid, _) = nix::unistd::getpeereid(stream)?;
        (pid, uid.as_raw())
    };
    if pid <= 0 || uid != geteuid().as_raw() {
        return Err(fail("peer kernel PID/UID rejected"));
    }
    let identity = process_identity(pid as u32)?;
    if identity.uid != uid {
        return Err(fail("peer process credentials changed"));
    }
    Ok(identity)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputLoss {
    pub bytes: usize,
    pub sha256: String,
    pub errno: Option<i32>,
    pub requested_cancel_before_send: bool,
    pub content: String,
}
fn loss(
    pending: &VecDeque<u8>,
    errno: Option<i32>,
    requested_cancel_before_send: bool,
) -> InputLoss {
    let mut digest = Sha256::new();
    let (first, second) = pending.as_slices();
    digest.update(first);
    digest.update(second);
    InputLoss {
        bytes: pending.len(),
        sha256: format!("{:x}", digest.finalize()),
        errno,
        requested_cancel_before_send,
        content: "opaque_not_interpreted".into(),
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportReport {
    pub authenticated_connections: usize,
    pub rejected_peers: usize,
    pub read_from_client: u64,
    pub forwarded_to_server: u64,
    pub read_from_server: u64,
    pub forwarded_to_client: u64,
    pub discarded_to_client: u64,
    pub rejected_to_server: u64,
    pub rejected_during_requested_cancel: u64,
    pub pending_to_server_on_cleanup: u64,
    pub peak_directional_bytes: usize,
    pub peak_connections: usize,
    pub errors: Vec<String>,
    pub input_loss: Vec<InputLoss>,
}
impl TransportReport {
    fn confirm_detach(
        &self,
        requested: bool,
        outcome: ExitObservation,
        owner_confirmed: bool,
    ) -> io::Result<()> {
        if self.pending_to_server_on_cleanup > 0 {
            return Err(fail(format!(
                "opaque pending input at cleanup: {}bytes; outcome unknown; receipt={:?}",
                self.pending_to_server_on_cleanup, self.input_loss
            )));
        }
        if self.rejected_to_server > 0
            && !(requested
                && outcome == ExitObservation::Exited(0)
                && owner_confirmed
                && self.rejected_to_server == self.rejected_during_requested_cancel)
        {
            return Err(fail(format!(
                "unconfirmed opaque input loss: {}bytes rejected; outcome unknown, command not replayed; receipt={:?}",
                self.rejected_to_server, self.input_loss
            )));
        }
        Ok(())
    }
}
struct Connection {
    client: UnixStream,
    backend: UnixStream,
    to_client: VecDeque<u8>,
    to_server: VecDeque<u8>,
    client_eof: bool,
    backend_eof: bool,
    client_closed: bool,
    backend_closed: bool,
    reverse_close: Option<Instant>,
}
impl Connection {
    fn pump(&mut self, report: &mut TransportReport, cancel: &AtomicBool) -> io::Result<bool> {
        if self
            .reverse_close
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(fail("reverse-half-close drain exceeded2s"));
        }
        for upstream in [true, false] {
            let (source, destination, eof, read_total) = if upstream {
                (
                    &mut self.client,
                    &mut self.to_server,
                    &mut self.client_eof,
                    &mut report.read_from_client,
                )
            } else {
                (
                    &mut self.backend,
                    &mut self.to_client,
                    &mut self.backend_eof,
                    &mut report.read_from_server,
                )
            };
            if !*eof && destination.len() < LIMIT {
                let mut bytes = [0; 16384];
                let room = bytes.len().min(LIMIT - destination.len());
                match source.read(&mut bytes[..room]) {
                    Ok(0) => *eof = true,
                    Ok(count) => {
                        destination.extend(&bytes[..count]);
                        *read_total += count as u64;
                        report.peak_directional_bytes =
                            report.peak_directional_bytes.max(destination.len());
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) if error.kind() == io::ErrorKind::ConnectionReset => *eof = true,
                    Err(error) => return Err(error),
                }
            }
        }
        for upstream in [true, false] {
            let requested_cancel = cancel.load(Ordering::Acquire);
            let (destination, pending, forwarded) = if upstream {
                (
                    &mut self.backend,
                    &mut self.to_server,
                    &mut report.forwarded_to_server,
                )
            } else {
                (
                    &mut self.client,
                    &mut self.to_client,
                    &mut report.forwarded_to_client,
                )
            };
            if pending.is_empty() {
                continue;
            }
            let (first, second) = pending.as_slices();
            let bytes = if first.is_empty() { second } else { first };
            match destination.write(bytes) {
                Ok(0) => return Err(fail("zero-byte relay write")),
                Ok(count) => {
                    pending.drain(..count);
                    *forwarded += count as u64;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error)
                    if matches!(
                        error.raw_os_error(),
                        Some(nix::libc::EPIPE | nix::libc::ECONNRESET | nix::libc::ENOTCONN)
                    ) =>
                {
                    if upstream {
                        if report.input_loss.len() >= 16 {
                            return Err(fail("input-loss receipt bound exceeded"));
                        }
                        report.input_loss.push(loss(
                            pending,
                            error.raw_os_error(),
                            requested_cancel,
                        ));
                        report.rejected_to_server += pending.len() as u64;
                        if requested_cancel {
                            report.rejected_during_requested_cancel += pending.len() as u64;
                        }
                        pending.clear();
                        report.discarded_to_client += self.to_client.len() as u64;
                        self.to_client.clear();
                        return Ok(true);
                    }
                    report.discarded_to_client += pending.len() as u64;
                    pending.clear();
                    self.client_closed = true;
                    self.backend_eof = true;
                    self.reverse_close = Some(Instant::now() + Duration::from_secs(2));
                }
                Err(error) => return Err(error),
            }
        }
        if self.client_eof && self.to_server.is_empty() && !self.backend_closed {
            shutdown(&self.backend)?;
            self.backend_closed = true;
        }
        if self.backend_eof && self.to_client.is_empty() && !self.client_closed {
            shutdown(&self.client)?;
            self.client_closed = true;
        }
        Ok(self.client_eof
            && self.backend_eof
            && self.to_client.is_empty()
            && self.to_server.is_empty())
    }
}
fn shutdown(stream: &UnixStream) -> io::Result<()> {
    match stream.shutdown(std::net::Shutdown::Write) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(nix::libc::ENOTCONN | nix::libc::EPIPE | nix::libc::ECONNRESET)
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

struct Broker {
    root: std::path::PathBuf,
    namespace: Vec<(std::path::PathBuf, (u64, u64))>,
    expected: Arc<Mutex<Option<ProcessIdentity>>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    report: Arc<Mutex<TransportReport>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Broker {
    fn start(record: &Record) -> io::Result<Self> {
        let root = files::runtime_base()?.join(format!("a{}", Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        files::directory(&root, true)?;
        files::directory(&root.join("s"), true)?;
        files::directory(&root.join("s/0.43.0"), true)?;
        files::directory(&root.join("cache"), true)?;
        let socket = root.join("s/0.43.0").join(record.session_name());
        files::socket_path(&socket)?;
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let namespace = [
            &root,
            &root.join("s"),
            &root.join("s/0.43.0"),
            &root.join("cache"),
            &socket,
        ]
        .into_iter()
        .map(|path| Ok((path.clone(), files::inode(path)?)))
        .collect::<io::Result<Vec<_>>>()?;
        let expected = Arc::new(Mutex::new(None::<ProcessIdentity>));
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let report = Arc::new(Mutex::new(TransportReport::default()));
        let (expected_worker, stop_worker, cancel_worker, report_worker) = (
            expected.clone(),
            stop.clone(),
            cancel.clone(),
            report.clone(),
        );
        let backend = record.backend()?;
        let server = record
            .server
            .clone()
            .ok_or_else(|| fail("missing backend binding"))?;
        let worker = thread::Builder::new()
            .name("zmux-zellij-broker".into())
            .spawn(move || {
                let mut connections: Vec<Connection> = Vec::new();
                let mut stats = TransportReport::default();
                let result = (|| -> io::Result<()> {
                    while !stop_worker.load(Ordering::Acquire) {
                        let expected = expected_worker
                            .lock()
                            .map_err(|_| fail("child binding lock poisoned"))?
                            .clone();
                        if connections.len() < 4
                            && let Some(expected) = &expected
                        {
                            match listener.accept() {
                                Ok((client, _)) => {
                                    if peer(&client)? != *expected {
                                        stats.rejected_peers += 1;
                                    } else {
                                        let upstream = connect(
                                            &backend,
                                            Instant::now() + Duration::from_secs(2),
                                        )?;
                                        if peer(&upstream)? != server {
                                            return Err(fail(
                                                "backend connected peer identity changed",
                                            ));
                                        }
                                        client.set_nonblocking(true)?;
                                        upstream.set_nonblocking(true)?;
                                        connections.push(Connection {
                                            client,
                                            backend: upstream,
                                            to_client: VecDeque::new(),
                                            to_server: VecDeque::new(),
                                            client_eof: false,
                                            backend_eof: false,
                                            client_closed: false,
                                            backend_closed: false,
                                            reverse_close: None,
                                        });
                                        stats.authenticated_connections += 1;
                                        stats.peak_connections =
                                            stats.peak_connections.max(connections.len());
                                    }
                                }
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                                Err(error) => return Err(error),
                            }
                        }
                        for index in (0..connections.len()).rev() {
                            if connections[index].pump(&mut stats, &cancel_worker)? {
                                connections.remove(index);
                            }
                        }
                        *report_worker
                            .lock()
                            .map_err(|_| fail("transport report lock poisoned"))? = stats.clone();
                        if stats.rejected_to_server > 0 {
                            break;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(())
                })();
                for connection in connections {
                    stats.pending_to_server_on_cleanup += connection.to_server.len() as u64;
                    stats.discarded_to_client += connection.to_client.len() as u64;
                    if !connection.to_server.is_empty() && stats.input_loss.len() < 16 {
                        stats
                            .input_loss
                            .push(loss(&connection.to_server, None, false));
                    }
                }
                if let Err(error) = result {
                    stats.errors.push(error.to_string());
                }
                if let Ok(mut slot) = report_worker.lock() {
                    *slot = stats;
                }
            })?;
        Ok(Self {
            root,
            namespace,
            expected,
            stop,
            cancel,
            report,
            worker: Some(worker),
        })
    }
    fn close(&mut self) -> io::Result<TransportReport> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| fail("broker worker panicked"))?;
        }
        let report = self
            .report
            .lock()
            .map_err(|_| fail("broker report poisoned"))?
            .clone();
        for (index, (path, identity)) in self.namespace.iter().enumerate() {
            if files::inode(path)? != *identity {
                return Err(fail("broker namespace/socket replaced; cleanup denied"));
            }
            if index + 1 != self.namespace.len() {
                files::validate_directory(path, true)?;
            } else {
                let metadata = path.symlink_metadata()?;
                if !std::os::unix::fs::FileTypeExt::is_socket(&metadata.file_type())
                    || metadata.uid() != geteuid().as_raw()
                {
                    return Err(fail("broker socket type/owner changed; cleanup denied"));
                }
            }
        }
        fs::remove_dir_all(&self.root)?;
        Ok(report)
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        if self.worker.is_some()
            && let Err(error) = self.close()
        {
            eprintln!("zmux broker cleanup: {error}");
        }
    }
}

struct Attachment {
    broker: Broker,
    child: Option<OwnedChild>,
}
impl Drop for Attachment {
    fn drop(&mut self) {
        // Retain physical child ownership until every authenticated relay ends,
        // including all early-error paths. The child guard reaps afterward.
        if self.broker.worker.is_some()
            && let Err(error) = self.broker.close()
        {
            eprintln!("zmux attachment transport cleanup: {error}");
        }
    }
}

pub(super) fn connect(path: &std::path::Path, deadline: Instant) -> io::Result<UnixStream> {
    connect_while(path, deadline, || Ok(()))
}
pub(super) fn connect_while(
    path: &std::path::Path,
    deadline: Instant,
    mut tick: impl FnMut() -> io::Result<()>,
) -> io::Result<UnixStream> {
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.set_nonblocking(true)?;
    let address = socket2::SockAddr::unix(path)?;
    loop {
        tick()?;
        super::check_deadline(deadline, "Unix connect")?;
        match socket.connect(&address) {
            Ok(()) => return Ok(socket.into()),
            Err(error) if error.raw_os_error() == Some(nix::libc::EISCONN) => {
                return Ok(socket.into());
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(nix::libc::EAGAIN | nix::libc::EINPROGRESS | nix::libc::EALREADY)
                ) =>
            {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error),
        }
    }
}

fn acknowledge_completion(
    owner: &LocalZellij,
    record: &Record,
    receipt: &super::completion::TerminalReceiptV1,
    terminal: &mut ForegroundTerminal,
    observer: &mut Option<super::observer::Observer>,
    client: &mut super::completion_control::Client,
    mut tick: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    receipt.matches(record)?;
    let revision = if let Some(observer) = observer {
        observer.completed(receipt, Instant::now() + Duration::from_secs(10), || {
            terminal.startup_events()?;
            tick()
        })?
    } else {
        owner.consume_terminal(record, receipt)?;
        1
    };
    client.consumed(receipt, revision)
}

pub(super) fn attach(
    owner: &LocalZellij,
    record: Record,
    terminal: &mut ForegroundTerminal,
    mut observer: Option<super::observer::Observer>,
    startup_deadline: Instant,
) -> io::Result<i32> {
    terminal.startup_events()?;
    let mut completion = if record.completion.is_some() {
        let mut client = super::completion_control::Client::connect(&record, startup_deadline)?;
        let already = client.subscribe(
            observer.as_ref().map(|o| o.context()),
            observer.as_ref().map(|o| o.launcher()),
        )?;
        if let Some(receipt) = already {
            acknowledge_completion(
                owner,
                &record,
                &receipt,
                terminal,
                &mut observer,
                &mut client,
                || Ok(()),
            )?;
            terminal.restore()?;
            if let Some(observer) = &mut observer {
                observer.finished(
                    record.id,
                    FrontendReceipt {
                        owned_client_exit: ExitObservation::Exited(0),
                        requested_cancel: false,
                        original_owner_preserved: false,
                        workers_joined: true,
                        transport: TransportReport::default(),
                        owner_outcome: Some(
                            super::observer_protocol::OwnerOutcome::WorkloadCompleted(Box::new(
                                receipt.clone(),
                            )),
                        ),
                    },
                )?;
            }
            client.release()?;
            return receipt.workload_exit.mapped();
        }
        Some(client)
    } else {
        None
    };
    record.verify_live()?;
    let mut attachment = Attachment {
        broker: Broker::start(&record)?,
        child: None,
    };
    let config = record.runtime()?.join("config/config.kdl");
    let mut command = Command::new(&owner.binary);
    supervisor::environment(&mut command, &attachment.broker.root);
    command
        .arg("--config")
        .arg(config)
        .arg("attach")
        .arg(record.session_name())
        .args([
            "options",
            "--on-force-close",
            "detach",
            "--session-serialization",
            "false",
        ]);
    attachment.child = Some(OwnedChild::spawn(&mut command)?);
    let child = attachment.child.as_mut().expect("owned attachment child");
    let broker = &mut attachment.broker;
    let identity = process_identity(child.child.id())?;
    if identity.executable != owner.binary {
        return Err(fail("client executable changed after launch"));
    }
    *broker
        .expected
        .lock()
        .map_err(|_| fail("child binding poisoned"))? = Some(identity.clone());
    terminal.give_to(child)?;
    child.signal(nix::sys::signal::Signal::SIGCONT)?;
    let mut requested_cancel = false;
    let mut cancel_deadline = None;
    let mut connected = false;
    let mut completed = None;
    while child.observe(false)?.is_none() {
        if terminal.events(child, || {
            broker.cancel.store(true, Ordering::Release);
        })? {
            requested_cancel = true;
            cancel_deadline.get_or_insert(Instant::now() + Duration::from_secs(3));
        }
        if cancel_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            child.signal(nix::sys::signal::Signal::SIGKILL)?;
            return Err(fail(
                "client did not finish bounded cancellation; outcome unknown",
            ));
        }
        let report = broker
            .report
            .lock()
            .map_err(|_| fail("transport report poisoned"))?
            .clone();
        if completed.is_none()
            && let Some(client) = &mut completion
            && let Some(receipt) = client.query()?
        {
            acknowledge_completion(
                owner,
                &record,
                &receipt,
                terminal,
                &mut observer,
                client,
                || Ok(()),
            )?;
            broker.cancel.store(true, Ordering::Release);
            requested_cancel = true;
            child.signal(nix::sys::signal::Signal::SIGTERM)?;
            cancel_deadline.get_or_insert(Instant::now() + Duration::from_secs(3));
            completed = Some(receipt);
        }
        if completed.is_none()
            && let Some(observer) = &mut observer
        {
            observer.poll()?;
            if !connected {
                super::check_deadline(startup_deadline, "observer authenticated transport")?;
                if report.authenticated_connections > 0 {
                    observer.connected(record.id, &identity)?;
                    connected = true;
                }
            }
        }
        if !report.errors.is_empty() {
            return Err(fail(format!(
                "zellij relay failed; owner was not terminated; report={}",
                serde_json::to_string(&report).map_err(io::Error::other)?
            )));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let outcome = child
        .observe(false)?
        .ok_or_else(|| fail("missing owned child exit"))?;
    let report = broker.close()?;
    let preservation = record.verify_live();
    let reaped = child.reap()?;
    terminal.restore()?;
    if outcome != reaped {
        return Err(fail("owned child exit changed during cleanup"));
    }
    // This is a transport/lifecycle receipt, not a semantic attached proof.
    // In particular, a clean client exit cannot turn opaque rejected input into
    // a successful operation after the original workload has already ended.
    let receipt = FrontendReceipt {
        owned_client_exit: outcome,
        requested_cancel,
        original_owner_preserved: preservation.is_ok(),
        workers_joined: true,
        transport: report.clone(),
        owner_outcome: Some(match &completed {
            Some(receipt) => {
                super::observer_protocol::OwnerOutcome::WorkloadCompleted(Box::new(receipt.clone()))
            }
            None => super::observer_protocol::OwnerOutcome::LivePreserved,
        }),
    };
    if observer
        .as_ref()
        .is_none_or(|observer| observer.version() != 2)
    {
        eprintln!(
            "zmux local-zellij attachment receipt: {}",
            serde_json::to_string(&receipt).map_err(io::Error::other)?
        );
    }
    if let Some(observer) = &mut observer {
        observer.finished(record.id, receipt)?;
    }
    if !report.errors.is_empty() {
        return Err(fail(format!("relay failed: {:?}", report.errors)));
    }
    if report.authenticated_connections == 0 {
        return Err(fail("client ended without any authenticated transport"));
    }
    let completed_authenticated = if let Some(receipt) = &completed {
        super::completion::authenticate(&record, Instant::now() + Duration::from_secs(2))?.as_ref()
            == Some(receipt)
    } else {
        false
    };
    report.confirm_detach(
        requested_cancel,
        outcome,
        preservation.is_ok() || completed_authenticated,
    )?;
    if report.rejected_to_server > 0 {
        eprintln!(
            "zmux requested frontend detach rejected{} opaque bytes; delivery remains unknown and is never replayed; original live owner or exact completed publisher confirmed; receipt={:?}",
            report.rejected_to_server, report.input_loss
        );
    }
    if let Some(client) = &mut completion {
        client.release()?;
    }
    if let Some(receipt) = completed {
        return receipt.workload_exit.mapped();
    }
    match outcome {
        ExitObservation::Exited(code) => Ok(code),
        ExitObservation::Signaled(signal) => {
            Err(fail(format!("owned client terminated by signal{signal}")))
        }
        _ => Err(fail("owned client stopped without completing")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> (Connection, UnixStream, UnixStream) {
        let (client, source) = UnixStream::pair().unwrap();
        let (backend, target) = UnixStream::pair().unwrap();
        for stream in [&client, &source, &backend, &target] {
            stream.set_nonblocking(true).unwrap();
        }
        (
            Connection {
                client,
                backend,
                to_client: VecDeque::new(),
                to_server: VecDeque::new(),
                client_eof: false,
                backend_eof: false,
                client_closed: false,
                backend_closed: false,
                reverse_close: None,
            },
            source,
            target,
        )
    }

    #[test]
    fn real_socket_backpressure_preserves_order_and_half_close_with_bounded_buffers() {
        let (mut connection, mut source, mut target) = connection();
        socket2::SockRef::from(&connection.backend)
            .set_send_buffer_size(4096)
            .unwrap();
        let expected = (0..1024 * 1024)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let mut sent = 0;
        let mut report = TransportReport::default();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(5);
        while connection.to_server.len() < LIMIT {
            match source.write(&expected[sent..]) {
                Ok(count) => sent += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                other => panic!("source backpressure: {other:?}"),
            }
            assert!(!connection.pump(&mut report, &cancel).unwrap());
            assert!(
                Instant::now() < deadline,
                "actual socket never reached bounded relay backpressure"
            );
        }
        let mut interrupted = report.clone();
        interrupted.pending_to_server_on_cleanup = connection.to_server.len() as u64;
        assert!(
            interrupted
                .confirm_detach(true, ExitObservation::Exited(0), true)
                .is_err()
        );
        let mut received = Vec::new();
        let mut source_closed = false;
        let mut target_eof = false;
        while !target_eof {
            if sent < expected.len() {
                match source.write(&expected[sent..]) {
                    Ok(count) => sent += count,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                    other => panic!("source recovery: {other:?}"),
                }
            } else if !source_closed {
                source.shutdown(std::net::Shutdown::Write).unwrap();
                source_closed = true;
            }
            connection.pump(&mut report, &cancel).unwrap();
            let mut bytes = [0; 8192];
            match target.read(&mut bytes) {
                Ok(0) => target_eof = true,
                Ok(count) => received.extend_from_slice(&bytes[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                other => panic!("target recovery: {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "ordered relay drain exceeded bound"
            );
        }
        assert_eq!(received, expected);
        assert_eq!(report.peak_directional_bytes, LIMIT);
        assert_eq!(report.forwarded_to_server, expected.len() as u64);
        assert_eq!(report.rejected_to_server, 0);
        target.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(connection.pump(&mut report, &cancel).unwrap());
    }

    #[test]
    fn rejected_real_socket_input_retains_hash_and_pre_send_cancellation_phase() {
        for canceled in [false, true] {
            let (mut connection, mut source, _target) = connection();
            source.write_all(b"abc").unwrap();
            // Exercise a real rejected send without depending on peer-close
            // timing: Darwin can accept writes after the peer shuts down.
            // SHUT_WR rejects this writer even if concurrent forks inherit it.
            connection
                .backend
                .shutdown(std::net::Shutdown::Write)
                .unwrap();
            let mut report = TransportReport::default();
            let deadline = Instant::now() + Duration::from_secs(1);
            while !connection
                .pump(&mut report, &AtomicBool::new(canceled))
                .unwrap()
            {
                assert!(
                    Instant::now() < deadline,
                    "real rejected-input drain exceeded bound: {report:?}"
                );
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(report.read_from_client, 3);
            assert_eq!(report.forwarded_to_server, 0);
            assert_eq!(report.rejected_to_server, 3);
            assert_eq!(
                report.rejected_during_requested_cancel,
                if canceled { 3 } else { 0 }
            );
            assert_eq!(report.input_loss.len(), 1);
            let loss = &report.input_loss[0];
            assert_eq!(loss.bytes, 3);
            assert_eq!(
                loss.sha256,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
            assert_eq!(loss.requested_cancel_before_send, canceled);
            assert_eq!(loss.content, "opaque_not_interpreted");
            assert!(loss.errno.is_some());
            assert_eq!(
                report
                    .confirm_detach(true, ExitObservation::Exited(0), true)
                    .is_ok(),
                canceled
            );
            assert!(
                report
                    .confirm_detach(true, ExitObservation::Exited(0), false)
                    .is_err()
            );
            assert!(
                report
                    .confirm_detach(false, ExitObservation::Exited(0), true)
                    .is_err()
            );
            assert!(
                report
                    .confirm_detach(true, ExitObservation::Exited(7), true)
                    .is_err()
            );
        }
    }

    #[test]
    fn real_kernel_peers_reject_wrong_owned_child_before_connect_and_changed_backend_before_forwarding()
     {
        let mut sleeper = Command::new("/bin/sleep");
        sleeper.arg("10");
        let mut child = OwnedChild::spawn(&mut sleeper).unwrap();
        let expected = process_identity(child.child.id()).unwrap();
        let record = Record {
            completion: None,
            id: Uuid::new_v4(),
            name: "peer-negative".into(),
            state: "bound".into(),
            server: Some(expected.clone()),
            supervisor: None,
            workload: None,
            namespace: None,
        };
        let runtime = record.runtime().unwrap();
        files::directory(&runtime, true).unwrap();
        files::directory(&runtime.join("s"), true).unwrap();
        files::directory(&runtime.join("s/0.43.0"), true).unwrap();
        let backend = UnixListener::bind(record.backend().unwrap()).unwrap();
        backend.set_nonblocking(true).unwrap();
        let mut broker = Broker::start(&record).unwrap();
        *broker.expected.lock().unwrap() = Some(expected);
        let client =
            UnixStream::connect(broker.root.join("s/0.43.0").join(record.session_name())).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if broker.report.lock().unwrap().rejected_peers == 1 {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            backend.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "wrong PID opened a backend connection"
        );
        drop(client);
        // Admit this exact actual process on the client side. The listening
        // backend belongs to us, not the independently reserved sleep process.
        *broker.expected.lock().unwrap() = Some(process_identity(std::process::id()).unwrap());
        let mut client =
            UnixStream::connect(broker.root.join("s/0.43.0").join(record.session_name())).unwrap();
        client.write_all(b"must-not-forward").unwrap();
        loop {
            if !broker.report.lock().unwrap().errors.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let report = broker.close().unwrap();
        assert!(
            report
                .errors
                .iter()
                .any(|error| error == "backend connected peer identity changed")
        );
        assert_eq!(report.authenticated_connections, 0);
        assert_eq!(report.read_from_client, 0);
        assert_eq!(report.forwarded_to_server, 0);
        let (mut upstream, _) = backend.accept().unwrap();
        upstream.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match upstream.read(&mut [0; 64]) {
                Ok(0) => break,
                Ok(count) => panic!("rejected backend received{count} bytes"),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "rejected backend did not reach EOF"
                    );
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("rejected backend read failed: {error}"),
            }
        }
        child.cancel().unwrap();
        fs::remove_dir_all(runtime).unwrap();
    }
}
