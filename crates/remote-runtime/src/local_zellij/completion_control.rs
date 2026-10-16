use super::{
    Record,
    completion::{self, CONTROL_LIMIT, CommittedBinding, ExecutionBinding, TerminalReceipt},
    fail, files,
    observer_protocol::Context,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};
const MAX_CONNECTIONS: usize = 12;
const MAX_SUBSCRIBERS: usize = 4;
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Query,
    Subscribe {
        context: Option<Context>,
        launcher: Option<ProcessIdentity>,
    },
    Release,
    Consumed {
        receipt_digest: String,
        document_revision: u64,
        context: Option<Context>,
    },
    Retire {
        receipt_digest: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_contract: Option<super::admission::ExecutionContract>,
    owner_uuid: Uuid,
    owner_generation: Uuid,
    binding_digest: String,
    challenge: Uuid,
    operation: Operation,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_contract: Option<super::admission::ExecutionContract>,
    challenge: Uuid,
    binding_digest: String,
    receipt: Option<TerminalReceipt>,
    consumed: bool,
    error: Option<String>,
    retiring: bool,
}
fn contract(binding: &ExecutionBinding) -> Option<super::admission::ExecutionContract> {
    if binding.contract_version() == 2 {
        Some(super::admission::ExecutionContract::AdmittedLifetimeV2)
    } else {
        None
    }
}

fn frame<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.is_empty() || body.len() > CONTROL_LIMIT {
        return Err(fail("completion control frame exceeds bound"));
    }
    let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
    bytes.extend(body);
    Ok(bytes)
}
fn receive<T: for<'a> Deserialize<'a>>(stream: &mut UnixStream) -> io::Result<T> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header) as usize;
    if !(1..=CONTROL_LIMIT).contains(&size) {
        return Err(fail("invalid completion frame bound"));
    }
    let mut body = vec![0; size];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(io::Error::other)
}
fn private_socket(record: &Record, binding: &ExecutionBinding) -> io::Result<PathBuf> {
    let root = record.runtime()?;
    files::validate_directory(&root, true)?;
    if files::inode(&root)? != binding.namespace().root
        || files::inode(&record.backend()?)? != binding.namespace().socket
    {
        return Err(fail("reserved namespace/socket was replaced"));
    }
    let path = root.join("completion.sock");
    let info = path.symlink_metadata()?;
    use std::os::unix::fs::FileTypeExt;
    if !info.file_type().is_socket()
        || info.uid() != binding.supervisor().uid
        || info.mode() & 0o077 != 0
        || (info.dev(), info.ino()) != binding.completion_socket()
    {
        return Err(fail("completion publisher socket replaced"));
    }
    Ok(path)
}
fn live(record: &Record, binding: &ExecutionBinding) -> io::Result<()> {
    if process_identity(binding.supervisor().pid)? != *binding.supervisor()
        || process_identity(binding.server().pid)? != *binding.server()
    {
        return Err(fail("original completion publisher identity changed"));
    }
    private_socket(record, binding)?;
    Ok(())
}
pub(super) struct Client {
    stream: UnixStream,
    record: Record,
    binding: ExecutionBinding,
    subscribed: bool,
    context: Option<Context>,
}
impl Client {
    pub fn connect(record: &Record, deadline: Instant) -> io::Result<Self> {
        let binding = record.execution_binding()?;
        live(record, &binding)?;
        let path = private_socket(record, &binding)?;
        let stream = super::broker::connect(&path, deadline)?;
        stream.set_nonblocking(false)?;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(2));
        if remaining.is_zero() {
            return Err(fail("completion query deadline"));
        }
        stream.set_read_timeout(Some(remaining))?;
        stream.set_write_timeout(Some(remaining))?;
        if super::broker::peer(&stream)? != *binding.supervisor() {
            return Err(fail("completion peer is not original supervisor"));
        }
        Ok(Self {
            stream,
            record: record.clone(),
            binding,
            subscribed: false,
            context: None,
        })
    }
    fn request(&mut self, operation: Operation) -> io::Result<Response> {
        live(&self.record, &self.binding)?;
        let challenge = Uuid::new_v4();
        let request = Request {
            version: self.binding.contract_version(),
            execution_contract: contract(&self.binding),
            owner_uuid: self.binding.owner_uuid(),
            owner_generation: self.binding.owner_generation(),
            binding_digest: self.binding.digest()?,
            challenge,
            operation,
        };
        self.stream.write_all(&frame(&request)?)?;
        let response: Response = receive(&mut self.stream)?;
        if response.version != self.binding.contract_version()
            || response.execution_contract != contract(&self.binding)
            || response.challenge != challenge
            || response.binding_digest != self.binding.digest()?
        {
            return Err(fail("completion response correlation rejected"));
        }
        if let Some(error) = response.error {
            return Err(fail(error));
        }
        if let Some(receipt) = &response.receipt {
            receipt.matches(&self.record)?;
        }
        live(&self.record, &self.binding)?;
        Ok(response)
    }
    pub fn state(&mut self) -> io::Result<(Option<TerminalReceipt>, bool)> {
        let response = self.request(Operation::Query)?;
        Ok((response.receipt, response.consumed))
    }
    pub fn query(&mut self) -> io::Result<Option<TerminalReceipt>> {
        Ok(self.state()?.0)
    }
    pub fn subscribe(
        &mut self,
        context: Option<Context>,
        launcher: Option<ProcessIdentity>,
    ) -> io::Result<Option<TerminalReceipt>> {
        let response = self.request(Operation::Subscribe {
            context: context.clone(),
            launcher,
        })?;
        self.subscribed = true;
        self.context = context;
        Ok(response.receipt)
    }
    pub fn release(&mut self) -> io::Result<()> {
        if self.subscribed {
            self.request(Operation::Release)?;
            self.subscribed = false;
        }
        Ok(())
    }
    pub fn consumed(&mut self, receipt: &TerminalReceipt, revision: u64) -> io::Result<()> {
        let response = self.request(Operation::Consumed {
            receipt_digest: receipt.digest()?,
            document_revision: revision,
            context: self.context.clone(),
        })?;
        if !response.consumed {
            return Err(fail("completion consumption not confirmed"));
        }
        Ok(())
    }
    pub fn retire(&mut self, receipt: &TerminalReceipt) -> io::Result<()> {
        let challenge = Uuid::new_v4();
        let request = Request {
            version: self.binding.contract_version(),
            execution_contract: contract(&self.binding),
            owner_uuid: self.binding.owner_uuid(),
            owner_generation: self.binding.owner_generation(),
            binding_digest: self.binding.digest()?,
            challenge,
            operation: Operation::Retire {
                receipt_digest: receipt.digest()?,
            },
        };
        live(&self.record, &self.binding)?;
        self.stream.write_all(&frame(&request)?)?;
        // Retire is the only response whose publisher may self-exit immediately.
        let response: Response = receive(&mut self.stream)?;
        if response.version != self.binding.contract_version()
            || response.execution_contract != contract(&self.binding)
            || response.challenge != challenge
            || response.binding_digest != self.binding.digest()?
            || !response.retiring
            || !response.consumed
            || response
                .receipt
                .as_ref()
                .is_none_or(|r| r.digest().ok() != receipt.digest().ok())
            || response.error.is_some()
        {
            return Err(fail("completion retirement not confirmed"));
        }
        Ok(())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if self.subscribed {
            let _ = self.release();
        }
    }
}
struct Connection {
    stream: UnixStream,
    peer: ProcessIdentity,
    input: Vec<u8>,
    output: Vec<u8>,
    subscribed: bool,
    context: Option<Context>,
    close: bool,
}
pub(super) struct Publisher {
    listener: UnixListener,
    endpoint: (u64, u64),
    root: PathBuf,
    directory: fs::File,
    profile: PathBuf,
    binding: Option<ExecutionBinding>,
    connections: Vec<Connection>,
    receipt: Option<TerminalReceipt>,
    publication_error: Option<String>,
    consumed: Option<String>,
    retiring: bool,
}
impl Publisher {
    pub fn create(root: PathBuf, profile: PathBuf) -> io::Result<Self> {
        let directory = fs::File::from(rustix::fs::open(
            &root,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?);
        let path = root.join("completion.sock");
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            endpoint: files::inode(&path)?,
            directory,
            listener,
            root,
            profile,
            binding: None,
            connections: Vec::new(),
            receipt: None,
            publication_error: None,
            consumed: None,
            retiring: false,
        })
    }
    pub fn endpoint(&self) -> (u64, u64) {
        self.endpoint
    }
    pub fn bind(&mut self, binding: CommittedBinding) -> io::Result<()> {
        self.bind_execution(ExecutionBinding::V1(binding))
    }
    pub fn bind_execution(&mut self, binding: ExecutionBinding) -> io::Result<()> {
        binding.validate()?;
        if binding.completion_socket() != self.endpoint
            || *binding.supervisor() != process_identity(std::process::id())?
            || *binding.server() != process_identity(binding.supervisor().parent_pid)?
            || files::inode(&self.root)? != binding.namespace().root
        {
            return Err(fail("supervisor committed binding changed"));
        }
        self.binding = Some(binding);
        Ok(())
    }
    pub fn publish(&mut self, status: completion::WorkloadExit) -> io::Result<()> {
        self.publish_observed(status, super::admission::ExecutionObservation::NotObserved)
    }
    pub fn publish_observed(
        &mut self,
        status: completion::WorkloadExit,
        observation: super::admission::ExecutionObservation,
    ) -> io::Result<()> {
        let binding = self
            .binding
            .clone()
            .ok_or_else(|| fail("uncommitted supervisor cannot publish completion"))?;
        let receipt = match &binding {
            ExecutionBinding::V1(v) => TerminalReceipt::V1(completion::TerminalReceiptV1 {
                version: 1,
                domain: completion::DOMAIN.into(),
                binding: v.clone(),
                binding_digest: v.digest()?,
                terminal_sequence: 1,
                workload_exit: status,
                child_reaped: true,
            }),
            ExecutionBinding::V2(v) => TerminalReceipt::V2(super::admission::TerminalReceiptV2 {
                version: 2,
                domain: super::admission::TERMINAL_DOMAIN.into(),
                binding: v.clone(),
                binding_digest: v.digest()?,
                terminal_sequence: 1,
                workload_exit: status,
                child_reaped: true,
                execution_observation: observation,
            }),
        };
        receipt.validate()?;
        let body = serde_json::to_vec(&receipt).map_err(io::Error::other)?;
        let current = self.directory.metadata()?;
        if (current.dev(), current.ino()) != binding.namespace().root
            || files::inode(&self.root)? != binding.namespace().root
        {
            return Err(fail("retained terminal publication directory changed"));
        }
        match publish_at(
            &self.directory,
            if binding.contract_version() == 1 {
                "terminal-v1.json"
            } else {
                "terminal-v2.json"
            },
            &body,
        ) {
            Ok(()) => self.receipt = Some(receipt),
            Err(e) => {
                self.publication_error = Some(e.to_string());
                return Err(e);
            }
        }
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<bool> {
        for _ in 0..MAX_CONNECTIONS {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    if self.connections.len() >= MAX_CONNECTIONS {
                        continue;
                    }
                    if let Ok(peer) = super::broker::peer(&stream)
                        && peer.uid == nix::unistd::geteuid().as_raw()
                    {
                        self.connections.push(Connection {
                            stream,
                            peer,
                            input: Vec::new(),
                            output: Vec::new(),
                            subscribed: false,
                            context: None,
                            close: false,
                        });
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        let Some(binding) = &self.binding else {
            return Ok(false);
        };
        let binding = binding.clone();
        let mut subscribed = self
            .connections
            .iter()
            .filter(|c| c.subscribed && !c.close)
            .count();
        for connection in &mut self.connections {
            if !connection.output.is_empty() {
                match connection.stream.write(&connection.output) {
                    Ok(0) => connection.close = true,
                    Ok(n) => {
                        connection.output.drain(..n);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                    Err(_) => connection.close = true,
                }
                if !connection.output.is_empty() {
                    continue;
                }
            }
            let mut buffer = [0; 2048];
            match connection.stream.read(&mut buffer) {
                Ok(0) => connection.close = true,
                Ok(n) => connection.input.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(_) => connection.close = true,
            }
            if connection.close {
                if connection.subscribed {
                    subscribed -= 1;
                    connection.subscribed = false;
                }
                continue;
            }
            if connection.input.len() > CONTROL_LIMIT + 4 {
                connection.close = true;
                continue;
            }
            if connection.input.len() < 4 {
                continue;
            }
            let length =
                u32::from_le_bytes(connection.input[..4].try_into().expect("four bytes")) as usize;
            if !(1..=CONTROL_LIMIT).contains(&length) {
                connection.close = true;
                continue;
            }
            if connection.input.len() < 4 + length {
                continue;
            }
            let request: Request = match serde_json::from_slice(&connection.input[4..4 + length]) {
                Ok(v) => v,
                Err(_) => {
                    connection.close = true;
                    continue;
                }
            };
            connection.input.drain(..4 + length);
            let mut error = None;
            if request.version != binding.contract_version()
                || request.execution_contract != contract(&binding)
                || request.challenge.is_nil()
                || request.owner_uuid != binding.owner_uuid()
                || request.owner_generation != binding.owner_generation()
                || request.binding_digest != binding.digest()?
            {
                error = Some("completion request correlation rejected".into());
            } else {
                let writer = connection.peer.executable == binding.supervisor().executable
                    && process_identity(connection.peer.pid)
                        .is_ok_and(|peer| peer == connection.peer);
                match request.operation {
                    Operation::Query => (),
                    Operation::Subscribe { context, launcher } => {
                        let observer_link = match (&context, &launcher) {
                            (None, None) => true,
                            (Some(context), Some(launcher)) => {
                                context.validate().is_ok()
                                    && connection.peer.parent_pid == launcher.pid
                                    && process_identity(launcher.pid)
                                        .is_ok_and(|current| current == *launcher)
                            }
                            _ => false,
                        };
                        if !writer
                            || self.retiring
                            || connection.subscribed
                            || subscribed >= MAX_SUBSCRIBERS
                            || !observer_link
                        {
                            error = Some("completion subscription denied or full".into());
                        } else {
                            connection.subscribed = true;
                            connection.context = context;
                            subscribed += 1;
                        }
                    }
                    Operation::Release => {
                        if connection.subscribed {
                            connection.subscribed = false;
                            subscribed -= 1;
                        }
                        connection.context = None;
                    }
                    Operation::Consumed {
                        receipt_digest,
                        document_revision,
                        context,
                    } => {
                        let matches = self
                            .receipt
                            .as_ref()
                            .is_some_and(|r| r.digest().is_ok_and(|d| d == receipt_digest));
                        let local = consumed_path(&self.profile, binding.owner_uuid());
                        let local_proof = files::read_private(&local, completion::TERMINAL_LIMIT)
                            .ok()
                            .and_then(|b| serde_json::from_slice::<TerminalReceipt>(&b).ok())
                            .is_some_and(|r| {
                                r.digest().is_ok_and(|d| d == receipt_digest)
                                    && r.execution_binding() == binding
                            });
                        let authorized = connection.subscribed
                            && connection.context == context
                            && (connection.context.is_some() && document_revision > 0
                                || connection.context.is_none() && local_proof);
                        if !writer || !matches || !authorized {
                            error = Some("completion durable consumption denied".into());
                        } else {
                            self.consumed = Some(receipt_digest);
                        }
                    }
                    Operation::Retire { receipt_digest } => {
                        let local = files::read_private(
                            &consumed_path(&self.profile, binding.owner_uuid()),
                            completion::TERMINAL_LIMIT,
                        )
                        .ok()
                        .and_then(|b| serde_json::from_slice::<TerminalReceipt>(&b).ok())
                        .is_some_and(|r| {
                            r.execution_binding() == binding
                                && r.digest().is_ok_and(|d| d == receipt_digest)
                        });
                        if !writer
                            || !local
                            || self
                                .receipt
                                .as_ref()
                                .is_none_or(|r| r.digest().ok().as_ref() != Some(&receipt_digest))
                        {
                            error = Some(
                                "completed cleanup requires exact local durable consumption".into(),
                            );
                        } else {
                            self.retiring = true;
                            if subscribed > 0 {
                                self.retiring = false;
                                error = Some(
                                    "completed owner cleanup busy: subscribed frontends remain"
                                        .into(),
                                );
                            } else {
                                self.consumed = Some(receipt_digest);
                            }
                        }
                    }
                }
            }
            if error.is_none() {
                error = self.publication_error.clone();
            }
            connection.output = frame(&Response {
                version: binding.contract_version(),
                execution_contract: contract(&binding),
                challenge: request.challenge,
                binding_digest: binding.digest()?,
                receipt: self.receipt.clone(),
                consumed: self.consumed.is_some(),
                error,
                retiring: self.retiring && subscribed == 0,
            })?;
        }
        self.connections.retain(|c| !c.close);
        Ok(
            self.retiring
                && subscribed == 0
                && self.connections.iter().all(|c| c.output.is_empty()),
        )
    }
}
pub(super) fn consumed_path(profile: &std::path::Path, owner: Uuid) -> PathBuf {
    profile.join(format!("consumed-{owner}.json"))
}
pub(super) fn publish(root: &std::path::Path, name: &str, body: &[u8]) -> io::Result<()> {
    use rustix::fs::{self as at, Mode, OFlags};
    let directory = fs::File::from(at::open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    publish_at(&directory, name, body)
}
fn publish_at(directory: &fs::File, name: &str, body: &[u8]) -> io::Result<()> {
    use rustix::fs::{self as at, AtFlags, Mode, OFlags};
    let temporary = format!("pending-{}", Uuid::new_v4());
    let mut file = fs::File::from(at::openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    let result = (|| {
        file.write_all(body)?;
        file.sync_all()?;
        match at::linkat(
            directory,
            temporary.as_str(),
            directory,
            name,
            AtFlags::empty(),
        ) {
            Ok(()) => (),
            Err(rustix::io::Errno::EXIST) => {
                let mut existing = fs::File::from(at::openat(
                    directory,
                    name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
                let info = existing.metadata()?;
                if !info.is_file()
                    || info.uid() != nix::unistd::geteuid().as_raw()
                    || info.mode() & 0o077 != 0
                    || info.len() > completion::TERMINAL_LIMIT as u64
                {
                    return Err(fail("unsafe existing terminal receipt"));
                }
                let mut bytes = Vec::new();
                existing.read_to_end(&mut bytes)?;
                if bytes != body {
                    return Err(fail("conflicting immutable terminal publication"));
                }
            }
            Err(e) => return Err(e.into()),
        }
        directory.sync_all()
    })();
    let removed =
        at::unlinkat(directory, temporary.as_str(), AtFlags::empty()).map_err(io::Error::from);
    result.and(removed)
}
pub(super) fn authenticate(
    record: &Record,
    deadline: Instant,
) -> io::Result<Option<TerminalReceipt>> {
    Client::connect(record, deadline)?.query()
}
