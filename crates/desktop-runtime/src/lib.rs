//! Rust-owned desktop coordinator. Swift submits commands and observes immutable revisions.
pub mod attention;
pub mod browser;
pub mod control;
pub mod control_credentials;
pub mod control_policy;
mod download_paths;
pub mod downloads;
pub mod local_tmux;
pub mod metadata;
pub mod persistence;
pub mod profiles;
mod replies;
pub mod sessions;
pub mod targets;
pub mod transfers;
pub mod windows;
pub mod workspace_order;
use attention::{
    Attention, AttentionCommand, DismissSelection, NativeAttentionAction, NativeNotificationEvent,
    NativeNotificationSnapshot, Notification, NotificationAction, NotificationAddress,
    NotificationContent, NotificationFilter, NotificationMutation, NotificationMutationKind,
    NotificationNavigation, NotificationScope, NotificationSelection,
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
    WorkspaceOrder {
        request: workspace_order::OrderRequest,
    },
    Windows {
        request: windows::WindowRequest,
    },
    #[serde(skip)]
    NativeWindow {
        event: windows::NativeWindowEvent,
    },
    LocalTmux {
        request: local_tmux::Request,
    },
    Browser {
        request: BrowserRequest,
    },
    Downloads {
        request: downloads::DownloadRequest,
    },
    #[serde(skip)]
    NativeDownload {
        request: downloads::NativeDownload,
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
    Profiles {
        action: profiles::ProfileCommand,
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
    Metadata(#[from] metadata::MetadataError),
    #[error("{0}")]
    Browser(#[from] browser::BrowserError),
    #[error("{0}")]
    Download(#[from] downloads::DownloadError),
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
    pub workspace_metadata: Vec<metadata::WorkspaceMetadata>,
    pub notifications_dropped: u64,
    pub notification_cursor: u64,
    pub native_notifications: NativeNotificationSnapshot,
    pub uploads: Vec<TransferSnapshot>,
    pub persistence: sessions::PersistenceSnapshot,
    pub browser_profiles: profiles::ProfileSnapshot,
    pub local_tmux_epoch: uuid::Uuid,
    pub local_tmux_operations: Vec<local_tmux::Receipt>,
    pub browser_epoch: uuid::Uuid,
    pub browser_operations: Vec<BrowserReceipt>,
    pub workspace_identities: Vec<zmux_workspace_core::targets::ObjectIdentity>,
    pub window_epoch: uuid::Uuid,
    pub window_requests: Vec<windows::NativeWindowRequest>,
    pub window_hosts: Vec<windows::WindowHostToken>,
    pub window_operations: Vec<windows::WindowReceipt>,
    pub observed_key_window: Option<zmux_workspace_core::WindowId>,
    pub download_epoch: uuid::Uuid,
    pub downloads: Vec<downloads::DownloadRecord>,
    pub download_waits: Vec<downloads::WaitReceipt>,
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
    reply_deadline: Option<Instant>,
}

pub(crate) struct Execution {
    pub result: Result<(), DesktopError>,
    pub snapshot: Arc<Snapshot>,
    pub created_workspaces: Vec<zmux_workspace_core::WorkspaceId>,
    pub created_panes: Vec<zmux_workspace_core::PaneId>,
    pub created_surfaces: Vec<SurfaceId>,
    pub created_uploads: Vec<u64>,
    pub profile_operation_id: Option<u64>,
    pub tmux_operation_id: Option<u64>,
    pub session_operation_id: Option<u64>,
    pub metadata_result: Option<metadata::WorkspaceMetadata>,
    pub notification_mutation: Option<NotificationMutation>,
    pub notification_navigation: Option<NotificationNavigation>,
    pub notification_reply: Option<attention::NotificationReply>,
    pub browser_operation_id: Option<u64>,
    pub browser_dispatch: Option<BrowserDispatchBatch>,
    pub download_result: Option<serde_json::Value>,
    pub window_result: Option<serde_json::Value>,
    pub workspace_order_result: Option<serde_json::Value>,
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

#[derive(Debug)]
pub struct ProfileAdmissionError {
    pub error: DesktopError,
    pub outcome_unknown: bool,
}

#[derive(Debug, Serialize)]
pub struct NativeNotificationExecution {
    pub revision: u64,
    pub focused_window_id: Option<zmux_workspace_core::WindowId>,
    /// Native presentation may activate its window only after confirmed Open.
    pub focused_workspace_id: Option<zmux_workspace_core::WorkspaceId>,
    pub focused_surface_id: Option<SurfaceId>,
    pub navigation: Option<NotificationNavigation>,
    pub reply: Option<attention::NotificationReply>,
}

/// Native/embedded hosts select installation state and the opt-in tmux profile.
/// Environment resolution remains deferred to background operation/launch workers.
#[derive(Clone, Default)]
pub struct DesktopOptions {
    pub state_directory: Option<PathBuf>,
    pub local_tmux_profile: local_tmux::Profile,
}
impl Desktop {
    pub fn new(shell: Shell) -> Result<Self, DesktopError> {
        Self::with_options(shell, DesktopOptions::default())
    }
    pub fn with_persistence(shell: Shell, state_directory: PathBuf) -> Result<Self, DesktopError> {
        Self::with_options(
            shell,
            DesktopOptions {
                state_directory: Some(state_directory),
                ..Default::default()
            },
        )
    }
    pub fn with_options(shell: Shell, options: DesktopOptions) -> Result<Self, DesktopError> {
        let persistence = options
            .state_directory
            .clone()
            .map(sessions::SessionPersistence::start)
            .transpose()?;
        Self::start(
            shell,
            persistence,
            options.state_directory,
            options.local_tmux_profile,
        )
    }
    fn start(
        shell: Shell,
        persistence: Option<sessions::SessionPersistence>,
        profile_directory: Option<PathBuf>,
        tmux_profile: local_tmux::Profile,
    ) -> Result<Self, DesktopError> {
        let model = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
        let browser = BrowserRuntime::default();
        let downloads = downloads::DownloadRuntime::default();
        let windows = windows::WindowRuntime::default();
        let profiles = profiles::ProfileRuntime::start(profile_directory)?;
        let local_tmux = local_tmux::Runtime::new(tmux_profile);
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
                workspace_metadata: Vec::new(),
                notifications_dropped: 0,
                notification_cursor: 0,
                native_notifications: attention.native_snapshot(&model.snapshot()),
                uploads: Vec::new(),
                persistence: persistence_snapshot,
                browser_profiles: profiles.snapshot.clone(),
                local_tmux_epoch: local_tmux.epoch,
                local_tmux_operations: Vec::new(),
                browser_epoch: browser.epoch(),
                browser_operations: Vec::new(),
                workspace_identities: Vec::new(),
                window_epoch: windows.epoch,
                window_requests: Vec::new(),
                window_hosts: Vec::new(),
                window_operations: Vec::new(),
                observed_key_window: None,
                download_epoch: downloads.epoch,
                downloads: Vec::new(),
                download_waits: Vec::new(),
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
                    local_tmux,
                    tmux_operation_id: None,
                    model,
                    browser,
                    downloads,
                    windows,
                    window_result: None,
                    workspace_order_result: None,
                    download_result: None,
                    profiles,
                    profile_operation_id: None,
                    browser_operation_id: None,
                    browser_dispatch: None,
                    persistence,
                    restoring: None,
                    session_operation_id: None,
                    attention,
                    metadata: metadata::MetadataStore::default(),
                    metadata_result: None,
                    notification_mutation: None,
                    notification_navigation: None,
                    notification_reply: None,
                    pending_replies: Vec::new(),
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
            reply_deadline: None,
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
            reply_deadline: None,
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
            reply_deadline: None,
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

    pub fn native_window_event(
        &self,
        event: windows::NativeWindowEvent,
    ) -> Result<serde_json::Value, DesktopError> {
        let execution =
            self.execute_native(Command::NativeWindow { event }, NATIVE_COMMAND_TIMEOUT)?;
        execution.result?;
        execution
            .window_result
            .ok_or_else(|| DesktopError::Failure("native window acknowledgement missing".into()))
    }

    /// Trusted download lifecycle channel. Run off the UI thread and never replay
    /// a missing acknowledgement; admission/publication may already have happened.
    pub fn native_download(
        &self,
        request: downloads::NativeDownload,
    ) -> Result<serde_json::Value, DesktopError> {
        let execution =
            self.execute_native(Command::NativeDownload { request }, NATIVE_COMMAND_TIMEOUT)?;
        execution.result?;
        execution
            .download_result
            .ok_or_else(|| DesktopError::Failure("native download acknowledgement missing".into()))
    }

    /// Admit one durable profile creation and return that exact executor receipt.
    /// Call off the UI thread; an acknowledgement timeout must never be replayed.
    pub fn native_profile_add(
        &self,
        name: String,
    ) -> Result<profiles::ProfileReceipt, ProfileAdmissionError> {
        let execution = self
            .execute_native(
                Command::Profiles {
                    action: profiles::ProfileCommand::Add { name },
                },
                NATIVE_COMMAND_TIMEOUT,
            )
            .map_err(|error| ProfileAdmissionError {
                outcome_unknown: matches!(error, DesktopError::Timeout | DesktopError::Closed),
                error,
            })?;
        execution.result.map_err(|error| ProfileAdmissionError {
            error,
            outcome_unknown: false,
        })?;
        execution
            .snapshot
            .browser_profiles
            .operations
            .iter()
            .find(|receipt| Some(receipt.operation_id) == execution.profile_operation_id)
            .cloned()
            .ok_or_else(|| ProfileAdmissionError {
                error: DesktopError::Failure("profile admission receipt is missing".into()),
                outcome_unknown: true,
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
        let opens = match &action {
            NativeAttentionAction::Open { .. }
            | NativeAttentionAction::OpenInbox { .. }
            | NativeAttentionAction::JumpToUnread => true,
            NativeAttentionAction::Dismiss { .. } | NativeAttentionAction::Reply { .. } => false,
            NativeAttentionAction::Event { .. } => {
                return Err(DesktopError::Failure(
                    "use native_notification_event for notification observations".into(),
                ));
            }
        };
        let execution =
            self.execute_native(Command::NativeAttention { action }, NATIVE_COMMAND_TIMEOUT)?;
        execution.result?;
        let opens = execution
            .notification_navigation
            .as_ref()
            .map_or(opens, |navigation| navigation.opened);
        let focused = opens
            .then(|| {
                execution
                    .snapshot
                    .model
                    .workspaces
                    .iter()
                    .find(|workspace| {
                        Some(workspace.id) == execution.snapshot.model.focused_workspace
                    })
            })
            .flatten();
        Ok(NativeNotificationExecution {
            revision: execution.snapshot.revision,
            focused_window_id: opens
                .then_some(execution.snapshot.model.focused_window)
                .flatten(),
            focused_workspace_id: focused.map(|workspace| workspace.id),
            focused_surface_id: focused.and_then(|workspace| {
                workspace
                    .root
                    .panes()
                    .into_iter()
                    .find(|pane| pane.id == workspace.focused_pane)
                    .map(|pane| pane.selected_surface)
            }),
            navigation: execution.notification_navigation,
            reply: execution.notification_reply,
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
        let reply_deadline = matches!(
            &request.payload,
            Command::NativeAttention {
                action: NativeAttentionAction::Reply { .. }
            }
        )
        .then(|| Instant::now() + replies::REPLY_WRITE_TIMEOUT);
        self.send(Envelope {
            request,
            acknowledgement: Some(sender),
            authorization: None,
            reply_deadline,
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
    cancellation: Arc<AtomicBool>,
    launch_id: u64,
    session: Option<Session>,
    connection: Option<Arc<ManagedConnection>>,
    cached: Option<Arc<TerminalSnapshot>>,
    starting: bool,
    failure: Option<String>,
    resize: Option<Size>,
    last_notice: u64,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
    }
}
struct Completion {
    launch_id: u64,
    id: SessionId,
    result: Result<Session, SessionError>,
    connection: Option<Arc<ManagedConnection>>,
    cleanup: Option<Session>,
}
fn notification_filter(
    model: &Model,
    scope: &zmux_workspace_core::targets::TargetScope,
    caller: &zmux_workspace_core::targets::CallerContext,
    selection: NotificationScope,
) -> Result<NotificationFilter, DesktopError> {
    use attention::AttentionError;
    use zmux_workspace_core::targets::{CallerContext, TargetScope};
    if scope.pane.is_some() {
        return Err(AttentionError::InvalidSelector(
            "notification bulk operations accept workspace/surface selectors, not pane selectors",
        )
        .into());
    }
    if matches!(selection, NotificationScope::Caller)
        && scope.workspace.is_none()
        && scope.surface.is_none()
    {
        let resolved = scope.resolve(model, caller)?;
        return Ok(if caller.surface.is_some() || caller.workspace.is_none() {
            NotificationFilter::Surface {
                identity_namespace: model.identity_namespace,
                surface_id: resolved.surface,
            }
        } else {
            NotificationFilter::Workspace {
                identity_namespace: model.identity_namespace,
                workspace_id: resolved.workspace,
            }
        });
    }
    let mut resolved_scope = TargetScope {
        window: scope.window.clone(),
        workspace: scope.workspace.clone().or_else(|| caller.workspace.clone()),
        surface: scope.surface.clone(),
        ..Default::default()
    };
    if resolved_scope.workspace.is_none() {
        return Err(AttentionError::InvalidSelector(
            "workspace context is required for scoped notification operations",
        )
        .into());
    }
    if matches!(selection, NotificationScope::Surface) {
        resolved_scope.surface = resolved_scope.surface.or_else(|| caller.surface.clone());
        if resolved_scope.surface.is_none() {
            return Err(AttentionError::InvalidSelector(
                "surface selection requires a surface target",
            )
            .into());
        }
    }
    let target = resolved_scope.resolve(model, &CallerContext::default())?;
    Ok(if resolved_scope.surface.is_some() {
        NotificationFilter::Surface {
            identity_namespace: model.identity_namespace,
            surface_id: target.surface,
        }
    } else {
        NotificationFilter::Workspace {
            identity_namespace: model.identity_namespace,
            workspace_id: target.workspace,
        }
    })
}

struct Worker {
    local_tmux: local_tmux::Runtime,
    tmux_operation_id: Option<u64>,
    profiles: profiles::ProfileRuntime,
    profile_operation_id: Option<u64>,
    model: WorkspaceStore,
    browser: BrowserRuntime,
    downloads: downloads::DownloadRuntime,
    windows: windows::WindowRuntime,
    window_result: Option<serde_json::Value>,
    workspace_order_result: Option<serde_json::Value>,
    download_result: Option<serde_json::Value>,
    browser_operation_id: Option<u64>,
    browser_dispatch: Option<BrowserDispatchBatch>,
    persistence: Option<sessions::SessionPersistence>,
    restoring: Option<sessions::PendingRestore>,
    session_operation_id: Option<u64>,
    attention: Attention,
    metadata: metadata::MetadataStore,
    metadata_result: Option<metadata::WorkspaceMetadata>,
    notification_mutation: Option<NotificationMutation>,
    notification_navigation: Option<NotificationNavigation>,
    notification_reply: Option<attention::NotificationReply>,
    pending_replies: Vec<replies::PendingReply>,
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
            dirty |= self.downloads.poll(&self.model.snapshot(), Instant::now());
            match self.windows.reconcile(&self.model.snapshot()) {
                Ok(changed) => dirty |= changed,
                Err(error) => self.last_error = Some(error.to_string()),
            }
            dirty |= self.profiles.poll();
            dirty |= self.poll_local_tmux();
            if !self.profiles.snapshot.loading {
                dirty |= self.poll_sessions();
            }
            dirty |= self.attention.reconcile(&self.model.snapshot());
            dirty |= self.poll_transfers();
            dirty |= self.poll_replies();
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
        self.cancel_replies();
        self.local_tmux.cancel_all();
        self.attention
            .reset_native("desktop shutdown cancelled native notifications");
        for slot in self.slots.values() {
            slot.cancellation.store(true, Ordering::Release);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.local_tmux.worker_count() != 0 && Instant::now() < deadline {
            self.local_tmux.drain();
            thread::sleep(Duration::from_millis(10));
        }
        if self.local_tmux.worker_count() != 0 {
            self.cleanup_error=Some("local tmux workers exceeded shutdown deadline; owner creation outcome may be unknown".into());
        }
        if let Err(error) = self.profiles.shutdown(deadline) {
            self.cleanup_error = Some(error.to_string());
        }
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
                Ok(mut completion) => {
                    if let Some(session) = completion.cleanup.take() {
                        self.retire(session);
                    }
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
        if let Err(error) = self.downloads.shutdown(deadline) {
            self.cleanup_error = Some(error);
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
        let mut acknowledgement = envelope.acknowledgement;
        self.last_request_id = request.request_id;
        let previous_upload_id = self.next_transfer_id;
        self.profile_operation_id = None;
        self.tmux_operation_id = None;
        self.session_operation_id = None;
        self.notification_mutation = None;
        self.metadata_result = None;
        self.notification_navigation = None;
        self.notification_reply = None;
        self.browser_operation_id = None;
        self.browser_dispatch = None;
        self.download_result = None;
        self.window_result = None;
        self.workspace_order_result = None;
        // Capture creation receipts on the same executor as the mutation. Selection is
        // independent: a no-focus split must still identify its new objects precisely.
        let previous_model = if acknowledgement.is_some()
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
            Ok(_guard) => match request.payload {
                Command::NativeAttention {
                    action: NativeAttentionAction::Reply { target, text },
                } => self.admit_reply(target, text, envelope.reply_deadline, &mut acknowledgement),
                command => self.execute_command(command),
            },
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
        if let Some(sender) = acknowledgement {
            let _ = sender.try_send(Execution {
                result,
                profile_operation_id: self.profile_operation_id,
                tmux_operation_id: self.tmux_operation_id,
                session_operation_id: self.session_operation_id,
                metadata_result: self.metadata_result.take(),
                notification_mutation: self.notification_mutation.take(),
                notification_navigation: self.notification_navigation.take(),
                notification_reply: self.notification_reply.take(),
                browser_operation_id: self.browser_operation_id,
                browser_dispatch: self.browser_dispatch.take(),
                download_result: self.download_result.take(),
                window_result: self.window_result.take(),
                workspace_order_result: self.workspace_order_result.take(),
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
        if self.windows.terminating()
            && !matches!(
                &command,
                Command::NativeWindow { .. }
                    | Command::NativeAttention {
                        action: NativeAttentionAction::Event { .. }
                    }
                    | Command::NativeDownload { .. }
                    | Command::BrowserHost { .. }
                    | Command::BrowserDrain
                    | Command::Windows {
                        request: windows::WindowRequest {
                            action: windows::WindowAction::List | windows::WindowAction::Current,
                            ..
                        }
                    }
            )
        {
            return Err(DesktopError::Failure(
                "application termination has already been requested".into(),
            ));
        }
        if self.session_transition_pending()
            && !matches!(
                &command,
                Command::Session { .. }
                    | Command::Attention { .. }
                    | Command::BrowserHost { .. }
                    | Command::BrowserDrain
                    | Command::NativeDownload { .. }
                    | Command::NativeWindow { .. }
                    | Command::NativeAttention { .. }
            )
        {
            return Err(DesktopError::Failure(
                "session restore is pending; retry after its receipt completes".into(),
            ));
        }
        match command {
            command @ (Command::Windows { .. } | Command::NativeWindow { .. }) => {
                self.execute_window_command(command)
            }
            Command::WorkspaceOrder { request } => self.workspace_order(request),
            Command::LocalTmux { request } => self.local_tmux_command(request),
            Command::Downloads { request } => {
                let model = self.model.snapshot();
                let target = request.scope.resolve(&model, &request.caller)?;
                if !model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .any(|s| {
                        s.id == target.surface
                            && s.kind == zmux_workspace_core::SurfaceKind::Browser
                    })
                {
                    return Err(browser::BrowserError::NotBrowser.into());
                }
                self.download_result = Some(match request.action {
                    downloads::DownloadAction::List { limit } => {
                        let mut result = self.downloads.list(
                            model.object_uuid(
                                zmux_workspace_core::targets::TargetKind::Surface,
                                target.surface.0,
                            ),
                            limit,
                        )?;
                        result["surface_id"] = serde_json::json!(target.surface);
                        result["surface_uuid"] = serde_json::json!(model.object_uuid(
                            zmux_workspace_core::targets::TargetKind::Surface,
                            target.surface.0
                        ));
                        result["workspace_id"] = serde_json::json!(target.workspace);
                        result["workspace_uuid"] = serde_json::json!(model.object_uuid(
                            zmux_workspace_core::targets::TargetKind::Workspace,
                            target.workspace.0
                        ));
                        result
                    }
                    downloads::DownloadAction::Wait { path, timeout_ms } => {
                        serde_json::to_value(self.downloads.wait(
                            &model,
                            target.surface,
                            path,
                            timeout_ms,
                            Instant::now(),
                        )?)
                        .map_err(|e| DesktopError::Failure(e.to_string()))?
                    }
                    downloads::DownloadAction::Cancel { download_id } => {
                        self.downloads.cancel(
                            model.object_uuid(
                                zmux_workspace_core::targets::TargetKind::Surface,
                                target.surface.0,
                            ),
                            download_id,
                        )?;
                        serde_json::json!({"accepted": true})
                    }
                });
                Ok(())
            }
            Command::NativeDownload { request } => {
                self.download_result = Some(match request {
                    downloads::NativeDownload::Begin {
                        owner,
                        token,
                        filename,
                    } => {
                        self.browser.check_owner(&owner)?;
                        serde_json::to_value(self.downloads.begin(
                            &self.model.snapshot(),
                            owner,
                            token,
                            filename,
                            Instant::now(),
                        )?)
                        .map_err(|e| DesktopError::Failure(e.to_string()))?
                    }
                    other => self.downloads.update(other)?,
                });
                Ok(())
            }
            Command::Profiles { action } => {
                self.profile_operation_id = Some(self.profiles.admit(action)?);
                Ok(())
            }
            Command::Browser { request } => {
                self.browser_operation_id = Some(self.browser.admit(
                    &self.model.snapshot(),
                    request,
                    Instant::now(),
                )?);
                Ok(())
            }
            Command::BrowserHost { event } => {
                let detached = if let BrowserHostEvent::Detach { owner } = &event {
                    Some(owner.host_id)
                } else {
                    None
                };
                if let Some(action) =
                    self.browser
                        .host_event(&self.model.snapshot(), event, Instant::now())?
                {
                    self.action(action)?;
                }
                if let Some(host) = detached {
                    self.downloads.detach(host);
                }
                Ok(())
            }
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
            Command::Action {
                action: Action::ObserveBrowser { .. },
            } => Err(browser::BrowserError::Invalid(
                "browser observations require a trusted native document owner",
            )
            .into()),
            Command::Action { action } => {
                if Self::window_action_is_private(&action) {
                    return Err(DesktopError::Failure(
                        "window lifecycle requires acknowledged window admission".into(),
                    ));
                }
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
    fn targeted(&mut self, mut request: TargetRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        request.validate_revision(&model)?;
        if request.caller.local_tmux.is_some() {
            let explicit = request.scope.workspace.is_some()
                || request.scope.pane.is_some()
                || request.scope.surface.is_some();
            if explicit {
                // Explicit notification workspace attribution may use a unique
                // ambient surface, but ambiguity cannot veto the hard scope.
                request.caller = if matches!(&request.operation, TargetOperation::Notify { .. })
                    && request.scope.workspace.is_some()
                    && request.scope.pane.is_none()
                    && request.scope.surface.is_none()
                {
                    request.caller.bind_local_tmux(&model).unwrap_or_default()
                } else {
                    Default::default()
                };
            } else {
                request.caller = request.caller.bind_local_tmux(&model)?;
            }
        }
        match request.operation {
            TargetOperation::Action { mut action } => {
                if let zmux_workspace_core::targets::TargetAction::CreateBrowser {
                    profile: Some(selector),
                    ..
                } = &mut action
                {
                    *selector = self.profiles.resolve(selector)?.to_string();
                }
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
            TargetOperation::Notify {
                title,
                subtitle,
                body,
                desktop,
                reply,
            } => {
                let address =
                    targets::notification_address(&model, &request.scope, &request.caller)?;
                self.notify_notification(
                    address,
                    NotificationContent {
                        title,
                        subtitle,
                        body,
                        desktop,
                        reply,
                    },
                )
            }
            TargetOperation::Metadata { action } => {
                let workspace =
                    targets::metadata_workspace(&model, &request.scope, &request.caller)?;
                self.metadata.reconcile(&model);
                self.metadata_result = Some(self.metadata.apply(workspace, action)?);
                Ok(())
            }
            TargetOperation::Notifications { action } => {
                self.scoped_notifications(&model, &request.scope, &request.caller, action)
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
                    && self
                        .attention
                        .open_target(&model, n.id)
                        .is_ok_and(|address| self.notification_is_visible(address))
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
                .filter(|w| {
                    state.windows.iter().any(|window| {
                        Some(window.id) == self.windows.observed_key
                            && window.selected_workspace == Some(w.id)
                    })
                })
                .flat_map(|w| {
                    w.root
                        .panes()
                        .into_iter()
                        .filter(|p| p.id == w.focused_pane)
                })
                .any(|p| p.selected_surface == id)
    }
    fn notification_is_visible(&self, address: NotificationAddress) -> bool {
        match address {
            NotificationAddress::Workspace(id) => {
                self.app_active
                    && self.model.snapshot().windows.iter().any(|window| {
                        Some(window.id) == self.windows.observed_key
                            && window.selected_workspace == Some(id)
                    })
            }
            NotificationAddress::Surface(id) => self.surface_is_visible(id),
        }
    }
    fn select_notification(&mut self, address: NotificationAddress) -> Result<(), DesktopError> {
        self.action(match address {
            NotificationAddress::Workspace(workspace_id) => {
                Action::SelectWorkspace { workspace_id }
            }
            NotificationAddress::Surface(surface_id) => Action::SelectSurface { surface_id },
        })
    }
    fn notify_notification(
        &mut self,
        address: NotificationAddress,
        content: NotificationContent,
    ) -> Result<(), DesktopError> {
        let read = self.notification_is_visible(address);
        let id =
            self.attention
                .notify_owned(&self.model.snapshot(), address, "cli", &content, read)?;
        self.notification_mutation = Some(
            self.attention
                .mutation_receipt(vec![id], self.attention.cursor()),
        );
        Ok(())
    }
    fn open_notification(
        &mut self,
        target: &attention::NotificationTarget,
        banner: bool,
    ) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        let address = if banner {
            self.attention.native_open_target(&model, target)?
        } else {
            self.attention.native_inbox_target(&model, target)?
        };
        self.select_notification(address)?;
        self.notification_mutation = Some(self.attention.apply_mutation(
            &NotificationFilter::Id(target.notification_id),
            NotificationMutationKind::MarkRead,
            None,
            None,
        )?);
        Ok(())
    }
    fn jump_to_unread(&mut self) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        self.attention.reconcile(&model);
        let candidate = self
            .attention
            .items
            .iter()
            .rev()
            .filter(|item| !item.read)
            .find_map(|item| {
                item.target
                    .as_ref()
                    .filter(|target| self.attention.native_inbox_target(&model, target).is_ok())
                    .map(|target| (target.clone(), item.uuid))
            });
        let navigation = if let Some((target, uuid)) = candidate {
            self.open_notification(&target, false)?;
            NotificationNavigation {
                opened: true,
                notification_id: Some(target.notification_id),
                notification_uuid: Some(uuid),
                target: Some(target),
            }
        } else {
            NotificationNavigation {
                opened: false,
                notification_id: None,
                notification_uuid: None,
                target: None,
            }
        };
        self.notification_navigation = Some(navigation);
        Ok(())
    }
    fn attention_action(&mut self, command: AttentionCommand) -> Result<(), DesktopError> {
        match command {
            AttentionCommand::Notify {
                surface_id,
                title,
                subtitle,
                body,
                desktop,
                reply,
            } => {
                self.notify_notification(
                    NotificationAddress::Surface(surface_id),
                    NotificationContent {
                        title,
                        subtitle,
                        body,
                        desktop,
                        reply,
                    },
                )?;
            }
            AttentionCommand::Read { id } => {
                let id = self.attention.resolve(id)?;
                self.notification_mutation = Some(self.attention.apply_mutation(
                    &NotificationFilter::Id(id),
                    NotificationMutationKind::MarkRead,
                    None,
                    None,
                )?);
            }
            AttentionCommand::Dismiss { id } => {
                let id = self.attention.resolve(id)?;
                self.notification_mutation = Some(self.attention.apply_mutation(
                    &NotificationFilter::Id(id),
                    NotificationMutationKind::Dismiss,
                    None,
                    None,
                )?);
            }
            AttentionCommand::Clear { through_id } => {
                self.notification_mutation = Some(self.attention.apply_mutation(
                    &NotificationFilter::All,
                    NotificationMutationKind::Clear,
                    Some(through_id),
                    None,
                )?);
            }
            AttentionCommand::AppActive { active } => {
                self.app_active = active;
                if active {
                    self.acknowledge_focused();
                }
            }
            AttentionCommand::Open { id } => {
                let id = self.attention.resolve(id)?;
                let target = self
                    .attention
                    .find(id)
                    .map_err(DesktopError::Failure)?
                    .target
                    .clone()
                    .ok_or(attention::AttentionError::StaleTarget)?;
                self.open_notification(&target, false)?;
            }
            AttentionCommand::JumpToUnread => self.jump_to_unread()?,
        }
        Ok(())
    }
    fn scoped_notifications(
        &mut self,
        model: &Model,
        scope: &zmux_workspace_core::targets::TargetScope,
        caller: &zmux_workspace_core::targets::CallerContext,
        action: NotificationAction,
    ) -> Result<(), DesktopError> {
        use attention::AttentionError;
        self.attention.reconcile(model);
        let global = || -> Result<NotificationFilter, AttentionError> {
            if scope.window.is_some()
                || scope.workspace.is_some()
                || scope.pane.is_some()
                || scope.surface.is_some()
            {
                return Err(AttentionError::InvalidSelector(
                    "ID/global notification operations do not accept target selectors",
                ));
            }
            Ok(NotificationFilter::All)
        };
        let (filter, kind, through_id, expected_epoch) = match action {
            NotificationAction::MarkRead { selection } => match selection {
                NotificationSelection::Id { id } => {
                    global()?;
                    let id = self.attention.resolve(id)?;
                    (
                        NotificationFilter::Id(id),
                        NotificationMutationKind::MarkRead,
                        None,
                        None,
                    )
                }
                NotificationSelection::All => {
                    (global()?, NotificationMutationKind::MarkRead, None, None)
                }
                NotificationSelection::Workspace => {
                    if scope.workspace.is_none() {
                        return Err(AttentionError::InvalidSelector(
                            "mark-read workspace selection requires an explicit workspace",
                        )
                        .into());
                    }
                    (
                        notification_filter(model, scope, caller, NotificationScope::Workspace)?,
                        NotificationMutationKind::MarkRead,
                        None,
                        None,
                    )
                }
            },
            NotificationAction::Dismiss { selection } => match selection {
                DismissSelection::Id { id } => {
                    global()?;
                    let id = self.attention.resolve(id)?;
                    (
                        NotificationFilter::Id(id),
                        NotificationMutationKind::Dismiss,
                        None,
                        None,
                    )
                }
                DismissSelection::AllRead => {
                    (global()?, NotificationMutationKind::DismissRead, None, None)
                }
            },
            NotificationAction::Clear {
                scope: selection,
                through_id,
                expected_epoch,
            } => {
                let filter = match selection {
                    NotificationScope::All => global()?,
                    _ => notification_filter(model, scope, caller, selection)?,
                };
                (
                    filter,
                    NotificationMutationKind::Clear,
                    through_id,
                    expected_epoch,
                )
            }
        };
        self.notification_mutation =
            Some(
                self.attention
                    .apply_mutation(&filter, kind, through_id, expected_epoch)?,
            );
        Ok(())
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
                self.open_notification(&target, true)?;
            }
            NativeAttentionAction::OpenInbox { target } => {
                self.open_notification(&target, false)?;
            }
            NativeAttentionAction::Dismiss { target } => {
                self.attention.native_open_target(&model, &target)?;
                self.attention.native_dismiss(&target)?;
            }
            NativeAttentionAction::JumpToUnread => self.jump_to_unread()?,
            NativeAttentionAction::Reply { .. } => {
                return Err(DesktopError::Failure(
                    "notification replies require an acknowledged native envelope".into(),
                ));
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
        let move_context = if let Action::MoveWorkspaceToWindow {
            workspace_id,
            destination_window_id,
            index,
            focus,
        } = &action
        {
            let model = self.model.snapshot();
            let source = model
                .windows
                .iter()
                .find(|w| w.workspace_ids.contains(workspace_id))
                .ok_or_else(|| DesktopError::Failure("source workspace does not exist".into()))?
                .id;
            if !model.windows.iter().any(|w| w.id == *destination_window_id)
                || self.windows.host(*destination_window_id).is_none()
            {
                return Err(DesktopError::Failure(
                    "workspace move destination native host is not ready".into(),
                ));
            }
            Some((
                *workspace_id,
                source,
                *destination_window_id,
                *focus && (source != *destination_window_id || index.is_some()),
            ))
        } else {
            None
        };
        let focus_requested = matches!(
            &action,
            Action::FocusWindow { .. }
                | Action::FocusPane { .. }
                | Action::SelectWorkspace { .. }
                | Action::SelectSurface { .. }
                | Action::NavigateWorkspace { .. }
                | Action::NavigateSurface { .. }
                | Action::NavigatePane { .. }
                | Action::FocusPaneDirection { .. }
                | Action::NavigateFocus { .. }
                | Action::NavigateFocusInWindow { .. }
        );
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
            + self.local_tmux.reserved.len()
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
        self.profiles.validate_model(&candidate.snapshot())?;
        self.ensure_reserved(&candidate)?;
        let mut move_windows = None;
        let native_focus = if let Some((_, _, destination, true)) = move_context {
            let mut windows = self.windows.clone();
            let receipt = windows.request_move_focus(&candidate.snapshot(), destination)?;
            move_windows = Some(windows);
            Some(receipt)
        } else {
            None
        };
        self.model = candidate;
        if let Some(windows) = move_windows {
            self.windows = windows;
        }
        self.effects(transition.effects);
        if let Some((workspace, source, destination, focus)) = move_context {
            let model = self.model.snapshot();
            self.window_result = Some(serde_json::json!({
                "state":"topology_committed", "workspace_id":workspace,
                "workspace_uuid":model.object_uuid(zmux_workspace_core::targets::TargetKind::Workspace, workspace.0),
                "source_window_id":source, "destination_window_id":destination,
                "source_window_uuid":model.object_uuid(zmux_workspace_core::targets::TargetKind::Window, source.0),
                "destination_window_uuid":model.object_uuid(zmux_workspace_core::targets::TargetKind::Window, destination.0),
                "changed_window":source != destination, "focus_requested":focus,
                "native_focus":native_focus, "model_revision":model.revision,
            }));
        }
        if focus_requested
            && let Some(window) = self.model.snapshot().focused_window
            && self.windows.host(window).is_some()
        {
            self.windows.request(
                &self.model.snapshot(),
                window,
                windows::WindowKind::Focus,
                true,
            )?;
        }
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
                    local_tmux,
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
                    let cancellation = Arc::new(AtomicBool::new(false));
                    self.slots.insert(
                        session_id,
                        Slot {
                            cancellation: cancellation.clone(),
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
                    if let Some(cwd) = cwd.filter(|_| remote.is_none() && local_tmux.is_none()) {
                        shell.cwd = Some(PathBuf::from(cwd));
                    }
                    if let Some(cwd) = shell
                        .cwd
                        .as_ref()
                        .filter(|_| remote.is_none() && local_tmux.is_none())
                    {
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
                    let tmux_profile = self.local_tmux.profile.clone();
                    let binding = model
                        .workspaces
                        .iter()
                        .find_map(|w| {
                            w.root.panes().into_iter().find_map(|p| {
                                p.surfaces
                                    .iter()
                                    .find(|s| s.session_id == Some(session_id))
                                    .map(|s| zmux_remote_runtime::local_multiplexer::GuiBinding {
                                        identity_namespace: model.identity_namespace,
                                        workspace: model.object_uuid(
                                            zmux_workspace_core::targets::TargetKind::Workspace,
                                            w.id.0,
                                        ),
                                        surface: model.object_uuid(
                                            zmux_workspace_core::targets::TargetKind::Surface,
                                            s.id.0,
                                        ),
                                    })
                            })
                        })
                        .expect("live terminal has binding");
                    self.pending_launches += 1;
                    let result = thread::Builder::new()
                        .name(format!("zmux-launch-{}", session_id.0))
                        .spawn(move || {
                            let mut failed_client = None;
                            let result = (|| {
                                if let Some(identity) = local_tmux {
                                    if remote.is_some() {
                                        return Err(SessionError::Failure(
                                            "terminal cannot be both SSH and local tmux".into(),
                                        ));
                                    }
                                    return match local_tmux::launch_client(
                                        tmux_profile,
                                        *identity,
                                        &context,
                                        binding,
                                        cancellation,
                                    ) {
                                        Ok(session) => Ok((session, None)),
                                        Err(failure) => {
                                            failed_client = failure.cleanup;
                                            Err(failure.error)
                                        }
                                    };
                                }
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
                                // An app launched from a persistent shell still owns a
                                // fresh ordinary/SSH terminal, not the parent's tmux pane.
                                // Keep the executable override as user configuration.
                                let removals = [
                                    "TMUX",
                                    "TMUX_PANE",
                                    "ZMUX_LOCAL_TMUX",
                                    "ZMUX_LOCAL_TMUX_PROFILE",
                                ]
                                .map(std::ffi::OsString::from);
                                let session = Session::spawn_with_resource_and_removals(
                                    Launch {
                                        program: shell.program,
                                        args: shell.args,
                                        cwd: shell.cwd,
                                        size: zmux_session_runtime::default_size(),
                                    },
                                    &context,
                                    &removals,
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
                                cleanup: failed_client,
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
    fn complete(&mut self, mut completion: Completion) {
        if let Some(session) = completion.cleanup.take() {
            self.retire(session);
        }
        self.pending_launches -= 1;
        self.complete_local_tmux(&completion);
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
        if let Err(error) = self.windows.reconcile(&self.model.snapshot()) {
            self.last_error = Some(error.to_string());
        }
        self.revision += 1;
        let model = self.model.snapshot();
        self.attention.reconcile(&model);
        self.metadata.reconcile(&model);
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
            .filter(|workspace| {
                model
                    .windows
                    .iter()
                    .any(|window| window.selected_workspace == Some(workspace.id))
            })
            .flat_map(|workspace| {
                workspace
                    .root
                    .panes()
                    .into_iter()
                    .filter_map(|pane| pane.session_id())
                    .collect::<Vec<_>>()
            })
            .collect();
        let frames = control_frames
            .iter()
            .filter(|frame| focused_sessions.contains(&frame.session_id))
            .cloned()
            .collect();
        let native_notifications = self.attention.native_snapshot(&model);
        let workspace_identities = model
            .target_catalog()
            .into_iter()
            .filter(|identity| identity.kind == zmux_workspace_core::targets::TargetKind::Workspace)
            .collect();
        let published = Arc::new(Snapshot {
            revision: self.revision,
            workspace_identities,
            model,
            frames,
            notifications: self.attention.items.clone(),
            workspace_metadata: self.metadata.snapshot(),
            notifications_dropped: self.attention.dropped,
            notification_cursor: self.attention.cursor(),
            native_notifications,
            uploads: self.transfer_snapshots(),
            persistence: self
                .persistence
                .as_ref()
                .map(|p| p.snapshot.clone())
                .unwrap_or_default(),
            browser_profiles: self.profiles.snapshot.clone(),
            local_tmux_epoch: self.local_tmux.epoch,
            local_tmux_operations: self.local_tmux.receipts.clone(),
            browser_epoch: self.browser.epoch(),
            browser_operations: self.browser.receipts(),
            window_epoch: self.windows.epoch,
            window_requests: self.windows.requests(),
            window_hosts: self.windows.hosts(),
            window_operations: self.windows.receipts(),
            observed_key_window: self.windows.observed_key,
            download_epoch: self.downloads.epoch,
            downloads: self.downloads.records(),
            download_waits: self.downloads.wait_receipts(),
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
