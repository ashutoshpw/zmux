//! Bounded file service: one `zmux-files` worker owns all user-file I/O (probe, load, save and
//! fingerprint polling). The workspace executor and every UI/bridge caller only exchange small
//! bounded messages with it; nothing here is ever awaited on the executor.
//!
//! Ownership is fenced like the browser runtime: a view binds to a surface with a host UUID and
//! every mutating call carries the full [`FileOwner`]. A save is re-fenced on the worker
//! immediately before its irreversible rename. Unknown outcomes are reported, recorded in the
//! snapshot and never replayed.
use crate::{
    DesktopError, Worker,
    file_io::{
        self, Conflict, FileBackend, FileError, Fingerprint, MAX_TEXT_BYTES, Observation, Probe,
        RealFs,
    },
    targets::TargetRequest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_workspace_core::{
    Action, FileMode, MAX_SURFACES_PER_PANE, PaneId, Snapshot as Model, SurfaceId, SurfaceKind,
    targets::{CallerContext, TargetKind, TargetScope},
};

/// Pending worker jobs. One more may be running. Overload is an explicit `busy`.
pub const QUEUE_BOUND: usize = 8;
/// Most entries and `last_op` receipts the snapshot publishes.
pub const MAX_SNAPSHOT_ENTRIES: usize = 16;
/// Closed owners remembered so late calls read `stale_owner`, not `unknown_surface`.
pub const RETIRED_OWNERS: usize = 256;
/// Most files examined per poll tick.
pub const POLL_BATCH: usize = 4;
/// Most operands of one `open`.
pub const MAX_OPEN_ITEMS: usize = 16;

/// Production values are the defaults. Tests inject a backend and short intervals through
/// `DesktopOptions::files`; the reported capabilities are derived from the same values.
#[derive(Clone)]
pub struct FilesConfig {
    pub backend: Arc<dyn FileBackend>,
    /// Per attached entry.
    pub poll_interval: Duration,
    /// Worker wake-up slice; jobs always run before polls.
    pub poll_tick: Duration,
    /// A job running longer than this marks the worker stalled.
    pub stall_after: Duration,
    /// Total deadline of one `open` probe batch.
    pub probe_timeout: Duration,
    /// Wait for attach/reload/observe replies.
    pub read_timeout: Duration,
    pub save_timeout: Duration,
    /// Wait for the executor to acknowledge a file command (applying an `open`, a view's dirty
    /// bit or detach). An elapsed wait is reported as an unknown outcome.
    pub command_timeout: Duration,
    /// Called once per executor tick. Tests use it to hold the executor deterministically;
    /// production hosts leave it `None`.
    pub executor_tick_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            backend: Arc::new(RealFs),
            poll_interval: Duration::from_millis(1000),
            poll_tick: Duration::from_millis(250),
            stall_after: Duration::from_secs(5),
            probe_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(2),
            save_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(2),
            executor_tick_hook: None,
        }
    }
}

/// Identifies one native view instance bound to one surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileOwner {
    pub epoch: Uuid,
    pub identity_namespace: Uuid,
    pub surface_id: SurfaceId,
    pub host_id: Uuid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConflictInfo {
    /// `modified` or `deleted`.
    pub kind: String,
    pub disk_sha256: Option<String>,
    pub disk_size: Option<u64>,
}
impl From<Conflict> for ConflictInfo {
    fn from(conflict: Conflict) -> Self {
        Self {
            kind: conflict.kind.into(),
            disk_sha256: conflict.disk_sha256,
            disk_size: conflict.disk_size,
        }
    }
}
/// A refused or failed file operation. Messages never contain file content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileFailure {
    pub code: String,
    pub message: String,
    pub op_id: Option<u64>,
    /// True only when the result could not be confirmed. Never replay such an operation.
    pub outcome_unknown: bool,
    /// Boxed so a failure stays small inside every `Result` on the hot paths.
    pub conflict: Option<Box<ConflictInfo>>,
}
impl FileFailure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            op_id: None,
            outcome_unknown: false,
            conflict: None,
        }
    }
    fn with_op(mut self, op_id: u64) -> Self {
        self.op_id = Some(op_id);
        self
    }
}
impl From<&FileError> for FileFailure {
    fn from(error: &FileError) -> Self {
        Self::new(error.code, error.message.clone())
    }
}
impl std::fmt::Display for FileFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for FileFailure {}

/// A successful attach or reload. `body` is the file text (BOM removed) or preview bytes.
#[derive(Clone, Debug)]
pub struct FileContent {
    pub op_id: u64,
    pub owner: FileOwner,
    pub mode: FileMode,
    pub size: u64,
    pub generation: u64,
    pub sha256: String,
    pub mtime_ns: i64,
    pub writable: bool,
    pub bom: bool,
    pub body: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveReceipt {
    pub op_id: u64,
    pub generation: u64,
    pub sha256: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// Trusted native view events. Never constructible from a socket or JSON.
#[derive(Clone, Debug)]
pub enum FileHostEvent {
    Observed { owner: FileOwner, dirty: bool },
    Detach { owner: FileOwner },
}
/// One operand of an `open` after the off-executor lexical and filesystem checks.
pub enum OpenPlan {
    File {
        operand: String,
        probe: Result<Probe, FileFailure>,
    },
    Url {
        url: String,
    },
    /// Refused before any probe (unsupported scheme, malformed URL, wrong kind for the command).
    Invalid {
        kind: &'static str,
        operand: String,
        failure: FileFailure,
    },
}
/// The probed operands of one `open`, applied by the executor strictly in order. The target is
/// resolved once, against one model revision, before the first item changes anything.
pub struct OpenBatch {
    pub focus: bool,
    pub scope: TargetScope,
    pub caller: CallerContext,
    pub expected_model_revision: Option<u64>,
    pub items: Vec<OpenPlan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Unattached,
    InSync,
    Changed,
    Deleted,
    Unreadable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    Pending,
    Completed,
    Failed,
    OutcomeUnknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpReceipt {
    pub op_id: u64,
    pub kind: &'static str,
    pub state: OpState,
    pub code: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntrySnapshot {
    pub surface_id: SurfaceId,
    pub state: EntryState,
    pub attached: bool,
    pub generation: u64,
    pub disk_generation: u64,
    pub size: u64,
    pub mtime_ns: i64,
    pub error_code: Option<String>,
    pub last_op: Option<OpReceipt>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerSnapshot {
    pub state: &'static str,
    /// Whole seconds in milliseconds, so a hung volume costs at most one revision per second.
    pub stalled_ms: u64,
}
/// Published in every desktop snapshot. Never contains file bytes or hashes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FilesSnapshot {
    pub epoch: Uuid,
    pub worker: WorkerSnapshot,
    pub entries: Vec<EntrySnapshot>,
}

struct Baseline {
    sha256: String,
    bom: bool,
}
#[derive(Clone, Debug)]
struct Analyzed {
    fingerprint: Fingerprint,
    state: EntryState,
    code: Option<&'static str>,
}
struct Entry {
    path: String,
    mode: FileMode,
    /// Mirror of the model's trusted dirty bit, refreshed by `reconcile`.
    model_dirty: bool,
    host: Option<Uuid>,
    state: EntryState,
    error_code: Option<&'static str>,
    baseline: Option<Baseline>,
    /// Hash of the disk content the poller last examined (the baseline until it looks again,
    /// `None` while the file is gone). `disk_generation` advances only for content that differs
    /// from it, so a second look at the same external edit never counts as another edit.
    disk_sha: Option<String>,
    generation: u64,
    disk_generation: u64,
    size: u64,
    mtime_ns: i64,
    analyzed: Option<Analyzed>,
    pending: Option<Fingerprint>,
    missing_polls: u8,
    next_poll: Instant,
    in_flight: Option<u64>,
    /// Bumped by every detach so an operation admitted before it can tell its view went away.
    detaches: u64,
    last_op: Option<OpReceipt>,
}
impl Entry {
    fn new(path: String, mode: FileMode, now: Instant) -> Self {
        Self {
            path,
            mode,
            model_dirty: false,
            host: None,
            state: EntryState::Unattached,
            error_code: None,
            baseline: None,
            disk_sha: None,
            generation: 0,
            disk_generation: 0,
            size: 0,
            mtime_ns: 0,
            analyzed: None,
            pending: None,
            missing_polls: 0,
            next_poll: now,
            in_flight: None,
            detaches: 0,
            last_op: None,
        }
    }
    /// Adopt content that is now the file's visible baseline (a read or a committed save).
    fn adopt(&mut self, sha256: String, bom: bool, fingerprint: Fingerprint, interval: Duration) {
        self.generation += 1;
        self.disk_generation = self.generation;
        self.disk_sha = Some(sha256.clone());
        self.baseline = Some(Baseline { sha256, bom });
        self.size = fingerprint.size;
        self.mtime_ns = fingerprint.mtime_ns;
        self.analyzed = Some(Analyzed {
            fingerprint,
            state: EntryState::InSync,
            code: None,
        });
        self.state = EntryState::InSync;
        self.error_code = None;
        self.pending = None;
        self.missing_polls = 0;
        self.next_poll = Instant::now() + interval;
    }
}

struct State {
    epoch: Uuid,
    identity_namespace: Uuid,
    next_op: u64,
    entries: BTreeMap<u64, Entry>,
    retired: VecDeque<u64>,
    running_since: Option<Instant>,
}

enum LoadKind {
    Attach,
    Reload,
}
enum Outcome {
    Content(Box<FileContent>),
    Saved(SaveReceipt),
    Probe(Probe),
}
type Reply = Result<Outcome, FileFailure>;
enum Job {
    /// Wakes the worker so it notices shutdown without waiting for a full tick.
    Wake,
    Probe {
        operand: String,
        force: Option<FileMode>,
        reply: SyncSender<Reply>,
    },
    Load {
        surface: u64,
        op_id: u64,
        kind: LoadKind,
        host: Uuid,
        /// The entry's `detaches` when the job was admitted.
        detaches: u64,
        reply: SyncSender<Reply>,
    },
    Save {
        surface: u64,
        op_id: u64,
        host: Uuid,
        force: bool,
        body: Vec<u8>,
        reply: SyncSender<Reply>,
    },
}

struct Inner {
    config: FilesConfig,
    state: Mutex<State>,
    jobs: SyncSender<Job>,
    stop: AtomicBool,
    published: Mutex<Option<FilesSnapshot>>,
}
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panicked worker leaves plain bookkeeping behind; the dead worker makes every later
    // admission fail with `closed`, so continuing to read the state is safe and observable.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct FileService {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl FileService {
    pub fn start(config: FilesConfig) -> Result<Self, DesktopError> {
        let (jobs, receiver) = mpsc::sync_channel(QUEUE_BOUND);
        let inner = Arc::new(Inner {
            config,
            state: Mutex::new(State {
                epoch: Uuid::new_v4(),
                identity_namespace: Uuid::nil(),
                next_op: 1,
                entries: BTreeMap::new(),
                retired: VecDeque::new(),
                running_since: None,
            }),
            jobs,
            stop: AtomicBool::new(false),
            published: Mutex::new(None),
        });
        let worker_inner = Arc::clone(&inner);
        let worker = thread::Builder::new()
            .name("zmux-files".into())
            .spawn(move || worker_inner.run(&receiver))
            .map_err(|error| DesktopError::Failure(format!("start file worker: {error}")))?;
        Ok(Self {
            inner,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn config(&self) -> &FilesConfig {
        &self.inner.config
    }
    pub fn epoch(&self) -> Uuid {
        locked(&self.inner.state).epoch
    }

    /// Rotate the epoch: every existing owner becomes stale. Used by session restore.
    pub fn reset(&self) {
        let mut state = locked(&self.inner.state);
        state.epoch = Uuid::new_v4();
        state.entries.clear();
        state.retired.clear();
    }

    /// Mirror the model's file surfaces; retire owners of vanished surfaces. Cheap and idempotent.
    pub fn reconcile(&self, model: &Model) {
        let mut state = locked(&self.inner.state);
        state.identity_namespace = model.identity_namespace;
        let mut live = HashSet::new();
        let now = Instant::now();
        for surface in model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
        {
            let Some(file) = &surface.file else { continue };
            live.insert(surface.id.0);
            let entry = state
                .entries
                .entry(surface.id.0)
                .or_insert_with(|| Entry::new(file.path.clone(), file.mode, now));
            entry.model_dirty = file.dirty;
        }
        let gone: Vec<u64> = state
            .entries
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in gone {
            // Dropping the entry discards any in-flight result: the worker finds no entry.
            state.entries.remove(&id);
            state.retired.push_back(id);
            while state.retired.len() > RETIRED_OWNERS {
                state.retired.pop_front();
            }
        }
    }

    pub fn snapshot(&self, now: Instant) -> FilesSnapshot {
        let snapshot = self.build_snapshot(now);
        *locked(&self.inner.published) = Some(snapshot.clone());
        snapshot
    }
    fn build_snapshot(&self, now: Instant) -> FilesSnapshot {
        let state = locked(&self.inner.state);
        let stalled = state
            .running_since
            .map(|since| now.saturating_duration_since(since))
            .filter(|elapsed| *elapsed >= self.inner.config.stall_after);
        FilesSnapshot {
            epoch: state.epoch,
            worker: WorkerSnapshot {
                state: if stalled.is_some() { "stalled" } else { "ok" },
                stalled_ms: stalled.map_or(0, |elapsed| elapsed.as_secs() * 1000),
            },
            entries: state
                .entries
                .iter()
                .take(MAX_SNAPSHOT_ENTRIES)
                .map(|(id, entry)| EntrySnapshot {
                    surface_id: SurfaceId(*id),
                    state: if entry.host.is_some() {
                        entry.state
                    } else {
                        EntryState::Unattached
                    },
                    attached: entry.host.is_some(),
                    generation: entry.generation,
                    disk_generation: entry.disk_generation,
                    size: entry.size,
                    mtime_ns: entry.mtime_ns,
                    error_code: entry.error_code.map(str::to_owned),
                    last_op: entry.last_op.clone(),
                })
                .collect(),
        }
    }
    /// One executor tick: refresh the registry and report whether the published `files`
    /// section would differ. Never does I/O and never holds a lock across anything slow.
    pub fn poll(&self, model: &Model, now: Instant) -> bool {
        if let Some(hook) = &self.inner.config.executor_tick_hook {
            hook();
        }
        self.reconcile(model);
        let current = self.build_snapshot(now);
        locked(&self.inner.published).as_ref() != Some(&current)
    }

    fn admit_common(&self, state: &State) -> Result<(), FileFailure> {
        if self.inner.stop.load(Ordering::Acquire) {
            return Err(FileFailure::new(
                "closed",
                "the file service is shutting down",
            ));
        }
        if state
            .running_since
            .is_some_and(|since| since.elapsed() >= self.inner.config.stall_after)
        {
            return Err(FileFailure::new(
                "file_worker_unavailable",
                "the file worker is stalled on a slow volume; try again later",
            ));
        }
        Ok(())
    }
    fn owner_check(state: &State, owner: &FileOwner) -> Result<(), FileFailure> {
        if owner.epoch != state.epoch || owner.identity_namespace != state.identity_namespace {
            return Err(FileFailure::new(
                "stale_owner",
                "the document owner belongs to a previous session; reattach",
            ));
        }
        match state.entries.get(&owner.surface_id.0) {
            None if state.retired.contains(&owner.surface_id.0) => Err(FileFailure::new(
                "stale_owner",
                "the file tab was closed; this view is stale",
            )),
            None => Err(FileFailure::new("unknown_surface", "no such file surface")),
            Some(entry) if entry.host != Some(owner.host_id) => Err(FileFailure::new(
                "stale_owner",
                "this view is not the attached owner of the file tab",
            )),
            Some(_) => Ok(()),
        }
    }
    /// Validate an owner without side effects (bridge thread and executor both call this).
    pub fn check_owner(&self, owner: &FileOwner) -> Result<(), FileFailure> {
        Self::owner_check(&locked(&self.inner.state), owner)
    }

    /// Admit a state-changing operation on `surface`: marks it in flight and queues the job.
    /// On refusal everything is exactly as before.
    fn enqueue(
        &self,
        state: &mut State,
        surface: u64,
        kind: &'static str,
        build: impl FnOnce(u64) -> Job,
    ) -> Result<u64, FileFailure> {
        let op_id = state.next_op;
        let entry = state
            .entries
            .get_mut(&surface)
            .expect("caller validated the entry");
        if entry.in_flight.is_some() {
            return Err(FileFailure::new(
                "busy",
                "another file operation is still running for this tab",
            ));
        }
        let previous = entry.last_op.take();
        entry.in_flight = Some(op_id);
        entry.last_op = Some(OpReceipt {
            op_id,
            kind,
            state: OpState::Pending,
            code: None,
        });
        match self.inner.jobs.try_send(build(op_id)) {
            Ok(()) => {
                state.next_op += 1;
                Ok(op_id)
            }
            Err(error) => {
                let entry = state.entries.get_mut(&surface).expect("entry still exists");
                entry.in_flight = None;
                entry.last_op = previous;
                Err(match error {
                    TrySendError::Full(_) => FileFailure::new(
                        "busy",
                        "the file worker queue is full; nothing was started",
                    ),
                    TrySendError::Disconnected(_) => {
                        FileFailure::new("closed", "the file worker has stopped")
                    }
                })
            }
        }
    }
    fn wait(
        &self,
        receiver: &Receiver<Reply>,
        timeout: Duration,
        surface: u64,
        op_id: u64,
        is_save: bool,
    ) -> Result<Outcome, FileFailure> {
        match receiver.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => {
                if is_save {
                    let mut state = locked(&self.inner.state);
                    if let Some(receipt) = state
                        .entries
                        .get_mut(&surface)
                        .and_then(|e| e.last_op.as_mut())
                        .filter(|r| r.op_id == op_id && r.state == OpState::Pending)
                    {
                        receipt.state = OpState::OutcomeUnknown;
                        receipt.code = Some("timeout".into());
                    }
                }
                let mut failure = FileFailure::new(
                    "timeout",
                    if is_save {
                        "the save did not report back in time; its outcome is unknown. Check the snapshot and reload; it is never retried automatically"
                    } else {
                        "the file operation did not finish in time; nothing was delivered"
                    },
                )
                .with_op(op_id);
                failure.outcome_unknown = is_save;
                Err(failure)
            }
            Err(RecvTimeoutError::Disconnected) => {
                let mut failure =
                    FileFailure::new("closed", "the file worker stopped before replying")
                        .with_op(op_id);
                failure.outcome_unknown = is_save;
                Err(failure)
            }
        }
    }

    pub fn attach(
        &self,
        epoch: Uuid,
        surface_id: SurfaceId,
        host: Uuid,
    ) -> Result<FileContent, FileFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let op_id = {
            let mut state = locked(&self.inner.state);
            self.admit_common(&state)?;
            if epoch != state.epoch {
                return Err(FileFailure::new(
                    "stale_owner",
                    "the epoch belongs to a previous session; read the snapshot again",
                ));
            }
            match state.entries.get(&surface_id.0) {
                None if state.retired.contains(&surface_id.0) => {
                    return Err(FileFailure::new("stale_owner", "the file tab was closed"));
                }
                None => return Err(FileFailure::new("unknown_surface", "no such file surface")),
                Some(entry) if entry.host.is_some_and(|bound| bound != host) => {
                    return Err(FileFailure::new(
                        "host_conflict",
                        "another view is already attached to this file tab",
                    ));
                }
                // Attaching again as the bound host re-reads and re-baselines exactly like a
                // reload, so it obeys the same dirty rule: unsaved text is only dropped by an
                // explicit `reload` with `discard`.
                Some(entry) if entry.host == Some(host) && entry.model_dirty => {
                    return Err(FileFailure::new(
                        "dirty",
                        "the tab has unsaved changes; reload with discard to drop them",
                    ));
                }
                Some(_) => {}
            }
            let detaches = state.entries[&surface_id.0].detaches;
            self.enqueue(&mut state, surface_id.0, "attach", |op_id| Job::Load {
                surface: surface_id.0,
                op_id,
                kind: LoadKind::Attach,
                host,
                detaches,
                reply: sender,
            })?
        };
        self.finish_content(&receiver, surface_id.0, op_id)
    }
    pub fn reload(&self, owner: &FileOwner, discard: bool) -> Result<FileContent, FileFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let op_id = {
            let mut state = locked(&self.inner.state);
            self.admit_common(&state)?;
            Self::owner_check(&state, owner)?;
            if state.entries[&owner.surface_id.0].model_dirty && !discard {
                return Err(FileFailure::new(
                    "dirty",
                    "the tab has unsaved changes; reload with discard to drop them",
                ));
            }
            let detaches = state.entries[&owner.surface_id.0].detaches;
            self.enqueue(&mut state, owner.surface_id.0, "reload", |op_id| {
                Job::Load {
                    surface: owner.surface_id.0,
                    op_id,
                    kind: LoadKind::Reload,
                    host: owner.host_id,
                    detaches,
                    reply: sender,
                }
            })?
        };
        self.finish_content(&receiver, owner.surface_id.0, op_id)
    }
    fn finish_content(
        &self,
        receiver: &Receiver<Reply>,
        surface: u64,
        op_id: u64,
    ) -> Result<FileContent, FileFailure> {
        match self.wait(
            receiver,
            self.inner.config.read_timeout,
            surface,
            op_id,
            false,
        )? {
            Outcome::Content(content) => Ok(*content),
            _ => Err(FileFailure::new("internal", "unexpected worker reply").with_op(op_id)),
        }
    }
    pub fn save(
        &self,
        owner: &FileOwner,
        base_generation: u64,
        force: bool,
        body: Vec<u8>,
    ) -> Result<SaveReceipt, FileFailure> {
        if body.len() > MAX_TEXT_BYTES {
            return Err(FileFailure::new(
                "too_large",
                format!("save body is larger than the {MAX_TEXT_BYTES} byte limit"),
            ));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let op_id = {
            let mut state = locked(&self.inner.state);
            self.admit_common(&state)?;
            Self::owner_check(&state, owner)?;
            let entry = &state.entries[&owner.surface_id.0];
            if !entry.mode.is_editable() {
                return Err(FileFailure::new(
                    "invalid_request",
                    "images and PDFs are read-only previews",
                ));
            }
            let Some(baseline) = entry.baseline.as_ref() else {
                return Err(FileFailure::new(
                    "stale_owner",
                    "the tab has no loaded content; reattach first",
                ));
            };
            if file_io::saved_size(body.len(), baseline.bom) > MAX_TEXT_BYTES {
                return Err(FileFailure::new(
                    "too_large",
                    format!(
                        "the saved file, including its byte order mark, would exceed the {MAX_TEXT_BYTES} byte limit"
                    ),
                ));
            }
            if entry.generation != base_generation {
                return Err(FileFailure::new(
                    "stale_view",
                    "the view is behind a reload it did not take; nothing was written",
                ));
            }
            self.enqueue(&mut state, owner.surface_id.0, "save", |op_id| Job::Save {
                surface: owner.surface_id.0,
                op_id,
                host: owner.host_id,
                force,
                body,
                reply: sender,
            })?
        };
        match self.wait(
            &receiver,
            self.inner.config.save_timeout,
            owner.surface_id.0,
            op_id,
            true,
        )? {
            Outcome::Saved(receipt) => Ok(receipt),
            _ => Err(FileFailure::new("internal", "unexpected worker reply").with_op(op_id)),
        }
    }
    /// Release the binding. The unsaved text, if any, is owned by the view that is going away.
    pub fn detach(&self, owner: &FileOwner) -> Result<(), FileFailure> {
        let mut state = locked(&self.inner.state);
        Self::owner_check(&state, owner)?;
        let entry = state
            .entries
            .get_mut(&owner.surface_id.0)
            .expect("owner check found the entry");
        entry.host = None;
        entry.detaches += 1;
        entry.baseline = None;
        entry.disk_sha = None;
        entry.state = EntryState::Unattached;
        entry.error_code = None;
        entry.analyzed = None;
        entry.pending = None;
        entry.missing_polls = 0;
        Ok(())
    }

    /// Probe one operand on the worker, waiting no longer than `deadline`.
    pub fn probe(
        &self,
        operand: &str,
        force: Option<FileMode>,
        deadline: Instant,
    ) -> Result<Probe, FileFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let state = locked(&self.inner.state);
            self.admit_common(&state)?;
            self.inner
                .jobs
                .try_send(Job::Probe {
                    operand: operand.to_owned(),
                    force,
                    reply: sender,
                })
                .map_err(|error| match error {
                    TrySendError::Full(_) => FileFailure::new(
                        "busy",
                        "the file worker queue is full; nothing was started",
                    ),
                    TrySendError::Disconnected(_) => {
                        FileFailure::new("closed", "the file worker has stopped")
                    }
                })?;
        }
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(Outcome::Probe(probe))) => Ok(probe),
            Ok(Ok(_)) => Err(FileFailure::new("internal", "unexpected worker reply")),
            Ok(Err(failure)) => Err(failure),
            Err(RecvTimeoutError::Timeout) => Err(FileFailure::new(
                "probe_timeout",
                "the file could not be examined before the deadline; nothing was opened",
            )),
            Err(RecvTimeoutError::Disconnected) => {
                Err(FileFailure::new("closed", "the file worker stopped"))
            }
        }
    }

    pub fn request_shutdown(&self) {
        self.inner.stop.store(true, Ordering::Release);
        // A full queue means the worker is busy and will see the flag before its next job.
        let _ = self.inner.jobs.try_send(Job::Wake);
    }
    pub fn is_finished(&self) -> bool {
        locked(&self.worker)
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
    }
    /// Worker threads still owned (0 or 1), for the shutdown report.
    pub fn pending_count(&self) -> usize {
        usize::from(!self.is_finished())
    }
    /// Join the worker if it has stopped. `Ok(false)` means blocked I/O may still be running.
    pub fn poll_shutdown(&self) -> Result<bool, String> {
        let mut worker = locked(&self.worker);
        match worker.take() {
            None => Ok(true),
            Some(handle) if handle.is_finished() => handle
                .join()
                .map(|()| true)
                .map_err(|_| "file worker panicked".to_string()),
            Some(handle) => {
                *worker = Some(handle);
                Ok(false)
            }
        }
    }
    pub fn shutdown(&self, deadline: Instant) -> Result<(), String> {
        self.request_shutdown();
        loop {
            if self.poll_shutdown()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("file worker shutdown timed out; IO may still be running".into());
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for FileService {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        let _ = self.inner.jobs.try_send(Job::Wake);
        // Never join a possibly blocked storage call in Drop.
    }
}

impl Inner {
    fn run(&self, jobs: &Receiver<Job>) {
        while !self.stop.load(Ordering::Acquire) {
            match jobs.recv_timeout(self.config.poll_tick) {
                Ok(job) => {
                    self.execute(job);
                    // Jobs always run before polls.
                    while !self.stop.load(Ordering::Acquire)
                        && let Ok(job) = jobs.try_recv()
                    {
                        self.execute(job);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !self.stop.load(Ordering::Acquire) {
                self.poll_round();
            }
        }
        // Queued jobs are dropped: their callers see `closed`. A save already past its
        // rename point completed inside `execute`.
        while jobs.try_recv().is_ok() {}
    }
    fn set_running(&self, running: bool) {
        locked(&self.state).running_since = running.then(Instant::now);
    }
    fn execute(&self, job: Job) {
        match job {
            Job::Wake => {}
            Job::Probe {
                operand,
                force,
                reply,
            } => {
                self.set_running(true);
                let result = file_io::probe(self.config.backend.as_ref(), &operand, force);
                self.set_running(false);
                // The caller may have timed out and gone; a probe has no state to record.
                let _ = reply.send(
                    result
                        .map(Outcome::Probe)
                        .map_err(|error| FileFailure::from(&error)),
                );
            }
            Job::Load {
                surface,
                op_id,
                kind,
                host,
                detaches,
                reply,
            } => self.load(surface, op_id, &kind, host, detaches, &reply),
            Job::Save {
                surface,
                op_id,
                host,
                force,
                body,
                reply,
            } => self.save(surface, op_id, host, force, &body, &reply),
        }
    }
    /// The caller may have timed out; the outcome is already recorded in the entry's
    /// `last_op`, so a failed send loses nothing.
    fn answer(reply: &SyncSender<Reply>, result: Reply) {
        let _ = reply.send(result);
    }

    fn load(
        &self,
        surface: u64,
        op_id: u64,
        kind: &LoadKind,
        host: Uuid,
        detaches: u64,
        reply: &SyncSender<Reply>,
    ) {
        let target = {
            let state = locked(&self.state);
            state
                .entries
                .get(&surface)
                .filter(|entry| entry.in_flight == Some(op_id))
                .map(|entry| (entry.path.clone(), entry.mode))
        };
        let Some((path, mode)) = target else {
            Self::answer(
                reply,
                Err(FileFailure::new("stale_owner", "the file tab was closed").with_op(op_id)),
            );
            return;
        };
        self.set_running(true);
        let result = file_io::load(self.config.backend.as_ref(), Path::new(&path), mode);
        self.set_running(false);
        let mut state = locked(&self.state);
        let (epoch, namespace) = (state.epoch, state.identity_namespace);
        let Some(entry) = state
            .entries
            .get_mut(&surface)
            .filter(|entry| entry.in_flight == Some(op_id))
        else {
            Self::answer(
                reply,
                Err(FileFailure::new(
                    "stale_owner",
                    "the file tab was closed while the operation ran; its result was discarded",
                )
                .with_op(op_id)),
            );
            return;
        };
        entry.in_flight = None;
        let kind_name = match kind {
            LoadKind::Attach => "attach",
            LoadKind::Reload => "reload",
        };
        if entry.detaches != detaches {
            // The view released the tab while this ran: adopting the result would re-bind a
            // host that no longer exists and block every later attach.
            entry.last_op = Some(OpReceipt {
                op_id,
                kind: kind_name,
                state: OpState::Failed,
                code: Some("stale_owner".into()),
            });
            Self::answer(
                reply,
                Err(FileFailure::new(
                    "stale_owner",
                    "the view detached while the operation ran; its result was discarded",
                )
                .with_op(op_id)),
            );
            return;
        }
        match result {
            Ok(loaded) => {
                entry.host = Some(host);
                entry.adopt(
                    loaded.sha256.clone(),
                    loaded.bom,
                    loaded.meta.fingerprint,
                    self.config.poll_interval,
                );
                entry.last_op = Some(OpReceipt {
                    op_id,
                    kind: kind_name,
                    state: OpState::Completed,
                    code: None,
                });
                let content = FileContent {
                    op_id,
                    owner: FileOwner {
                        epoch,
                        identity_namespace: namespace,
                        surface_id: SurfaceId(surface),
                        host_id: host,
                    },
                    mode: entry.mode,
                    size: loaded.meta.fingerprint.size,
                    generation: entry.generation,
                    sha256: loaded.sha256.clone(),
                    mtime_ns: loaded.meta.fingerprint.mtime_ns,
                    writable: loaded.writable,
                    bom: loaded.bom,
                    body: loaded.body().to_vec(),
                };
                Self::answer(reply, Ok(Outcome::Content(Box::new(content))));
            }
            Err(error) => {
                entry.last_op = Some(OpReceipt {
                    op_id,
                    kind: kind_name,
                    state: OpState::Failed,
                    code: Some(error.code.into()),
                });
                Self::answer(reply, Err(FileFailure::from(&error).with_op(op_id)));
            }
        }
    }

    fn save(
        &self,
        surface: u64,
        op_id: u64,
        host: Uuid,
        force: bool,
        body: &[u8],
        reply: &SyncSender<Reply>,
    ) {
        let target = {
            let state = locked(&self.state);
            state
                .entries
                .get(&surface)
                .filter(|entry| entry.in_flight == Some(op_id) && entry.host == Some(host))
                .and_then(|entry| {
                    entry
                        .baseline
                        .as_ref()
                        .map(|baseline| (entry.path.clone(), baseline.sha256.clone(), baseline.bom))
                })
        };
        let Some((path, expected, bom)) = target else {
            // The tab may still exist (its view detached while the save was queued): release
            // the one-op-in-flight marker and record the refusal, or the tab stays busy forever.
            if let Some(entry) = locked(&self.state)
                .entries
                .get_mut(&surface)
                .filter(|entry| entry.in_flight == Some(op_id))
            {
                entry.in_flight = None;
                entry.last_op = Some(OpReceipt {
                    op_id,
                    kind: "save",
                    state: OpState::Failed,
                    code: Some("stale_owner".into()),
                });
            }
            Self::answer(
                reply,
                Err(
                    FileFailure::new("stale_owner", "the file tab was closed or replaced")
                        .with_op(op_id),
                ),
            );
            return;
        };
        let epoch = locked(&self.state).epoch;
        // Last chance to undo: consulted by the save after its temporary file is durable.
        let fence = || {
            let state = locked(&self.state);
            state.epoch == epoch
                && state
                    .entries
                    .get(&surface)
                    .is_some_and(|e| e.in_flight == Some(op_id) && e.host == Some(host))
        };
        self.set_running(true);
        let result = file_io::save(
            self.config.backend.as_ref(),
            Path::new(&path),
            body,
            bom,
            &expected,
            force,
            &fence,
        );
        self.set_running(false);
        let mut state = locked(&self.state);
        let entry = state
            .entries
            .get_mut(&surface)
            .filter(|entry| entry.in_flight == Some(op_id));
        match result {
            Ok(success) => {
                let receipt = SaveReceipt {
                    op_id,
                    generation: 0,
                    sha256: success.sha256.clone(),
                    size: success.meta.fingerprint.size,
                    mtime_ns: success.meta.fingerprint.mtime_ns,
                };
                let Some(entry) = entry else {
                    // The save is on disk but its view is gone: report the truth.
                    Self::answer(reply, Ok(Outcome::Saved(receipt)));
                    return;
                };
                entry.in_flight = None;
                if entry.host == Some(host) {
                    entry.adopt(
                        success.sha256,
                        bom,
                        success.meta.fingerprint,
                        self.config.poll_interval,
                    );
                }
                entry.last_op = Some(OpReceipt {
                    op_id,
                    kind: "save",
                    state: OpState::Completed,
                    code: None,
                });
                let generation = entry.generation;
                Self::answer(
                    reply,
                    Ok(Outcome::Saved(SaveReceipt {
                        generation,
                        ..receipt
                    })),
                );
            }
            Err(failure) => {
                let mut reported = FileFailure::from(&failure.error).with_op(op_id);
                reported.outcome_unknown = failure.outcome_unknown;
                reported.conflict = failure.conflict.map(|c| Box::new(ConflictInfo::from(c)));
                if let Some(entry) = entry {
                    entry.in_flight = None;
                    if let Some(committed) = failure.committed
                        && entry.host == Some(host)
                    {
                        // The new content is visible even though the save reports an error.
                        entry.adopt(
                            committed.sha256,
                            bom,
                            committed.meta.fingerprint,
                            self.config.poll_interval,
                        );
                    }
                    entry.last_op = Some(OpReceipt {
                        op_id,
                        kind: "save",
                        state: if failure.outcome_unknown {
                            OpState::OutcomeUnknown
                        } else {
                            OpState::Failed
                        },
                        code: Some(failure.error.code.into()),
                    });
                }
                Self::answer(reply, Err(reported));
            }
        }
    }

    fn poll_round(&self) {
        let now = Instant::now();
        let due: Vec<PollInput> = {
            let mut state = locked(&self.state);
            let mut picked = Vec::new();
            for (id, entry) in &mut state.entries {
                if picked.len() >= POLL_BATCH {
                    break;
                }
                let (Some(host), Some(baseline)) = (entry.host, entry.baseline.as_ref()) else {
                    continue;
                };
                if entry.in_flight.is_some() || now < entry.next_poll {
                    continue;
                }
                entry.next_poll = now + self.config.poll_interval;
                picked.push(PollInput {
                    surface: *id,
                    path: entry.path.clone(),
                    mode: entry.mode,
                    host,
                    generation: entry.generation,
                    baseline_sha: baseline.sha256.clone(),
                    disk_sha: entry.disk_sha.clone(),
                    state: entry.state,
                    code: entry.error_code,
                    analyzed: entry.analyzed.clone(),
                    pending: entry.pending,
                    missing: entry.missing_polls,
                });
            }
            picked
        };
        for input in due {
            if self.stop.load(Ordering::Acquire) {
                return;
            }
            self.set_running(true);
            let backend = self.config.backend.as_ref();
            let path = Path::new(&input.path);
            let update = decide(&input, file_io::observe(backend, path), || {
                file_io::load(backend, path, input.mode)
            });
            self.set_running(false);
            let mut state = locked(&self.state);
            let Some(entry) = state.entries.get_mut(&input.surface).filter(|entry| {
                entry.in_flight.is_none()
                    && entry.host == Some(input.host)
                    && entry.generation == input.generation
            }) else {
                continue;
            };
            entry.state = update.state;
            entry.error_code = update.code;
            entry.pending = update.pending;
            entry.missing_polls = update.missing;
            if let Some(analyzed) = update.analyzed {
                entry.size = analyzed.fingerprint.size;
                entry.mtime_ns = analyzed.fingerprint.mtime_ns;
                entry.analyzed = Some(analyzed);
            }
            if update.bump_disk_generation {
                entry.disk_generation += 1;
            }
            if let Some(disk_sha) = update.disk_sha {
                entry.disk_sha = disk_sha;
            }
        }
    }
}

struct PollInput {
    surface: u64,
    path: String,
    mode: FileMode,
    host: Uuid,
    generation: u64,
    baseline_sha: String,
    disk_sha: Option<String>,
    state: EntryState,
    code: Option<&'static str>,
    analyzed: Option<Analyzed>,
    pending: Option<Fingerprint>,
    missing: u8,
}
struct PollUpdate {
    state: EntryState,
    code: Option<&'static str>,
    pending: Option<Fingerprint>,
    missing: u8,
    analyzed: Option<Analyzed>,
    bump_disk_generation: bool,
    /// `Some` replaces the entry's remembered disk hash.
    disk_sha: Option<Option<String>>,
}
/// Pure external-change decision (design D14). `load` runs only once metadata has been
/// stable across two polls, which coalesces a writer that is still mid-write.
fn decide(
    input: &PollInput,
    observation: Observation,
    load: impl FnOnce() -> Result<file_io::LoadedFile, FileError>,
) -> PollUpdate {
    let keep = |pending, missing| PollUpdate {
        state: input.state,
        code: input.code,
        pending,
        missing,
        analyzed: None,
        bump_disk_generation: false,
        disk_sha: None,
    };
    match observation {
        Observation::Missing => {
            // One miss may be an editor mid atomic-replace; two in a row is a delete.
            let missing = input.missing.saturating_add(1);
            if missing >= 2 {
                PollUpdate {
                    state: EntryState::Deleted,
                    code: None,
                    // A file that comes back is a new disk generation even if its bytes match.
                    disk_sha: Some(None),
                    ..keep(None, missing)
                }
            } else {
                keep(None, missing)
            }
        }
        Observation::Unreadable(error) => PollUpdate {
            state: EntryState::Unreadable,
            code: Some(error.code),
            ..keep(None, 0)
        },
        Observation::Present(meta) => {
            let fingerprint = meta.fingerprint;
            if let Some(analyzed) = input
                .analyzed
                .as_ref()
                .filter(|a| a.fingerprint == fingerprint)
            {
                return PollUpdate {
                    state: analyzed.state,
                    code: analyzed.code,
                    ..keep(None, 0)
                };
            }
            if input.pending != Some(fingerprint) {
                return keep(Some(fingerprint), 0);
            }
            match load() {
                Ok(loaded) => {
                    let changed = loaded.sha256 != input.baseline_sha;
                    // The fingerprint was taken before the bytes were read, so a write that
                    // lands in between is examined twice; only different bytes are a new edit.
                    let new_content = changed && input.disk_sha.as_deref() != Some(&loaded.sha256);
                    let state = if changed {
                        EntryState::Changed
                    } else {
                        EntryState::InSync
                    };
                    PollUpdate {
                        state,
                        code: None,
                        pending: None,
                        missing: 0,
                        analyzed: Some(Analyzed {
                            fingerprint: loaded.meta.fingerprint,
                            state,
                            code: None,
                        }),
                        bump_disk_generation: new_content,
                        disk_sha: Some(Some(loaded.sha256.clone())),
                    }
                }
                Err(error) => PollUpdate {
                    state: EntryState::Unreadable,
                    code: Some(error.code),
                    pending: None,
                    missing: 0,
                    analyzed: Some(Analyzed {
                        fingerprint,
                        state: EntryState::Unreadable,
                        code: Some(error.code),
                    }),
                    bump_disk_generation: false,
                    disk_sha: None,
                },
            }
        }
    }
}

/// Executor side: applies probed operands and trusted view events to the model.
impl Worker {
    pub(crate) fn file_host_event(&mut self, event: FileHostEvent) -> Result<(), DesktopError> {
        let files = Arc::clone(&self.shared.files);
        match event {
            FileHostEvent::Observed { owner, dirty } => {
                files.check_owner(&owner)?;
                self.action(Action::ObserveFile {
                    surface_id: owner.surface_id,
                    dirty,
                })
            }
            FileHostEvent::Detach { owner } => {
                files.detach(&owner)?;
                let dirty = self
                    .model
                    .snapshot()
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .find(|s| s.id == owner.surface_id)
                    .and_then(|s| s.file.as_ref())
                    .is_some_and(|f| f.dirty);
                if dirty {
                    // The view that held the unsaved text is gone; the bit would stick.
                    self.action(Action::ObserveFile {
                        surface_id: owner.surface_id,
                        dirty: false,
                    })?;
                }
                Ok(())
            }
        }
    }

    /// Apply a probed `open`. Whole-request failures (stale revision, unresolved target) are
    /// returned before any item runs; everything else is a per-item result.
    pub(crate) fn open_files(&mut self, batch: OpenBatch) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        let mut request = TargetRequest {
            scope: batch.scope,
            caller: batch.caller,
            expected_model_revision: batch.expected_model_revision,
            operation: Default::default(),
        };
        crate::prepare_target(&model, &mut request)?;
        let pane = request.scope.resolve(&model, &request.caller)?.pane;
        let mut focus = batch.focus;
        let mut ok = true;
        let mut items = Vec::new();
        for (index, plan) in batch.items.into_iter().enumerate() {
            let item = match plan {
                OpenPlan::File {
                    probe: Ok(probe), ..
                } => self.open_file(index, &probe, pane, &mut focus),
                OpenPlan::File {
                    operand,
                    probe: Err(failure),
                } => Err(ItemError::new(
                    "file",
                    operand,
                    failure.code,
                    failure.message,
                )),
                OpenPlan::Url { url } => self.open_url(index, &url, pane, &mut focus),
                OpenPlan::Invalid {
                    kind,
                    operand,
                    failure,
                } => Err(ItemError::new(kind, operand, failure.code, failure.message)),
            };
            items.push(match item {
                Ok(value) => value,
                Err(error) => {
                    ok = false;
                    error.into_json(index)
                }
            });
        }
        self.open_result = Some(serde_json::json!({"ok": ok, "items": items}));
        Ok(())
    }

    fn open_file(
        &mut self,
        index: usize,
        probe: &Probe,
        pane: PaneId,
        focus: &mut bool,
    ) -> Result<serde_json::Value, ItemError> {
        let fail =
            |code: &str, message: String| ItemError::new("file", probe.path.clone(), code, message);
        let model = self.model.snapshot();
        let locate = |model: &Model| {
            model.workspaces.iter().find_map(|workspace| {
                workspace.root.panes().into_iter().find_map(|pane| {
                    pane.surfaces
                        .iter()
                        .find(|s| s.file.as_ref().is_some_and(|f| f.path == probe.path))
                        .map(|s| (s.id, s.file.as_ref().map(|f| f.mode)))
                })
            })
        };
        let deduplicated = locate(&model);
        if let Some((surface_id, _)) = deduplicated {
            if *focus {
                self.action(Action::SelectSurface { surface_id })
                    .map_err(|error| fail("rejected", error.to_string()))?;
            }
        } else {
            let files = surfaces(&model)
                .filter(|(_, _, s)| s.file.is_some())
                .count();
            if files >= crate::MAX_FILE_SURFACES {
                return Err(fail(
                    "file_capacity",
                    format!(
                        "at most {} file tabs can be open; close one first",
                        crate::MAX_FILE_SURFACES
                    ),
                ));
            }
            check_pane_room(&model, pane).map_err(|(code, message)| fail(code, message))?;
            self.action(Action::CreateFile {
                pane_id: pane,
                path: probe.path.clone(),
                mode: probe.mode,
                focus: *focus,
            })
            .map_err(|error| fail("rejected", error.to_string()))?;
        }
        *focus = false;
        let model = self.model.snapshot();
        let (surface_id, mode) = locate(&model)
            .ok_or_else(|| fail("rejected", "the file tab disappeared while opening".into()))?;
        Ok(serde_json::json!({
            "index": index, "kind": "file", "ok": true, "path": probe.path,
            "mode": mode.unwrap_or(probe.mode),
            "deduplicated": deduplicated.is_some(),
            "surface": surface_receipt(&model, surface_id),
        }))
    }

    fn open_url(
        &mut self,
        index: usize,
        url: &str,
        pane: PaneId,
        focus: &mut bool,
    ) -> Result<serde_json::Value, ItemError> {
        let fail =
            |code: &str, message: String| ItemError::new("url", url.to_owned(), code, message);
        let model = self.model.snapshot();
        let browsers = surfaces(&model)
            .filter(|(_, _, s)| s.kind == SurfaceKind::Browser)
            .count();
        if browsers >= crate::MAX_BROWSERS {
            return Err(fail(
                "browser_capacity",
                format!(
                    "at most {} browser tabs can be open; close one first",
                    crate::MAX_BROWSERS
                ),
            ));
        }
        check_pane_room(&model, pane).map_err(|(code, message)| fail(code, message))?;
        let before: HashSet<_> = surfaces(&model).map(|(_, _, s)| s.id).collect();
        self.action(Action::CreateBrowser {
            profile_id: None,
            pane_id: pane,
            url: url.to_owned(),
            focus: *focus,
        })
        .map_err(|error| fail("rejected", error.to_string()))?;
        *focus = false;
        let model = self.model.snapshot();
        let created = surfaces(&model)
            .map(|(_, _, s)| s.id)
            .find(|id| !before.contains(id))
            .ok_or_else(|| {
                fail(
                    "rejected",
                    "the browser tab disappeared while opening".into(),
                )
            })?;
        Ok(serde_json::json!({
            "index": index, "kind": "url", "ok": true, "url": url,
            "surface": surface_receipt(&model, created),
        }))
    }
}

/// Lexical classification of a URL operand (design D3). Only `http` and `https` open as browser
/// tabs; every other `scheme://` form is refused, and `file://` is not a way to name a file.
pub fn classify_url(value: &str, force: Option<FileMode>) -> Result<(), FileFailure> {
    if force.is_some() {
        return Err(FileFailure::new(
            "invalid_operand",
            "this command opens paths only; use `zmux open` for web addresses",
        ));
    }
    let lower = value.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(if value.contains("://") {
            FileFailure::new(
                "unsupported_scheme",
                "only http:// and https:// addresses open as tabs; name a file by its path",
            )
        } else {
            FileFailure::new(
                "invalid_url",
                "a web address must start with http:// or https://",
            )
        });
    }
    zmux_workspace_core::validate_browser_url(value)
        .map_err(|error| FileFailure::new("invalid_url", error.to_string()))
}

/// A per-item failure: data in the result, never a whole-request error.
struct ItemError {
    kind: &'static str,
    operand: String,
    code: String,
    message: String,
}
impl ItemError {
    fn new(
        kind: &'static str,
        operand: String,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operand,
            code: code.into(),
            message: message.into(),
        }
    }
    fn into_json(self, index: usize) -> serde_json::Value {
        let key = if self.kind == "url" { "url" } else { "path" };
        serde_json::json!({
            "index": index, "kind": self.kind, "ok": false, key: self.operand,
            "error": {"code": self.code, "message": self.message},
        })
    }
}
fn surfaces(
    model: &Model,
) -> impl Iterator<
    Item = (
        zmux_workspace_core::WorkspaceId,
        PaneId,
        &zmux_workspace_core::Surface,
    ),
> {
    model.workspaces.iter().flat_map(|workspace| {
        workspace.root.panes().into_iter().flat_map(move |pane| {
            pane.surfaces
                .iter()
                .map(move |s| (workspace.id, pane.id, s))
        })
    })
}
fn check_pane_room(model: &Model, pane: PaneId) -> Result<(), (&'static str, String)> {
    let tabs = model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .find(|p| p.id == pane)
        .map(|p| p.surfaces.len())
        .ok_or(("rejected", "the target pane no longer exists".to_owned()))?;
    if tabs >= MAX_SURFACES_PER_PANE {
        return Err((
            "tab_capacity",
            format!("the pane already has {MAX_SURFACES_PER_PANE} tabs"),
        ));
    }
    Ok(())
}
fn surface_receipt(model: &Model, surface: SurfaceId) -> serde_json::Value {
    let (workspace, pane, _) = surfaces(model)
        .find(|(_, _, s)| s.id == surface)
        .expect("a receipt is built for a live surface");
    let window = model
        .windows
        .iter()
        .find(|window| window.workspace_ids.contains(&workspace))
        .expect("canonical workspace owner")
        .id;
    serde_json::json!({
        "window_id": window,
        "window_uuid": model.object_uuid(TargetKind::Window, window.0),
        "workspace_id": workspace,
        "workspace_uuid": model.object_uuid(TargetKind::Workspace, workspace.0),
        "pane_id": pane,
        "pane_uuid": model.object_uuid(TargetKind::Pane, pane.0),
        "surface_id": surface,
        "surface_uuid": model.object_uuid(TargetKind::Surface, surface.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(analyzed: Option<Fingerprint>, pending: Option<Fingerprint>) -> PollInput {
        PollInput {
            surface: 1,
            path: "/x".into(),
            mode: FileMode::Text,
            host: Uuid::new_v4(),
            generation: 1,
            baseline_sha: "base".into(),
            disk_sha: Some("base".into()),
            state: EntryState::InSync,
            code: None,
            analyzed: analyzed.map(|fingerprint| Analyzed {
                fingerprint,
                state: EntryState::InSync,
                code: None,
            }),
            pending,
            missing: 0,
        }
    }
    fn fingerprint(size: u64) -> Fingerprint {
        Fingerprint {
            dev: 1,
            ino: 1,
            size,
            mtime_ns: size as i64,
            ctime_ns: 0,
        }
    }
    fn meta(size: u64) -> file_io::Meta {
        file_io::Meta {
            fingerprint: fingerprint(size),
            mode: 0o644,
            nlink: 1,
            regular: true,
            directory: false,
        }
    }
    fn never() -> Result<file_io::LoadedFile, FileError> {
        panic!("content must not be read");
    }

    #[test]
    fn unchanged_metadata_never_reads_content() {
        let update = decide(
            &input(Some(fingerprint(5)), None),
            Observation::Present(meta(5)),
            never,
        );
        assert_eq!(update.state, EntryState::InSync);
        assert!(update.pending.is_none() && !update.bump_disk_generation);
    }
    #[test]
    fn a_new_fingerprint_must_be_stable_for_one_more_poll_before_content_is_read() {
        let first = decide(
            &input(Some(fingerprint(5)), None),
            Observation::Present(meta(9)),
            never,
        );
        assert_eq!(first.pending, Some(fingerprint(9)));
        // A writer still appending changes the fingerprint again: the wait restarts.
        let still_writing = decide(
            &input(Some(fingerprint(5)), first.pending),
            Observation::Present(meta(12)),
            never,
        );
        assert_eq!(still_writing.pending, Some(fingerprint(12)));
    }
    fn loaded(sha256: &str, size: u64) -> Result<file_io::LoadedFile, FileError> {
        Ok(file_io::LoadedFile {
            bytes: Vec::new(),
            meta: meta(size),
            sha256: sha256.into(),
            bom: false,
            writable: true,
        })
    }
    #[test]
    fn only_new_disk_content_is_a_new_disk_generation() {
        let mut state = input(Some(fingerprint(5)), Some(fingerprint(9)));
        // The first examination of an external edit counts.
        let first = decide(&state, Observation::Present(meta(9)), || loaded("edit", 9));
        assert_eq!(first.state, EntryState::Changed);
        assert!(first.bump_disk_generation);
        assert_eq!(first.disk_sha, Some(Some("edit".into())));
        // The same bytes examined again because the fingerprint moved (a write landed between
        // the fingerprint and the read) are the same edit.
        state.disk_sha = Some("edit".into());
        state.analyzed = first.analyzed;
        state.pending = Some(fingerprint(11));
        let again = decide(&state, Observation::Present(meta(11)), || {
            loaded("edit", 11)
        });
        assert_eq!(again.state, EntryState::Changed);
        assert!(!again.bump_disk_generation, "same bytes, same generation");
        // Different bytes are the next generation.
        state.pending = Some(fingerprint(13));
        let next = decide(&state, Observation::Present(meta(13)), || {
            loaded("edit two", 13)
        });
        assert!(next.bump_disk_generation);
        // Healing back to the baseline is not a new generation, and a file that disappeared and
        // came back is, even with the bytes it had before.
        let healed = decide(&state, Observation::Present(meta(13)), || {
            loaded("base", 13)
        });
        assert_eq!(healed.state, EntryState::InSync);
        assert!(!healed.bump_disk_generation);
        state.missing = 1;
        let gone = decide(&state, Observation::Missing, never);
        assert_eq!(gone.disk_sha, Some(None));
        state.disk_sha = None;
        state.pending = Some(fingerprint(11));
        let back = decide(&state, Observation::Present(meta(11)), || {
            loaded("edit", 11)
        });
        assert!(back.bump_disk_generation);
    }
    #[test]
    fn one_miss_is_not_a_delete_but_two_are() {
        let mut state = input(Some(fingerprint(5)), None);
        let first = decide(&state, Observation::Missing, never);
        assert_eq!(first.state, EntryState::InSync);
        assert_eq!(first.missing, 1);
        state.missing = first.missing;
        let second = decide(&state, Observation::Missing, never);
        assert_eq!(second.state, EntryState::Deleted);
    }
    #[test]
    fn registry_reset_rotates_the_epoch_and_fences_every_owner() {
        let service = FileService::start(FilesConfig::default()).unwrap();
        let before = service.epoch();
        let owner = FileOwner {
            epoch: before,
            identity_namespace: Uuid::nil(),
            surface_id: SurfaceId(1),
            host_id: Uuid::new_v4(),
        };
        service.reset();
        assert_ne!(service.epoch(), before);
        assert_eq!(service.check_owner(&owner).unwrap_err().code, "stale_owner");
        service
            .shutdown(Instant::now() + Duration::from_secs(30))
            .unwrap();
        assert!(service.is_finished());
    }
    #[test]
    fn shutdown_joins_an_idle_worker_and_refuses_new_work() {
        // A tick far longer than the shutdown budget: only the wake-up can end the worker in
        // time, so a missing wake-up fails the shutdown itself (no stopwatch involved).
        let service = FileService::start(FilesConfig {
            poll_tick: Duration::from_secs(600),
            ..FilesConfig::default()
        })
        .unwrap();
        service
            .shutdown(Instant::now() + Duration::from_secs(30))
            .unwrap();
        assert_eq!(
            service
                .probe("/x", None, Instant::now() + Duration::from_secs(1))
                .unwrap_err()
                .code,
            "closed"
        );
    }
}
