//! Rust-owned desktop coordinator. Swift submits commands and observes immutable revisions.
pub mod attention;
pub mod browser;
pub mod control;
pub mod control_credentials;
pub mod control_policy;
pub mod persistence;
pub mod sessions;
pub mod targets;
pub mod transfers;
use attention::{
    Attention, AttentionCommand, NativeAttentionAction, NativeNotificationEvent,
    NativeNotificationSnapshot, Notification, NotificationContent,
};
use browser::{
    BrowserDispatchBatch, BrowserHostEvent, BrowserReceipt, BrowserRequest, BrowserRuntime,
};
use targets::{TargetOperation, TargetRequest};
use transfers::{TransferCommand, TransferSlot, TransferSnapshot};
use zmux_remote_runtime::connection::ManagedConnection;
use zmux_workspace_core::targets::TargetError;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;
use zmux_session_runtime::{
    INPUT_CHUNK_BYTES, Launch, Session, SessionError, Snapshot as TerminalSnapshot,
};
use zmux_terminal_adapter::Size;
use zmux_workspace_core::{
    Action, Effect, SessionId, Snapshot as Model, SurfaceId, WorkspaceStore,
};

pub const MAX_TERMINALS: usize = 8;
pub const MAX_BROWSERS: usize = 4;
const MAX_COMMANDS: usize = 32;
const UPDATE_INTERVAL: Duration = Duration::from_millis(16);
const TRANSFER_RECEIPT_TIMEOUT: Duration = Duration::from_secs(2);
const NATIVE_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Shell {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}
impl Default for Shell {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let fallback = "/bin/zsh";
        #[cfg(not(target_os = "macos"))]
        let fallback = "/bin/sh";
        let program = std::env::var("SHELL")
            .ok()
            .filter(|p| p.starts_with('/') && std::path::Path::new(p).is_file())
            .unwrap_or_else(|| fallback.into());
        #[cfg(target_os = "macos")]
        let args = vec!["-l".into()];
        #[cfg(not(target_os = "macos"))]
        let args = vec!["-i".into()];
        Self {
            program,
            args,
            cwd: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_dir()),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Browser {
        request: BrowserRequest,
    },
    #[serde(skip)]
    BrowserHost {
        event: BrowserHostEvent,
    },
    #[serde(skip)]
    BrowserDrain,
    #[serde(skip)]
    NativeAttention {
        action: NativeAttentionAction,
    },
    Session {
        action: sessions::SessionCommand,
    },
    Transfer {
        action: TransferCommand,
    },
    Targeted {
        request: TargetRequest,
    },
    Attention {
        action: AttentionCommand,
    },
    Action {
        action: Action,
    },
    Input {
        session_id: SessionId,
        text: String,
    },
    Wheel {
        session_id: SessionId,
        event: zmux_terminal_adapter::Wheel,
    },
    Scroll {
        session_id: SessionId,
        lines: i32,
    },
    Resize {
        session_id: SessionId,
        rows: u16,
        cols: u16,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub request_id: u64,
    pub payload: Command,
}

#[derive(Debug, Error)]
pub enum DesktopError {
    #[error("{0}")]
    Attention(#[from] attention::AttentionError),
    #[error("{0}")]
    Browser(#[from] browser::BrowserError),
    #[error("{0}")]
    Policy(#[from] control_policy::PolicyError),
    #[error("target surface is not a terminal")]
    NotTerminal,
    #[error("terminal is starting or no longer available")]
    TerminalUnavailable,
    #[error("{0}")]
    Target(#[from] TargetError),
    #[error("desktop command queue is full; command was not accepted")]
    QueueFull,
    #[error("desktop is closed")]
    Closed,
    #[error("desktop snapshot is being published; retry")]
    Busy,
    #[error("desktop command acknowledgement timed out; outcome is unknown")]
    Timeout,
    #[error("{0}")]
    Failure(String),
}
#[derive(Clone, Serialize)]
pub struct Frame {
    pub session_id: SessionId,
    pub process_id: Option<u32>,
    pub starting: bool,
    pub snapshot: Option<Arc<TerminalSnapshot>>,
    pub failure: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub model: Model,
    pub frames: Vec<Frame>,
    pub notifications: Vec<Notification>,
    pub notifications_dropped: u64,
    pub native_notifications: NativeNotificationSnapshot,
    pub uploads: Vec<TransferSnapshot>,
    pub persistence: sessions::PersistenceSnapshot,
    pub browser_epoch: uuid::Uuid,
    pub browser_operations: Vec<BrowserReceipt>,
    pub browser_pending_requests: usize,
    pub last_request_id: u64,
    pub last_error: Option<String>,
    pub closed: bool,
    pub pending_launches: usize,
    pub cleanup_error: Option<String>,
    /// All live terminal frames for local inspection. Native rendering receives only `frames`.
    #[serde(skip)]
    pub control_frames: Vec<Frame>,
}
struct Shared {
    stop: AtomicBool,
    snapshot: Mutex<Arc<Snapshot>>,
}
pub struct Desktop {
    sender: SyncSender<Envelope>,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct Envelope {
    request: Request,
    acknowledgement: Option<SyncSender<Execution>>,
    authorization: Option<control_policy::ControlAdmission>,
}

pub(crate) struct Execution {
    pub result: Result<(), DesktopError>,
    pub snapshot: Arc<Snapshot>,
    pub created_workspaces: Vec<zmux_workspace_core::WorkspaceId>,
    pub created_panes: Vec<zmux_workspace_core::PaneId>,
    pub created_surfaces: Vec<SurfaceId>,
    pub created_uploads: Vec<u64>,
    pub session_operation_id: Option<u64>,
    pub browser_operation_id: Option<u64>,
    pub browser_dispatch: Option<BrowserDispatchBatch>,
}

#[derive(Debug)]
pub struct TransferExecution {
    pub result: Result<(), DesktopError>,
    pub revision: u64,
    pub upload_id: Option<u64>,
}

#[derive(Debug)]
pub struct TransferExecutionError {
    pub error: DesktopError,
    pub outcome_unknown: bool,
}

#[derive(Debug, Serialize)]
pub struct NativeNotificationExecution {
    pub revision: u64,
    /// Native presentation may activate its window only after confirmed Open.
    pub focused_surface_id: Option<SurfaceId>,
}

impl Desktop {
    pub fn new(shell: Shell) -> Result<Self, DesktopError> {
        Self::start(shell, None)
    }

    pub fn with_persistence(shell: Shell, state_directory: PathBuf) -> Result<Self, DesktopError> {
        let persistence = sessions::SessionPersistence::start(state_directory)?;
        Self::start(shell, Some(persistence))
    }

    fn start(
        shell: Shell,
        persistence: Option<sessions::SessionPersistence>,
    ) -> Result<Self, DesktopError> {
        let model = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
        let browser = BrowserRuntime::default();
        let attention = Attention::default();
        let persistence_snapshot = persistence
            .as_ref()
            .map(|p| p.snapshot.clone())
            .unwrap_or_default();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            snapshot: Mutex::new(Arc::new(Snapshot {
                revision: 0,
                model: model.snapshot(),
                frames: Vec::new(),
                notifications: Vec::new(),
                notifications_dropped: 0,
                native_notifications: attention.native_snapshot(&model.snapshot()),
                uploads: Vec::new(),
                persistence: persistence_snapshot,
                browser_epoch: browser.epoch(),
                browser_operations: Vec::new(),
                browser_pending_requests: 0,
                last_request_id: 0,
                last_error: None,
                closed: false,
                pending_launches: 0,
                cleanup_error: None,
                control_frames: Vec::new(),
            })),
        });
        let (sender, receiver) = mpsc::sync_channel(MAX_COMMANDS);
        let (completion_tx, completion_rx) = mpsc::sync_channel(MAX_TERMINALS);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("zmux-workspaces".into())
            .spawn(move || {
                let mut worker = Worker {
                    model,
                    browser,
                    browser_operation_id: None,
                    browser_dispatch: None,
                    persistence,
                    restoring: None,
                    session_operation_id: None,
                    attention,
                    transfers: Vec::new(),
                    next_transfer_id: 1,
                    app_active: false,
                    slots: HashMap::new(),
                    retired: Vec::new(),
                    shell,
                    shared: worker_shared,
                    completion_tx,
                    completion_rx,
                    pending_launches: 0,
                    next_launch_id: 1,
                    last_request_id: 0,
                    last_error: None,
                    cleanup_error: None,
                    revision: 0,
                };
                if worker.persistence.is_none() {
                    worker.starter_workspace();
                }
                worker.publish(false);
                worker.run(receiver);
            })
            .map_err(|e| DesktopError::Failure(format!("start desktop coordinator: {e}")))?;
        Ok(Self {
            sender,
            shared,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn submit(&self, request: Request) -> Result<(), DesktopError> {
        self.validate(&request)?;
        self.send(Envelope {
            request,
            acknowledgement: None,
            authorization: None,
        })
    }

    pub(crate) fn execute_authorized(
        &self,
        request: Request,
        timeout: Duration,
        authorization: control_policy::ControlAdmission,
    ) -> Result<Execution, DesktopError> {
        self.validate(&request)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Envelope {
            request,
            acknowledgement: Some(sender),
            authorization: Some(authorization),
        })?;
        receiver.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => DesktopError::Timeout,
            mpsc::RecvTimeoutError::Disconnected => DesktopError::Closed,
        })
    }

    /// Execute one native file transfer on the workspace worker and return that envelope's
    /// acknowledgement. The caller must remain off the native UI thread for up to two seconds.
    pub fn execute_transfer_receipt(
        &self,
        request: Request,
    ) -> Result<TransferExecution, TransferExecutionError> {
        if request.version != 1 {
            return Err(TransferExecutionError {
                error: DesktopError::Failure(
                    "native transfer receipt requires protocol version 1".into(),
                ),
                outcome_unknown: false,
            });
        }
        self.validate(&request)
            .map_err(|error| TransferExecutionError {
                error,
                outcome_unknown: false,
            })?;
        match &request.payload {
            Command::Transfer {
                action:
                    TransferCommand::PasteFiles {
                        expected_session_id: Some(_),
                        ..
                    },
            } => {}
            _ => {
                return Err(TransferExecutionError {
                    error: DesktopError::Failure(
                        "native transfer receipt requires paste_files with expected_session_id"
                            .into(),
                    ),
                    outcome_unknown: false,
                });
            }
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Envelope {
            request,
            acknowledgement: Some(sender),
            authorization: None,
        })
        .map_err(|error| TransferExecutionError {
            error,
            outcome_unknown: false,
        })?;
        let execution = receiver
            .recv_timeout(TRANSFER_RECEIPT_TIMEOUT)
            .map_err(|error| TransferExecutionError {
                error: match error {
                    mpsc::RecvTimeoutError::Timeout => DesktopError::Timeout,
                    mpsc::RecvTimeoutError::Disconnected => DesktopError::Closed,
                },
                outcome_unknown: true,
            })?;
        debug_assert!(execution.created_uploads.len() <= 1);
        Ok(TransferExecution {
            result: execution.result,
            revision: execution.snapshot.revision,
            upload_id: execution.created_uploads.into_iter().next(),
        })
    }

    /// Trusted native bridge only; execute off the UI thread. Draining is once-only:
    /// reserve response capacity before calling, and never retry a lost evaluation.
    pub fn browser_drain_requests(
        &self,
        timeout: Duration,
    ) -> Result<BrowserDispatchBatch, DesktopError> {
        let execution = self.execute_native(Command::BrowserDrain, timeout)?;
        execution.result?;
        execution.browser_dispatch.ok_or_else(|| {
            DesktopError::Failure("native browser dispatch acknowledgement missing".into())
        })
    }
    /// Trusted host lifecycle/completion path, distinct from socket commands.
    /// This waits for executor acknowledgement and must run off the UI thread.
    pub fn browser_host_event(&self, event: BrowserHostEvent) -> Result<(), DesktopError> {
        self.execute_native(Command::BrowserHost { event }, NATIVE_COMMAND_TIMEOUT)?
            .result
    }
    /// Trusted notification permission/delivery observations, excluded from JSON commands.
    /// Waits for confirmed executor application; call off the native UI thread.
    pub fn native_notification_event(
        &self,
        event: NativeNotificationEvent,
    ) -> Result<(), DesktopError> {
        self.execute_native(
            Command::NativeAttention {
                action: NativeAttentionAction::Event { event },
            },
            NATIVE_COMMAND_TIMEOUT,
        )?
        .result
    }
    /// Confirm exact notification ownership and selection/read before native activation.
    /// A timeout has an unknown outcome and must not trigger activation or an automatic retry.
    pub fn native_notification_action(
        &self,
        action: NativeAttentionAction,
    ) -> Result<NativeNotificationExecution, DesktopError> {
        let focused_surface_id = match &action {
            NativeAttentionAction::Open { target }
            | NativeAttentionAction::OpenInbox { target } => Some(target.surface_id),
            NativeAttentionAction::Dismiss { .. } => None,
            NativeAttentionAction::Event { .. } => {
                return Err(DesktopError::Failure(
                    "use native_notification_event for notification observations".into(),
                ));
            }
        };
        let execution =
            self.execute_native(Command::NativeAttention { action }, NATIVE_COMMAND_TIMEOUT)?;
        execution.result?;
        Ok(NativeNotificationExecution {
            revision: execution.snapshot.revision,
            focused_surface_id,
        })
    }
    fn execute_native(
        &self,
        payload: Command,
        timeout: Duration,
    ) -> Result<Execution, DesktopError> {
        let request = Request {
            version: 1,
            request_id: 0,
            payload,
        };
        self.validate(&request)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.send(Envelope {
            request,
            acknowledgement: Some(sender),
            authorization: None,
        })?;
        receiver.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => DesktopError::Timeout,
            mpsc::RecvTimeoutError::Disconnected => DesktopError::Closed,
        })
    }

    fn validate(&self, request: &Request) -> Result<(), DesktopError> {
        if self.shared.stop.load(Ordering::Acquire) {
            return Err(DesktopError::Closed);
        }
        if let Command::Input { text, .. } = &request.payload
            && (text.is_empty() || text.len() > INPUT_CHUNK_BYTES)
        {
            return Err(DesktopError::Failure(
                "input must contain 1–4096 bytes".into(),
            ));
        }
        if let Command::Resize { rows, cols, .. } = &request.payload {
            Size {
                rows: *rows,
                cols: *cols,
            }
            .validate()
            .map_err(|e| DesktopError::Failure(e.to_string()))?;
        }
        Ok(())
    }

    fn send(&self, envelope: Envelope) -> Result<(), DesktopError> {
        self.sender.try_send(envelope).map_err(|e| match e {
            TrySendError::Full(_) => DesktopError::QueueFull,
            TrySendError::Disconnected(_) => DesktopError::Closed,
        })
    }
    pub fn snapshot(&self) -> Result<Arc<Snapshot>, DesktopError> {
        self.shared
            .snapshot
            .try_lock()
            .map(|s| Arc::clone(&s))
            .map_err(|e| match e {
                std::sync::TryLockError::WouldBlock => DesktopError::Busy,
                std::sync::TryLockError::Poisoned(_) => {
                    DesktopError::Failure("desktop publication lock poisoned".into())
                }
            })
    }
    /// Run explicit shutdown off the native main thread. Cancellation itself is immediate.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), DesktopError> {
        self.shared.stop.store(true, Ordering::Release);
        let deadline = Instant::now() + timeout;
        let mut handle = self
            .worker
            .lock()
            .map_err(|_| DesktopError::Failure("desktop worker lock poisoned".into()))?;
        if let Some(worker) = handle.as_ref() {
            while !worker.is_finished() {
                if Instant::now() >= deadline {
                    return Err(DesktopError::Failure(
                        "desktop shutdown exceeded deadline".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        if let Some(worker) = handle.take() {
            worker
                .join()
                .map_err(|_| DesktopError::Failure("desktop worker panicked".into()))?;
        }
        if let Some(error) = &self.snapshot()?.cleanup_error {
            return Err(DesktopError::Failure(error.clone()));
        }
        Ok(())
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}
struct Slot {
    launch_id: u64,
    session: Option<Session>,
    connection: Option<Arc<ManagedConnection>>,
    cached: Option<Arc<TerminalSnapshot>>,
    starting: bool,
    failure: Option<String>,
    resize: Option<Size>,
    last_notice: u64,
}
struct Completion {
    launch_id: u64,
    id: SessionId,
    result: Result<Session, SessionError>,
    connection: Option<Arc<ManagedConnection>>,
}
struct Worker {
    model: WorkspaceStore,
    browser: BrowserRuntime,
    browser_operation_id: Option<u64>,
    browser_dispatch: Option<BrowserDispatchBatch>,
    persistence: Option<sessions::SessionPersistence>,
    restoring: Option<sessions::PendingRestore>,
    session_operation_id: Option<u64>,
    attention: Attention,
    transfers: Vec<TransferSlot>,
    next_transfer_id: u64,
    app_active: bool,
    slots: HashMap<SessionId, Slot>,
    retired: Vec<Session>,
    shell: Shell,
    shared: Arc<Shared>,
    completion_tx: SyncSender<Completion>,
    completion_rx: Receiver<Completion>,
    pending_launches: usize,
    next_launch_id: u64,
    last_request_id: u64,
    last_error: Option<String>,
    cleanup_error: Option<String>,
    revision: u64,
}
impl Worker {
    fn run(&mut self, receiver: Receiver<Envelope>) {
        while !self.shared.stop.load(Ordering::Acquire) {
            let mut dirty = false;
            match receiver.recv_timeout(UPDATE_INTERVAL) {
                Ok(envelope) => self.command(envelope),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            for _ in 1..MAX_COMMANDS {
                match receiver.try_recv() {
                    Ok(envelope) => self.command(envelope),
                    Err(_) => break,
                }
            }
            while let Ok(completion) = self.completion_rx.try_recv() {
                self.complete(completion);
                dirty = true;
            }
            let mut notices = Vec::new();
            for (id, slot) in &mut self.slots {
                if let Some(session) = &slot.session {
                    if let Some(size) = slot.resize {
                        match session.resize(size) {
                            Ok(()) => slot.resize = None,
                            Err(SessionError::Busy) => {}
                            Err(error) => {
                                slot.resize = None;
                                slot.failure = Some(error.to_string());
                                dirty = true;
                            }
                        }
                    }
                    match session.snapshot() {
                        Ok(snapshot) => {
                            for notice in &snapshot.viewport.notices {
                                if notice.sequence > slot.last_notice {
                                    self.attention.dropped = self.attention.dropped.saturating_add(
                                        notice.sequence.saturating_sub(slot.last_notice + 1),
                                    );
                                    slot.last_notice = notice.sequence;
                                    notices.push((*id, notice.clone()));
                                }
                            }
                            let changed = slot.cached.as_ref().is_none_or(|old| {
                                old.bytes_read != snapshot.bytes_read
                                    || old.viewport.scrollback_offset
                                        != snapshot.viewport.scrollback_offset
                                    || old.state != snapshot.state
                                    || old.viewport.size != snapshot.viewport.size
                            });
                            if changed {
                                slot.cached = Some(snapshot);
                                dirty = true;
                            }
                        }
                        Err(SessionError::Busy) => {}
                        Err(error) => {
                            slot.failure = Some(error.to_string());
                            dirty = true;
                        }
                    }
                }
            }
            for (session, notice) in notices {
                let model = self.model.snapshot();
                if let Some(surface_id) = model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .find(|s| s.session_id == Some(session))
                    .map(|s| s.id)
                {
                    let read = self.surface_is_visible(surface_id);
                    if let Err(error) = self.attention.notify_owned(
                        &model,
                        surface_id,
                        &notice.source,
                        &NotificationContent {
                            title: notice.title.clone(),
                            body: notice.body.clone(),
                            ..Default::default()
                        },
                        read,
                    ) {
                        self.last_error = Some(error.to_string());
                    }
                    dirty = true;
                }
            }
            dirty |= self
                .browser
                .reconcile(&self.model.snapshot(), Instant::now());
            dirty |= self.poll_sessions();
            dirty |= self.attention.reconcile(&self.model.snapshot());
            dirty |= self.poll_transfers();
            self.retired.retain_mut(|session| {
                if !session.is_finished() {
                    return true;
                }
                if let Err(error) = session.shutdown(Duration::ZERO) {
                    self.cleanup_error = Some(error.to_string());
                }
                false
            });
            if dirty {
                self.publish(false);
            }
        }
        self.browser
            .reset("desktop shutdown cancelled browser operations");
        self.attention
            .reset_native("desktop shutdown cancelled native notifications");
        let deadline = Instant::now() + Duration::from_secs(3);
        self.shutdown_persistence(deadline);
        self.cancel_transfers();
        let sessions: Vec<_> = self
            .slots
            .values_mut()
            .filter_map(|s| s.session.take())
            .collect();
        for session in sessions {
            self.retire(session);
        }
        while self.pending_launches != 0 && Instant::now() < deadline {
            match self.completion_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(completion) => {
                    self.pending_launches -= 1;
                    if let Ok(session) = completion.result {
                        self.retire(session);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if self.pending_launches != 0 {
            self.cleanup_error = Some("launch workers exceeded the shutdown deadline".into());
        }
        for session in &mut self.retired {
            if let Err(error) = session.shutdown(deadline.saturating_duration_since(Instant::now()))
            {
                self.cleanup_error = Some(error.to_string());
            }
        }
        while !self.transfers_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !self.transfers_finished() {
            self.cleanup_error = Some("upload workers exceeded the shutdown deadline".into());
        }
        self.retired.clear();
        self.slots.clear();
        if let Err(error) = zmux_remote_runtime::connection::drain_cleanup(
            deadline.saturating_duration_since(Instant::now()),
        ) {
            self.cleanup_error = Some(error.to_string());
        }
        self.publish(true);
    }
    fn retire(&mut self, session: Session) {
        if let Err(error) = session.request_close() {
            eprintln!("zmux terminal cancellation wake: {error}");
        }
        self.retired.push(session);
    }
    fn command(&mut self, envelope: Envelope) {
        let request = envelope.request;
        self.last_request_id = request.request_id;
        let previous_upload_id = self.next_transfer_id;
        self.session_operation_id = None;
        self.browser_operation_id = None;
        self.browser_dispatch = None;
        // Capture creation receipts on the same executor as the mutation. Selection is
        // independent: a no-focus split must still identify its new objects precisely.
        let previous_model = if envelope.acknowledgement.is_some()
            && matches!(
                &request.payload,
                Command::Action { .. } | Command::Targeted { .. }
            ) {
            Some(self.model.snapshot())
        } else {
            None
        };
        let authorization = envelope.authorization;
        let guard = authorization
            .as_ref()
            .map(|admission| admission.check())
            .transpose();
        let result = match guard {
            Err(error) => Err(error.into()),
            Ok(_guard) => self.execute_command(request.payload),
        };
        self.last_error = result.as_ref().err().map(ToString::to_string);
        let snapshot = self.publish(false);
        let (created_workspaces, created_panes, created_surfaces) = previous_model
            .map(|previous| {
                let previous_workspaces: std::collections::HashSet<_> =
                    previous.workspaces.iter().map(|w| w.id).collect();
                let previous_panes: std::collections::HashSet<_> = previous
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .map(|p| p.id)
                    .collect();
                let previous_surfaces: std::collections::HashSet<_> = previous
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .map(|s| s.id)
                    .collect();
                let workspaces = snapshot
                    .model
                    .workspaces
                    .iter()
                    .filter(|w| !previous_workspaces.contains(&w.id))
                    .map(|w| w.id)
                    .collect();
                let panes = snapshot
                    .model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .filter(|p| !previous_panes.contains(&p.id))
                    .map(|p| p.id)
                    .collect();
                let surfaces = snapshot
                    .model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .filter(|s| !previous_surfaces.contains(&s.id))
                    .map(|s| s.id)
                    .collect();
                (workspaces, panes, surfaces)
            })
            .unwrap_or_default();
        if let Some(sender) = envelope.acknowledgement {
            let _ = sender.try_send(Execution {
                result,
                session_operation_id: self.session_operation_id,
                browser_operation_id: self.browser_operation_id,
                browser_dispatch: self.browser_dispatch.take(),
                created_uploads: snapshot
                    .uploads
                    .iter()
                    .filter(|u| u.id >= previous_upload_id)
                    .map(|u| u.id)
                    .collect(),
                snapshot,
                created_workspaces,
                created_panes,
                created_surfaces,
            });
        }
    }
    fn execute_command(&mut self, command: Command) -> Result<(), DesktopError> {
        if self.session_transition_pending()
            && !matches!(
                &command,
                Command::Session { .. }
                    | Command::Attention { .. }
                    | Command::BrowserHost { .. }
                    | Command::BrowserDrain
                    | Command::NativeAttention { .. }
            )
        {
            return Err(DesktopError::Failure(
                "session restore is pending; retry after its receipt completes".into(),
            ));
        }
        match command {
            Command::Browser { request } => {
                self.browser_operation_id = Some(self.browser.admit(
                    &self.model.snapshot(),
                    request,
                    Instant::now(),
                )?);
                Ok(())
            }
            Command::BrowserHost { event } => self
                .browser
                .host_event(&self.model.snapshot(), event, Instant::now())
                .map_err(Into::into),
            Command::BrowserDrain => {
                self.browser_dispatch = Some(
                    self.browser
                        .drain_requests(&self.model.snapshot(), Instant::now()),
                );
                Ok(())
            }
            Command::NativeAttention { action } => self.native_attention_action(action),
            Command::Session { action } => {
                let restore = matches!(&action, sessions::SessionCommand::Restore { .. });
                self.session_command(action)?;
                if restore {
                    self.browser
                        .reset("session restore replaced native browser hosts");
                    self.attention
                        .reset_native("session restore replaced notification targets");
                }
                Ok(())
            }
            Command::Transfer { action } => self.transfer(action),
            Command::Targeted { request } => self.targeted(request),
            Command::Attention { action } => self.attention_action(action),
            Command::Action { action } => {
                let result = self.action(action);
                if result.is_ok() {
                    self.acknowledge_focused();
                }
                result
            }
            Command::Wheel { session_id, event } => self
                .slots
                .get(&session_id)
                .and_then(|s| s.session.as_ref())
                .ok_or_else(|| {
                    DesktopError::Failure("terminal is starting or no longer exists".into())
                })
                .and_then(|s| {
                    s.wheel(event)
                        .map_err(|e| DesktopError::Failure(e.to_string()))
                }),
            Command::Scroll { session_id, lines } => self
                .slots
                .get(&session_id)
                .and_then(|s| s.session.as_ref())
                .ok_or_else(|| {
                    DesktopError::Failure("terminal is starting or no longer exists".into())
                })
                .and_then(|s| {
                    s.scroll(lines)
                        .map_err(|e| DesktopError::Failure(e.to_string()))
                }),
            Command::Input { session_id, text } => self
                .slots
                .get(&session_id)
                .and_then(|s| s.session.as_ref())
                .ok_or_else(|| {
                    DesktopError::Failure("terminal is starting or no longer exists".into())
                })
                .and_then(|s| {
                    s.send_input(text.as_bytes())
                        .map_err(|e| DesktopError::Failure(e.to_string()))
                }),
            Command::Resize {
                session_id,
                rows,
                cols,
            } => match self.slots.get_mut(&session_id) {
                Some(slot) => {
                    slot.resize = Some(Size { rows, cols });
                    Ok(())
                }
                None => Err(DesktopError::Failure("terminal no longer exists".into())),
            },
        }
    }
    fn targeted(&mut self, request: TargetRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        request.validate_revision(&model)?;
        match request.operation {
            TargetOperation::Action { action } => {
                let action = action.resolve(&model, &request.scope, &request.caller)?;
                self.action(action)?;
                self.acknowledge_focused();
                Ok(())
            }
            TargetOperation::PasteFiles { paths } => {
                let target = request.scope.resolve(&model, &request.caller)?;
                self.transfer(TransferCommand::PasteFiles {
                    surface_id: target.surface,
                    expected_session_id: None,
                    paths,
                })
            }
            TargetOperation::Notify { title, body } => {
                let target = request.scope.resolve(&model, &request.caller)?;
                self.attention_action(AttentionCommand::Notify {
                    surface_id: target.surface,
                    title,
                    body,
                })
            }
            TargetOperation::SendText { text } => {
                let target = request.scope.resolve(&model, &request.caller)?;
                let session_id = model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .find(|s| s.id == target.surface)
                    .and_then(|s| s.session_id)
                    .ok_or(DesktopError::NotTerminal)?;
                let session = self
                    .slots
                    .get(&session_id)
                    .and_then(|s| s.session.as_ref())
                    .ok_or(DesktopError::TerminalUnavailable)?;
                session
                    .send_input(text.as_bytes())
                    .map_err(|e| DesktopError::Failure(e.to_string()))
            }
            _ => Err(DesktopError::Failure(
                "read operation cannot mutate the desktop".into(),
            )),
        }
    }
    fn acknowledge_focused(&mut self) {
        // Capture existing IDs at the serialized mutation boundary. Future arrivals
        // are evaluated independently; hidden surfaces keep their unread state.
        let model = self.model.snapshot();
        let ids: Vec<_> = self
            .attention
            .items
            .iter()
            .filter(|n| {
                !n.read
                    && self.surface_is_visible(n.surface_id)
                    && self.attention.open_target(&model, n.id).is_ok()
            })
            .map(|n| n.id)
            .collect();
        for id in ids {
            if let Err(error) = self.attention.read(id) {
                eprintln!("zmux attention acknowledgement: {error}");
            }
        }
    }
    fn surface_is_visible(&self, id: SurfaceId) -> bool {
        let state = self.model.snapshot();
        self.app_active
            && state
                .workspaces
                .iter()
                .filter(|w| Some(w.id) == state.focused_workspace)
                .flat_map(|w| {
                    w.root
                        .panes()
                        .into_iter()
                        .filter(|p| p.id == w.focused_pane)
                })
                .any(|p| p.selected_surface == id)
    }
    fn attention_action(&mut self, command: AttentionCommand) -> Result<(), DesktopError> {
        let result: Result<(), String> = (|| {
            match command {
                AttentionCommand::Notify {
                    surface_id,
                    title,
                    body,
                } => {
                    let state = self.model.snapshot();
                    if !state
                        .workspaces
                        .iter()
                        .flat_map(|w| w.root.panes())
                        .flat_map(|p| &p.surfaces)
                        .any(|s| s.id == surface_id)
                    {
                        return Err(format!("surface {} does not exist", surface_id.0));
                    }
                    self.attention
                        .notify_owned(
                            &state,
                            surface_id,
                            "cli",
                            &NotificationContent {
                                title,
                                body,
                                ..Default::default()
                            },
                            self.surface_is_visible(surface_id),
                        )
                        .map_err(|error| error.to_string())?;
                }
                AttentionCommand::Read { id } => self.attention.read(id)?,
                AttentionCommand::Dismiss { id } => self.attention.dismiss(id)?,
                AttentionCommand::Clear { through_id } => self.attention.clear(through_id),
                AttentionCommand::AppActive { active } => {
                    self.app_active = active;
                    if active {
                        self.acknowledge_focused();
                    }
                }
                AttentionCommand::Open { id } => {
                    let surface_id = self
                        .attention
                        .open_target(&self.model.snapshot(), id)
                        .map_err(|error| error.to_string())?;
                    self.action(Action::SelectSurface { surface_id })
                        .map_err(|e| e.to_string())?;
                    self.attention.read(id)?;
                }
            }
            Ok(())
        })();
        result.map_err(DesktopError::Failure)
    }
    fn native_attention_action(
        &mut self,
        action: NativeAttentionAction,
    ) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        self.attention.reconcile(&model);
        match action {
            NativeAttentionAction::Event { event } => self.attention.native_event(&model, event)?,
            NativeAttentionAction::Open { target } => {
                let surface_id = self.attention.native_open_target(&model, &target)?;
                self.action(Action::SelectSurface { surface_id })?;
                self.attention.native_dismiss(&target)?;
            }
            NativeAttentionAction::OpenInbox { target } => {
                let surface_id = self.attention.native_inbox_target(&model, &target)?;
                self.action(Action::SelectSurface { surface_id })?;
                self.attention.native_dismiss(&target)?;
            }
            NativeAttentionAction::Dismiss { target } => {
                self.attention.native_open_target(&model, &target)?;
                self.attention.native_dismiss(&target)?;
            }
        }
        Ok(())
    }
    pub(crate) fn model_with_session_metadata(&self) -> WorkspaceStore {
        let mut candidate = self.model.clone();
        for (id, slot) in &self.slots {
            if let Some(cwd) = slot
                .cached
                .as_ref()
                .and_then(|s| s.viewport.working_directory.clone())
            {
                candidate.set_session_cwd(*id, cwd);
            }
        }
        candidate
    }
    fn action(&mut self, action: Action) -> Result<(), DesktopError> {
        // Capture observed metadata on a candidate so rejected actions leave the
        // authoritative topology and descriptor state unchanged.
        let mut candidate = self.model_with_session_metadata();
        let transition = candidate
            .apply(action)
            .map_err(|e| DesktopError::Failure(e.to_string()))?;
        let starts = transition
            .effects
            .iter()
            .filter(|e| matches!(e, Effect::StartSession { .. }))
            .count();
        let owned = self.pending_launches
            + self.retired.len()
            + self.slots.values().filter(|s| s.session.is_some()).count();
        if starts > 0
            && (self.slots.len() + starts > MAX_TERMINALS || owned + starts > MAX_TERMINALS)
        {
            return Err(DesktopError::Failure(format!(
                "development build supports at most {MAX_TERMINALS} terminals; close a tab and wait for cleanup first"
            )));
        }
        let browsers = candidate
            .snapshot()
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .filter(|s| s.kind == zmux_workspace_core::SurfaceKind::Browser)
            .count();
        if browsers > MAX_BROWSERS {
            return Err(DesktopError::Failure(format!(
                "development build supports at most {MAX_BROWSERS} browser tabs"
            )));
        }
        self.ensure_reserved(&candidate)?;
        self.model = candidate;
        self.effects(transition.effects);
        Ok(())
    }
    fn effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::StopSession { session_id } => {
                    if let Some(mut slot) = self.slots.remove(&session_id)
                        && let Some(session) = slot.session.take()
                    {
                        self.retire(session);
                    }
                }
                Effect::StartSession {
                    session_id,
                    cwd,
                    remote,
                    ..
                } => {
                    // A preceding launch-worker failure can abort the whole restore group.
                    if !self
                        .launch_model()
                        .snapshot()
                        .workspaces
                        .iter()
                        .flat_map(|w| w.root.panes())
                        .flat_map(|p| &p.surfaces)
                        .any(|s| s.session_id == Some(session_id))
                    {
                        continue;
                    }
                    let launch_id = self.next_launch_id;
                    self.next_launch_id = self
                        .next_launch_id
                        .checked_add(1)
                        .expect("launch generation exhausted");
                    self.slots.insert(
                        session_id,
                        Slot {
                            launch_id,
                            session: None,
                            connection: None,
                            cached: None,
                            starting: true,
                            failure: None,
                            resize: None,
                            last_notice: 0,
                        },
                    );
                    let sender = self.completion_tx.clone();
                    let mut shell = self.shell.clone();
                    if let Some(cwd) = cwd.filter(|_| remote.is_none()) {
                        shell.cwd = Some(PathBuf::from(cwd));
                    }
                    if let Some(cwd) = shell.cwd.as_ref().filter(|_| remote.is_none()) {
                        self.launch_model_mut()
                            .set_session_cwd(session_id, cwd.to_string_lossy().into_owned());
                    }
                    let model = self.launch_model().snapshot();
                    let context = model.workspaces.iter().find_map(|w| {
                        w.root.panes().into_iter().find_map(|p| {
                            p.surfaces.iter().find(|s| s.session_id == Some(session_id)).map(|s| vec![
                                ("ZMUX_WORKSPACE_ID".into(), model.object_uuid(zmux_workspace_core::targets::TargetKind::Workspace, w.id.0).to_string()),
                                ("ZMUX_SURFACE_ID".into(), model.object_uuid(zmux_workspace_core::targets::TargetKind::Surface, s.id.0).to_string()),
                                ("ZMUX_TAB_ID".into(), model.object_uuid(zmux_workspace_core::targets::TargetKind::Surface, s.id.0).to_string()),
                            ])
                        })
                    }).expect("live terminal has an owning surface");
                    self.pending_launches += 1;
                    let result = thread::Builder::new()
                        .name(format!("zmux-launch-{}", session_id.0))
                        .spawn(move || {
                            let result = (|| {
                                let connection = remote
                                    .map(|remote| {
                                        let descriptor = remote.validate().map_err(|e| {
                                            SessionError::Failure(format!(
                                                "invalid SSH connection: {e}"
                                            ))
                                        })?;
                                        ManagedConnection::create(descriptor)
                                            .map(Arc::new)
                                            .map_err(|e| SessionError::Failure(e.to_string()))
                                    })
                                    .transpose()?;
                                if let Some(connection) = &connection {
                                    let process = connection.terminal_spec();
                                    shell.program = "/usr/bin/ssh".into();
                                    shell.args = process.args;
                                }
                                let session = Session::spawn_with_resource(
                                    Launch {
                                        program: shell.program,
                                        args: shell.args,
                                        cwd: shell.cwd,
                                        size: zmux_session_runtime::default_size(),
                                    },
                                    &context,
                                    connection.clone(),
                                )?;
                                Ok((session, connection))
                            })();
                            let (result, connection) = match result {
                                Ok((session, connection)) => (Ok(session), connection),
                                Err(error) => (Err(error), None),
                            };
                            // A dropped coordinator rejects the result; dropping Session cancels its worker.
                            if let Err(error) = sender.send(Completion {
                                launch_id,
                                id: session_id,
                                result,
                                connection,
                            }) {
                                drop(error.0);
                            }
                        });
                    if let Err(error) = result {
                        self.pending_launches -= 1;
                        let slot = self.slots.get_mut(&session_id).expect("new slot");
                        slot.starting = false;
                        slot.failure = Some(format!("launch worker failed: {error}"));
                        self.last_error = slot.failure.clone();
                        if self.restoring.is_some() {
                            self.fail_session_restore("launch worker failed".into());
                        } else {
                            let cleanup = self.model.finish_restore(session_id, false);
                            self.effects(cleanup);
                        }
                    }
                }
            }
        }
    }
    fn complete(&mut self, completion: Completion) {
        self.pending_launches -= 1;
        if self
            .slots
            .get(&completion.id)
            .is_none_or(|slot| slot.launch_id != completion.launch_id)
        {
            if let Ok(session) = completion.result {
                self.retire(session);
            }
            return;
        }
        if self.complete_session_restore(completion.id, &completion.result) {
            if let Some(slot) = self.slots.get_mut(&completion.id) {
                slot.starting = false;
                if let Ok(session) = completion.result {
                    slot.session = Some(session);
                    slot.connection = completion.connection;
                }
            } else if let Ok(session) = completion.result {
                self.retire(session);
            }
            self.commit_session_restore();
            return;
        }
        let success = completion.result.is_ok();
        if let Some(slot) = self.slots.get_mut(&completion.id) {
            slot.starting = false;
            match completion.result {
                Ok(session) => {
                    slot.session = Some(session);
                    slot.connection = completion.connection;
                }
                Err(error) => {
                    let message = format!("terminal launch failed: {error}");
                    slot.failure = Some(message.clone());
                    self.last_error = Some(message);
                }
            }
            let cleanup = self.model.finish_restore(completion.id, success);
            self.effects(cleanup);
        } else if let Ok(session) = completion.result {
            self.retire(session);
        }
        // Results for closed panes are canceled and reaped, never reattached.
    }
    fn publish(&mut self, closed: bool) -> Arc<Snapshot> {
        self.browser
            .reconcile(&self.model.snapshot(), Instant::now());
        self.revision += 1;
        let model = self.model.snapshot();
        self.attention.reconcile(&model);
        let control_frames: Vec<_> = model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| &pane.surfaces)
            .filter_map(|surface| {
                surface
                    .session_id
                    .and_then(|id| self.slots.get(&id))
                    .map(|slot| Frame {
                        session_id: surface.session_id.expect("terminal slot"),
                        process_id: slot.session.as_ref().map(Session::process_id),
                        starting: slot.starting,
                        snapshot: slot.cached.clone(),
                        failure: slot.failure.clone(),
                    })
            })
            .collect();
        let focused_sessions: Vec<_> = model
            .workspaces
            .iter()
            .find(|workspace| Some(workspace.id) == model.focused_workspace)
            .map(|workspace| {
                workspace
                    .root
                    .panes()
                    .into_iter()
                    .filter_map(|pane| pane.session_id())
                    .collect()
            })
            .unwrap_or_default();
        let frames = control_frames
            .iter()
            .filter(|frame| focused_sessions.contains(&frame.session_id))
            .cloned()
            .collect();
        let native_notifications = self.attention.native_snapshot(&model);
        let published = Arc::new(Snapshot {
            revision: self.revision,
            model,
            frames,
            notifications: self.attention.items.clone(),
            notifications_dropped: self.attention.dropped,
            native_notifications,
            uploads: self.transfer_snapshots(),
            persistence: self
                .persistence
                .as_ref()
                .map(|p| p.snapshot.clone())
                .unwrap_or_default(),
            browser_epoch: self.browser.epoch(),
            browser_operations: self.browser.receipts(),
            browser_pending_requests: self.browser.pending_request_count(),
            last_request_id: self.last_request_id,
            last_error: self.last_error.clone(),
            closed,
            pending_launches: self.pending_launches,
            cleanup_error: self.cleanup_error.clone(),
            control_frames,
        });
        match self.shared.snapshot.lock() {
            Ok(mut snapshot) => {
                *snapshot = Arc::clone(&published);
            }
            Err(error) => eprintln!("zmux desktop publication failed: {error}"),
        }
        published
    }
}
