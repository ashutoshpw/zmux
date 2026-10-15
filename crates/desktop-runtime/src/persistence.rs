//! Bounded, explicit session-file I/O. No autosave or implicit recovery selection.
//!
//! All path walking, serialization, syncing and advisory locking happens on one worker.
//! Paths must be absolute and contain no `..` or symlinks, including parent components.
//! Imported files must be regular, current-user-owned and private (0600 or stricter).
//! Saving uses an exclusive 0600 sibling, file sync, atomic commit, then directory sync.
//! A post-commit sync error may mean the new file is visible but durability is uncertain.
//!
//! A private 0700 state directory retains a bounded, fail-closed generation ledger.
//! Restoring reserves new session IDs plus a block for subsequent core allocations under
//! a descriptor lock. Fresh desktops and refills reserve blocks through `submit_reserve`.
//! The runtime must enforce the returned exclusive allocation end before publishing any
//! newly allocated object or process. The ledger is synced before returning a block;
//! failed/cancelled applications and crashes burn its unused IDs. The input
//! document is never rewritten. Corrupt documents/ledgers remain untouched for recovery.
//! All instances sharing identities must use the same state directory. Deleting/copying
//! that directory discards generation history; this is local state, not a distributed lock.
use rustix::fs::{self, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{RestoreLimits, SessionDocument, SessionId, ValidatedSessionDocument};

pub const MAX_DOCUMENT_BYTES: usize = 512 * 1024;
pub const MAX_PENDING_OPERATIONS: usize = 8;
pub const MAX_LEDGER_ENTRIES: usize = 128;
pub const ALLOCATION_BLOCK_IDS: u64 = 4096;
const MAX_LEDGER_BYTES: usize = 64 * 1024;
const LEDGER_NAME: &str = "session-generations.json";
const LOCK_NAME: &str = "session-generations.lock";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationId(pub u64);

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("persistence queue is full; operation was not accepted")]
    QueueFull,
    #[error("persistence worker is closed")]
    Closed,
    #[error("persistence operation ID space is exhausted")]
    OperationIdsExhausted,
    #[error("persistence operation cancelled during shutdown")]
    Cancelled,
    #[error("persistence shutdown deadline elapsed; worker may still be completing filesystem I/O")]
    ShutdownDeadline,
    #[error("persistence worker panicked")]
    WorkerPanicked,
    #[error("session path must be absolute with no parent traversal or symlinks")]
    InvalidPath,
    #[error("session save path is reserved for the persistence generation ledger or lock")]
    ReservedStatePath,
    #[error("{0} must be a private, current-user-owned regular file")]
    UnsafeFile(&'static str),
    #[error("persistence state directory must be a current-user-owned directory with mode 0700")]
    UnsafeStateDirectory,
    #[error("{0} exceeds its byte limit")]
    TooLarge(&'static str),
    #[error("session file already exists; explicit replacement is required")]
    AlreadyExists,
    #[error("session document does not exist")]
    NotFound,
    #[error("another instance is reserving session generations; retry")]
    LedgerBusy,
    #[error("session generation ledger is corrupt or has an unsupported version")]
    CorruptLedger,
    #[error("session generation ledger capacity is exhausted")]
    LedgerFull,
    #[error("session generation ID space is exhausted")]
    GenerationExhausted,
    #[error("generation reservation requires a nonzero allocator and identity namespace")]
    InvalidReservation,
    #[error("invalid session document: {0}")]
    InvalidDocument(String),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug)]
pub struct LoadedSession {
    pub document: ValidatedSessionDocument,
    pub session_ids: Vec<SessionId>,
    pub window_id: Option<zmux_workspace_core::WindowId>,
    pub allocation_start: u64,
    pub allocation_end: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationReservation {
    pub start: u64,
    pub end: u64,
}
#[derive(Debug)]
pub enum PersistenceOutcome {
    Loaded(LoadedSession),
    Reserved(AllocationReservation),
    Saved,
    Profiles(crate::profiles::Catalog),
}
#[derive(Debug)]
pub struct PersistenceCompletion {
    pub operation_id: OperationId,
    pub result: Result<PersistenceOutcome, PersistenceError>,
}

enum Request {
    Profiles {
        name: Option<String>,
    },
    Load {
        path: PathBuf,
    },
    Reserve {
        namespace: Uuid,
        minimum_id: u64,
    },
    Save {
        path: PathBuf,
        document: SessionDocument,
        replace: bool,
    },
}
struct Pending {
    id: OperationId,
    request: Request,
}

/// The caller polls completions; it never waits for filesystem work. Completion slots
/// count toward the same eight-operation limit as queued and executing requests.
/// Drop signals shutdown and detaches; call `join_until` only on a cleanup thread.
pub struct PersistenceWorker {
    requests: SyncSender<Pending>,
    completions: Receiver<PersistenceCompletion>,
    pending: Arc<AtomicUsize>,
    next_operation: AtomicU64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl PersistenceWorker {
    /// Starts the thread without inspecting or creating any filesystem path.
    pub fn start(state_path: PathBuf, limits: RestoreLimits) -> Result<Self, PersistenceError> {
        let (requests, receiver) = mpsc::sync_channel::<Pending>(MAX_PENDING_OPERATIONS);
        let (sender, completions) = mpsc::sync_channel(MAX_PENDING_OPERATIONS);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("zmux-session-files".into())
            .spawn(move || {
                loop {
                    let request = match receiver.recv_timeout(Duration::from_millis(20)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout)
                            if worker_stop.load(Ordering::Acquire) =>
                        {
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let result = if worker_stop.load(Ordering::Acquire) {
                        Err(PersistenceError::Cancelled)
                    } else {
                        match request.request {
                            Request::Profiles { name } => {
                                profile_catalog(&state_path, name).map(PersistenceOutcome::Profiles)
                            }
                            Request::Load { path } => {
                                load(&state_path, &path, limits).map(PersistenceOutcome::Loaded)
                            }
                            Request::Reserve {
                                namespace,
                                minimum_id,
                            } => state_directory(&state_path)
                                .and_then(|directory| {
                                    reserve_allocation(
                                        &directory,
                                        namespace,
                                        minimum_id,
                                        ALLOCATION_BLOCK_IDS,
                                    )
                                })
                                .map(PersistenceOutcome::Reserved),
                            Request::Save {
                                path,
                                document,
                                replace,
                            } => save(&state_path, &path, document, replace, limits)
                                .map(|()| PersistenceOutcome::Saved),
                        }
                    };
                    // Every accepted request owns a completion credit, including completed
                    // requests the caller has not polled. This send can never need to wait.
                    if sender
                        .try_send(PersistenceCompletion {
                            operation_id: request.id,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|source| PersistenceError::Io {
                operation: "start session persistence worker",
                source,
            })?;
        Ok(Self {
            requests,
            completions,
            pending: Arc::new(AtomicUsize::new(0)),
            next_operation: AtomicU64::new(1),
            stop,
            thread: Some(worker),
        })
    }
    pub fn submit_profiles(&self, name: Option<String>) -> Result<OperationId, PersistenceError> {
        self.submit(Request::Profiles { name })
    }
    pub fn submit_load(&self, path: PathBuf) -> Result<OperationId, PersistenceError> {
        self.submit(Request::Load { path })
    }
    /// Reserves a durable 4096-ID allocator block. No core allocation may exceed
    /// its exclusive end; a new reservation can skip unused IDs from other instances.
    pub fn submit_reserve(
        &self,
        namespace: Uuid,
        minimum_id: u64,
    ) -> Result<OperationId, PersistenceError> {
        self.submit(Request::Reserve {
            namespace,
            minimum_id,
        })
    }
    pub fn submit_save(
        &self,
        path: PathBuf,
        document: SessionDocument,
        replace: bool,
    ) -> Result<OperationId, PersistenceError> {
        self.submit(Request::Save {
            path,
            document,
            replace,
        })
    }
    fn submit(&self, request: Request) -> Result<OperationId, PersistenceError> {
        if self.stop.load(Ordering::Acquire) {
            return Err(PersistenceError::Closed);
        }
        self.pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value < MAX_PENDING_OPERATIONS).then_some(value + 1)
            })
            .map_err(|_| PersistenceError::QueueFull)?;
        let id =
            match self
                .next_operation
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                    value.checked_add(1)
                }) {
                Ok(value) => OperationId(value),
                Err(_) => {
                    self.pending.fetch_sub(1, Ordering::AcqRel);
                    return Err(PersistenceError::OperationIdsExhausted);
                }
            };
        match self.requests.try_send(Pending { id, request }) {
            Ok(()) => Ok(id),
            Err(error) => {
                self.pending.fetch_sub(1, Ordering::AcqRel);
                Err(match error {
                    TrySendError::Full(_) => PersistenceError::QueueFull,
                    TrySendError::Disconnected(_) => PersistenceError::Closed,
                })
            }
        }
    }
    pub fn try_completion(&self) -> Result<Option<PersistenceCompletion>, PersistenceError> {
        match self.completions.try_recv() {
            Ok(value) => {
                self.pending.fetch_sub(1, Ordering::AcqRel);
                Ok(Some(value))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(PersistenceError::Closed),
        }
    }
    pub fn request_shutdown(&self) {
        self.stop.store(true, Ordering::Release);
    }
    /// Explicit cleanup wait. Never call from UI/core execution. A deadline cannot
    /// interrupt an in-progress OS sync; on timeout Drop will detach the thread.
    pub fn join_until(&mut self, deadline: Instant) -> Result<(), PersistenceError> {
        self.request_shutdown();
        let Some(worker) = &self.thread else {
            return Ok(());
        };
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                return Err(PersistenceError::ShutdownDeadline);
            }
            thread::sleep(
                Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        self.thread
            .take()
            .expect("worker was present")
            .join()
            .map_err(|_| PersistenceError::WorkerPanicked)
    }
}
impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

fn io_error(operation: &'static str, source: impl Into<io::Error>) -> PersistenceError {
    PersistenceError::Io {
        operation,
        source: source.into(),
    }
}
fn open_directory(path: &Path) -> Result<File, PersistenceError> {
    if !path.is_absolute() {
        return Err(PersistenceError::InvalidPath);
    }
    let mut directory = File::from(
        fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("open filesystem root", e))?,
    );
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                directory = File::from(
                    fs::openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|e| io_error("open session parent directory without symlinks", e))?,
                );
            }
            _ => return Err(PersistenceError::InvalidPath),
        }
    }
    Ok(directory)
}
fn parent(path: &Path) -> Result<(File, OsString), PersistenceError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(PersistenceError::InvalidPath);
    }
    let name = path
        .file_name()
        .ok_or(PersistenceError::InvalidPath)?
        .to_os_string();
    let directory = open_directory(path.parent().ok_or(PersistenceError::InvalidPath)?)?;
    Ok((directory, name))
}
fn private_file(
    directory: &File,
    name: &OsStr,
    limit: usize,
    label: &'static str,
) -> Result<File, PersistenceError> {
    let file = File::from(
        fs::openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| io_error("open private session file", e))?,
    );
    check_file(&file, limit, label)?;
    Ok(file)
}
fn check_file(file: &File, limit: usize, label: &'static str) -> Result<(), PersistenceError> {
    let metadata = file
        .metadata()
        .map_err(|e| io_error("inspect private session file", e))?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7177 != 0
    {
        return Err(PersistenceError::UnsafeFile(label));
    }
    if metadata.len() > limit as u64 {
        return Err(PersistenceError::TooLarge(label));
    }
    Ok(())
}
fn read_bounded(
    file: File,
    limit: usize,
    label: &'static str,
) -> Result<Vec<u8>, PersistenceError> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read session file", e))?;
    if bytes.len() > limit {
        return Err(PersistenceError::TooLarge(label));
    }
    Ok(bytes)
}
fn state_directory(path: &Path) -> Result<File, PersistenceError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(PersistenceError::InvalidPath);
    }
    let mut directory = open_directory(Path::new("/"))?;
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                match fs::mkdirat(&directory, name, Mode::from_raw_mode(0o700)) {
                    Ok(()) => directory
                        .sync_all()
                        .map_err(|e| io_error("sync new persistence state directory", e))?,
                    Err(error) if error == rustix::io::Errno::EXIST => (),
                    Err(error) => {
                        return Err(io_error("create persistence state directory", error));
                    }
                }
                directory = File::from(
                    fs::openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|e| io_error("open private persistence state directory", e))?,
                );
            }
            _ => return Err(PersistenceError::InvalidPath),
        }
    }
    let metadata = directory
        .metadata()
        .map_err(|e| io_error("inspect persistence state directory", e))?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o7777 != 0o700 {
        return Err(PersistenceError::UnsafeStateDirectory);
    }
    Ok(directory)
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    #[serde(deserialize_with = "unique_next_ids")]
    next_ids: BTreeMap<String, u64>,
}
fn unique_next_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, u64>, D::Error> {
    struct Entries;
    impl<'de> serde::de::Visitor<'de> for Entries {
        type Value = BTreeMap<String, u64>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a bounded map of unique identity namespaces")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, u64>()? {
                if entries.len() >= MAX_LEDGER_ENTRIES || entries.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate or excess generation namespace",
                    ));
                }
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(Entries)
}
fn lock_ledger(directory: &File) -> Result<File, PersistenceError> {
    lock_named(directory, LOCK_NAME)
}
fn lock_named(directory: &File, name: &str) -> Result<File, PersistenceError> {
    // A concurrent first creation has returned ENOENT on a supported host.
    // Retry only this anchored open; persistent disappearance still fails closed.
    // No ledger access occurs until the opened descriptor passes validation and locks.
    let mut attempts = 0;
    let lock = loop {
        attempts += 1;
        match fs::openat(
            directory,
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(descriptor) => break File::from(descriptor),
            Err(error) if error == rustix::io::Errno::NOENT && attempts < 3 => {
                thread::yield_now();
            }
            Err(error) => return Err(io_error("open persistence generation lock", error)),
        }
    };
    check_file(&lock, 0, "generation lock")?;
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => PersistenceError::LedgerBusy,
        std::fs::TryLockError::Error(error) => io_error("lock persistence generations", error),
    })?;
    Ok(lock)
}
fn load(
    state: &Path,
    path: &Path,
    limits: RestoreLimits,
) -> Result<LoadedSession, PersistenceError> {
    // Create/verify local generation state on the worker even on first-launch absence.
    let directory = state_directory(state)?;
    let source = (|| {
        let (source_parent, name) = parent(path)?;
        private_file(
            &source_parent,
            &name,
            MAX_DOCUMENT_BYTES,
            "session document",
        )
    })()
    .map_err(|error| match error {
        PersistenceError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound => {
            PersistenceError::NotFound
        }
        other => other,
    })?;
    let bytes = read_bounded(source, MAX_DOCUMENT_BYTES, "session document")?;
    let document: SessionDocument = serde_json::from_slice(&bytes)
        .map_err(|e| PersistenceError::InvalidDocument(e.to_string()))?;
    let document = document
        .validate(limits)
        .map_err(|e| PersistenceError::InvalidDocument(e.to_string()))?;
    let reservation = reserve_allocation(
        &directory,
        document.identity_namespace(),
        document.next_id(),
        ALLOCATION_BLOCK_IDS
            + document.terminal_count() as u64
            + u64::from(document.requires_window_id()),
    )?;
    let session_end = reservation.start + document.terminal_count() as u64;
    let window_id = document
        .requires_window_id()
        .then_some(zmux_workspace_core::WindowId(session_end));
    Ok(LoadedSession {
        document,
        session_ids: (reservation.start..session_end).map(SessionId).collect(),
        window_id,
        allocation_start: reservation.start,
        allocation_end: reservation.end,
    })
}
fn reserve_allocation(
    directory: &File,
    namespace: Uuid,
    minimum_id: u64,
    count: u64,
) -> Result<AllocationReservation, PersistenceError> {
    if namespace.is_nil() || minimum_id == 0 {
        return Err(PersistenceError::InvalidReservation);
    }
    let _lock = lock_ledger(directory)?;
    let mut ledger = match private_file(
        directory,
        OsStr::new(LEDGER_NAME),
        MAX_LEDGER_BYTES,
        "generation ledger",
    ) {
        Ok(file) => serde_json::from_slice::<Ledger>(&read_bounded(
            file,
            MAX_LEDGER_BYTES,
            "generation ledger",
        )?)
        .map_err(|_| PersistenceError::CorruptLedger)?,
        Err(PersistenceError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Ledger {
                version: 1,
                ..Ledger::default()
            }
        }
        Err(error) => return Err(error),
    };
    if ledger.version != 1
        || ledger.next_ids.len() > MAX_LEDGER_ENTRIES
        || ledger.next_ids.iter().any(|(key, value)| {
            Uuid::parse_str(key).map_or(true, |id| id.is_nil() || id.to_string() != *key)
                || *value == 0
                || *value > u64::MAX - 4
        })
    {
        return Err(PersistenceError::CorruptLedger);
    }
    let namespace = namespace.to_string();
    if !ledger.next_ids.contains_key(&namespace) && ledger.next_ids.len() >= MAX_LEDGER_ENTRIES {
        return Err(PersistenceError::LedgerFull);
    }
    let start = minimum_id.max(ledger.next_ids.get(&namespace).copied().unwrap_or(1));
    let end = start
        .checked_add(count)
        .filter(|end| *end <= zmux_workspace_core::MAX_NEXT_IDENTIFIER)
        .ok_or(PersistenceError::GenerationExhausted)?;
    ledger.next_ids.insert(namespace, end);
    let bytes = serialize_bounded(&ledger, MAX_LEDGER_BYTES, "generation ledger")?;
    atomic_write(directory, OsStr::new(LEDGER_NAME), &bytes, true)?;
    Ok(AllocationReservation { start, end })
}

fn save(
    state: &Path,
    path: &Path,
    document: SessionDocument,
    replace: bool,
    limits: RestoreLimits,
) -> Result<(), PersistenceError> {
    document
        .clone()
        .validate(limits)
        .map_err(|e| PersistenceError::InvalidDocument(e.to_string()))?;
    let bytes = serialize_bounded(&document, MAX_DOCUMENT_BYTES, "session document")?;
    let state = state_directory(state)?;
    let (directory, name) = parent(path)?;
    if name == LOCK_NAME
        || name == LEDGER_NAME
        || name == crate::profiles::CATALOG_NAME
        || name == "browser-profiles.lock"
    {
        let destination = directory
            .metadata()
            .map_err(|e| io_error("inspect save parent", e))?;
        let state = state
            .metadata()
            .map_err(|e| io_error("inspect state directory", e))?;
        if destination.dev() == state.dev() && destination.ino() == state.ino() {
            return Err(PersistenceError::ReservedStatePath);
        }
    }
    atomic_write(&directory, &name, &bytes, replace)
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("document size limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialize_bounded(
    value: &impl Serialize,
    limit: usize,
    label: &'static str,
) -> Result<Vec<u8>, PersistenceError> {
    let mut output = BoundedBytes {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| PersistenceError::TooLarge(label))?;
    Ok(output.bytes)
}
fn atomic_write(
    directory: &File,
    name: &OsStr,
    bytes: &[u8],
    replace: bool,
) -> Result<(), PersistenceError> {
    // Validate an existing destination before replacing it. All operations remain
    // anchored to this directory descriptor even if the parent path is renamed.
    match private_file(
        directory,
        name,
        MAX_DOCUMENT_BYTES.max(MAX_LEDGER_BYTES),
        "save destination",
    ) {
        Ok(_) if !replace => return Err(PersistenceError::AlreadyExists),
        Ok(_) => (),
        Err(PersistenceError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let temporary = format!(".zmux-session-{}.tmp", Uuid::new_v4());
    let mut file = File::from(
        fs::openat(
            directory,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|e| io_error("create exclusive private session temporary file", e))?,
    );
    let result = (|| {
        file.write_all(bytes)
            .map_err(|e| io_error("write session temporary file", e))?;
        file.sync_all()
            .map_err(|e| io_error("sync session temporary file", e))?;
        if replace {
            fs::renameat(directory, temporary.as_str(), directory, name)
                .map_err(|e| io_error("atomically replace session file", e))?;
        } else {
            // Same-directory hard-link insertion is an atomic no-replace commit on
            // both macOS and Linux. Unlike an existence check, it cannot overwrite.
            fs::linkat(
                directory,
                temporary.as_str(),
                directory,
                name,
                AtFlags::empty(),
            )
            .map_err(|error| {
                if error == rustix::io::Errno::EXIST {
                    PersistenceError::AlreadyExists
                } else {
                    io_error("atomically create session file without replacement", error)
                }
            })?;
            fs::unlinkat(directory, temporary.as_str(), AtFlags::empty())
                .map_err(|e| io_error("remove committed session temporary name", e))?;
        }
        directory.sync_all().map_err(|e| {
            io_error(
                "sync committed session directory (file may already be visible)",
                e,
            )
        })
    })();
    if result.is_err() {
        match fs::unlinkat(directory, temporary.as_str(), AtFlags::empty()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => (),
            Err(error) => return Err(io_error("clean up failed session temporary file", error)),
        }
    }
    result
}

fn profile_catalog(
    state: &Path,
    name: Option<String>,
) -> Result<crate::profiles::Catalog, PersistenceError> {
    use crate::profiles::{CATALOG_NAME, Catalog, MAX_PROFILE_BYTES};
    let directory = state_directory(state)?;
    let _lock = lock_named(&directory, "browser-profiles.lock")?;
    let mut catalog = match private_file(
        &directory,
        OsStr::new(CATALOG_NAME),
        MAX_PROFILE_BYTES,
        "browser profile catalog",
    ) {
        Ok(file) => serde_json::from_slice::<Catalog>(&read_bounded(
            file,
            MAX_PROFILE_BYTES,
            "browser profile catalog",
        )?)
        .map_err(|error| {
            PersistenceError::InvalidDocument(format!("browser profile catalog: {error}"))
        })?,
        Err(PersistenceError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Catalog::default()
        }
        Err(error) => return Err(error),
    };
    catalog
        .validate()
        .map_err(PersistenceError::InvalidDocument)?;
    if let Some(name) = name {
        catalog
            .add(name)
            .map_err(PersistenceError::InvalidDocument)?;
        let bytes = serialize_bounded(&catalog, MAX_PROFILE_BYTES, "browser profile catalog")?;
        atomic_write(&directory, OsStr::new(CATALOG_NAME), &bytes, true)?;
    }
    Ok(catalog)
}
