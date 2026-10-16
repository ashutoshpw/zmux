//! Versioned, same-user local control for a running desktop.

use crate::control_policy::{
    ControlAdmission, ControlGate, ControlOrigin, ControlPolicy, SocketPassword,
};
use crate::targets::{TargetOperation, TargetRequest};
use crate::{Command, Desktop, DesktopError, Request, Snapshot};
use nix::unistd::{Uid, geteuid};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;
use zmux_session_runtime::State;
use zmux_workspace_core::targets::TargetKind;
use zmux_workspace_core::{Action, Axis, PaneId, SessionId, SurfaceId, WorkspaceId};

pub const CONTROL_VERSION: u32 = 2;
pub const MAX_CONTROL_REQUEST_BYTES: usize = 16_384;
pub const MAX_CONTROL_RESPONSE_BYTES: usize = 1_048_576;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_millis(500);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(250);
const ACCEPT_POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub version: u32,
    pub request_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<SocketPassword>,
    pub command: ControlCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlCommand {
    WorkspaceGroup {
        request: crate::workspace_groups::GroupRequest,
    },
    AgentMessages {
        request: crate::agent_messages::MessageRequest,
    },
    AgentMessagesStatus {
        request_id: uuid::Uuid,
    },
    SurfaceOrder {
        request: crate::surface_order::OrderRequest,
    },
    WorkspaceOrder {
        request: crate::workspace_order::OrderRequest,
    },
    Windows {
        request: crate::windows::WindowRequest,
    },
    WindowStatus {
        epoch: uuid::Uuid,
        operation_id: uuid::Uuid,
    },
    LocalTmux {
        request: crate::local_tmux::Request,
    },
    LocalTmuxStatus {
        operation_id: u64,
        epoch: uuid::Uuid,
    },
    BrowserProfiles {
        action: crate::profiles::ProfileCommand,
    },
    BrowserProfileReconcile {
        profile: uuid::Uuid,
        intent: uuid::Uuid,
    },
    BrowserProfileReconcileStatus {
        id: uuid::Uuid,
    },
    BrowserProfileIntentStatus {
        profile: uuid::Uuid,
        intent: uuid::Uuid,
    },
    BrowserProfilesStatus {
        #[serde(default)]
        epoch: Option<uuid::Uuid>,
        #[serde(default)]
        operation_id: Option<u64>,
    },
    Browser {
        request: crate::browser::BrowserRequest,
    },
    Downloads {
        request: crate::downloads::DownloadRequest,
    },
    DownloadWaitStatus {
        epoch: uuid::Uuid,
        ticket_id: uuid::Uuid,
    },
    BrowserStatus {
        operation_id: u64,
    },
    Session {
        action: crate::sessions::SessionCommand,
    },
    SessionStatus {
        #[serde(default)]
        operation_id: Option<u64>,
    },
    Targeted {
        request: TargetRequest,
    },
    ListUploads,
    Transfer {
        action: crate::transfers::TransferCommand,
    },
    Status,
    Current {
        #[serde(default = "crate::inspection::default_limit")]
        limit: usize,
    },
    Capabilities,
    /// Shared action transport; the same enum is used by the native bridge.
    Action {
        action: Action,
    },
    ListPaneSurfaces {
        pane_id: PaneId,
    },
    RecentlyClosed,
    Attention {
        action: crate::attention::AttentionCommand,
    },
    ListNotifications,
    ListWorkspaces,
    ListPanes {
        #[serde(default)]
        workspace_id: Option<WorkspaceId>,
    },
    NewWorkspace {
        name: String,
    },
    RenameWorkspace {
        workspace_id: WorkspaceId,
        name: String,
    },
    FocusPane {
        pane_id: PaneId,
    },
    SplitPane {
        pane_id: PaneId,
        axis: Axis,
    },
    ClosePane {
        pane_id: PaneId,
    },
    SendText {
        pane_id: PaneId,
        text: String,
    },
    ReadScreen {
        pane_id: PaneId,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlResponse {
    pub version: u32,
    pub request_id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}

impl ControlResponse {
    fn success(request_id: u64, result: Value) -> Self {
        Self {
            version: CONTROL_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn failure(request_id: u64, code: &str, message: impl Into<String>) -> Self {
        Self {
            version: CONTROL_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(ControlError {
                code: code.into(),
                message: message.into().chars().take(512).collect(),
            }),
        }
    }
}

#[derive(Debug, Error)]
pub enum ControlServerError {
    #[error("control socket path must be absolute: {0}")]
    RelativePath(PathBuf),
    #[error("control socket path has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("control socket directory is not private and owned by this user: {0}")]
    InsecureDirectory(PathBuf),
    #[error("control socket already has a running server: {0}")]
    AlreadyRunning(PathBuf),
    #[error("refusing to replace non-socket control path: {0}")]
    UnsafeExistingPath(PathBuf),
    #[error("{operation} {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("control listener thread panicked")]
    ThreadPanicked,
    #[error("control listener shutdown exceeded its deadline")]
    ShutdownTimeout,
}

/// One server is associated with one native desktop context. It accepts one bounded request at a
/// time on its own thread; terminal and workspace work remains on the existing runtime workers.
pub struct ControlServer {
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    gate: ControlGate,
}

impl ControlServer {
    pub fn start(path: &Path, desktop: Arc<Desktop>) -> Result<Self, ControlServerError> {
        Self::start_with_policy(path, desktop, ControlPolicy::default())
    }

    pub fn start_with_policy(
        path: &Path,
        desktop: Arc<Desktop>,
        policy: ControlPolicy,
    ) -> Result<Self, ControlServerError> {
        Self::start_for_uid(path, desktop, geteuid(), policy)
    }

    /// Revokes future admissions and waits for an already executing durable transaction.
    /// Call from a worker; filesystem commit barriers must never block a UI/executor thread.
    pub fn set_policy(
        &self,
        policy: ControlPolicy,
    ) -> Result<(), crate::control_policy::PolicyError> {
        self.gate.replace(policy)
    }

    fn start_for_uid(
        path: &Path,
        desktop: Arc<Desktop>,
        allowed_uid: Uid,
        policy: ControlPolicy,
    ) -> Result<Self, ControlServerError> {
        if !path.is_absolute() {
            return Err(ControlServerError::RelativePath(path.to_owned()));
        }
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| ControlServerError::MissingParent(path.to_owned()))?;
        prepare_private_directory(parent, allowed_uid)?;
        remove_stale_socket(path, allowed_uid)?;
        let listener = UnixListener::bind(path).map_err(|source| ControlServerError::Io {
            operation: "bind",
            path: path.to_owned(),
            source,
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ControlServerError::Io {
                operation: "protect",
                path: path.to_owned(),
                source,
            }
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|source| ControlServerError::Io {
                operation: "configure",
                path: path.to_owned(),
                source,
            })?;
        let identity = socket_identity(path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_path = path.to_owned();
        let gate = ControlGate::new(policy);
        let worker_gate = gate.clone();
        let worker = thread::Builder::new()
            .name("zmux-local-control".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            serve_connection(stream, &desktop, allowed_uid, &worker_gate)
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL);
                        }
                        Err(error) => {
                            eprintln!("zmux local control accept failed: {error}");
                            thread::sleep(ACCEPT_POLL);
                        }
                    }
                }
                remove_owned_socket(&worker_path, identity);
            })
            .map_err(|source| ControlServerError::Io {
                operation: "start listener for",
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            stop,
            worker: Mutex::new(Some(worker)),
            gate,
        })
    }

    /// Stop accepting new external commands without waiting for the listener.
    pub fn request_shutdown(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn shutdown(&self, timeout: Duration) -> Result<(), ControlServerError> {
        self.stop.store(true, Ordering::Release);
        let deadline = Instant::now() + timeout;
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| ControlServerError::ThreadPanicked)?;
        if let Some(handle) = worker.as_ref() {
            while !handle.is_finished() {
                if Instant::now() >= deadline {
                    return Err(ControlServerError::ShutdownTimeout);
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        if let Some(handle) = worker.take() {
            handle
                .join()
                .map_err(|_| ControlServerError::ThreadPanicked)?;
        }
        Ok(())
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn prepare_private_directory(path: &Path, owner: Uid) -> Result<(), ControlServerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir()
                || metadata.uid() != owner.as_raw()
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(ControlServerError::InsecureDirectory(path.to_owned()));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|source| ControlServerError::Io {
                operation: "create",
                path: path.to_owned(),
                source,
            })?;
            let metadata = fs::symlink_metadata(path).map_err(|source| ControlServerError::Io {
                operation: "inspect",
                path: path.to_owned(),
                source,
            })?;
            if !metadata.file_type().is_dir() || metadata.uid() != owner.as_raw() {
                return Err(ControlServerError::InsecureDirectory(path.to_owned()));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
                ControlServerError::Io {
                    operation: "protect",
                    path: path.to_owned(),
                    source,
                }
            })?;
        }
        Err(source) => {
            return Err(ControlServerError::Io {
                operation: "inspect",
                path: path.to_owned(),
                source,
            });
        }
    }
    Ok(())
}

fn remove_stale_socket(path: &Path, owner: Uid) -> Result<(), ControlServerError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ControlServerError::Io {
                operation: "inspect",
                path: path.to_owned(),
                source,
            });
        }
    };
    if !metadata.file_type().is_socket() || metadata.uid() != owner.as_raw() {
        return Err(ControlServerError::UnsafeExistingPath(path.to_owned()));
    }
    if UnixStream::connect(path).is_ok() {
        return Err(ControlServerError::AlreadyRunning(path.to_owned()));
    }
    fs::remove_file(path).map_err(|source| ControlServerError::Io {
        operation: "remove stale socket at",
        path: path.to_owned(),
        source,
    })
}

fn socket_identity(path: &Path) -> Result<(u64, u64), ControlServerError> {
    fs::symlink_metadata(path)
        .map(|metadata| (metadata.dev(), metadata.ino()))
        .map_err(|source| ControlServerError::Io {
            operation: "inspect",
            path: path.to_owned(),
            source,
        })
}

fn remove_owned_socket(path: &Path, identity: (u64, u64)) {
    let belongs_to_server = fs::symlink_metadata(path)
        .map(|metadata| (metadata.dev(), metadata.ino()) == identity)
        .unwrap_or(false);
    if belongs_to_server
        && let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!("zmux local control socket cleanup failed: {error}");
    }
}

fn serve_connection(
    mut stream: UnixStream,
    desktop: &Desktop,
    allowed_uid: Uid,
    gate: &ControlGate,
) {
    // Accepted sockets inherit O_NONBLOCK on BSD/macOS. Frame I/O uses bounded blocking
    // reads on this dedicated listener worker, independent of the nonblocking accept loop.
    if let Err(error) = stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT)))
        .and_then(|()| stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT)))
    {
        eprintln!("zmux local control connection setup failed: {error}");
        return;
    }
    let response = match peer_uid(&stream) {
        Ok(uid) if uid == allowed_uid => match read_request(&mut stream) {
            Ok(request) => handle_request(desktop, request, gate),
            Err(ReadRequestError::Oversized) => ControlResponse::failure(
                0,
                "request_too_large",
                format!("request exceeds {MAX_CONTROL_REQUEST_BYTES} bytes"),
            ),
            Err(ReadRequestError::Io(error)) => {
                ControlResponse::failure(0, "invalid_frame", error.to_string())
            }
            Err(ReadRequestError::Json(error)) => {
                ControlResponse::failure(0, "invalid_request", error.to_string())
            }
        },
        Ok(_) => ControlResponse::failure(0, "unauthorized", "peer user does not own zmux"),
        Err(error) => ControlResponse::failure(
            0,
            "peer_identity_unavailable",
            format!("could not verify peer user: {error}"),
        ),
    };
    if let Err(error) = write_response(&mut stream, &response) {
        eprintln!("zmux local control response failed: {error}");
    }
}

enum ReadRequestError {
    Oversized,
    Io(io::Error),
    Json(serde_json::Error),
}

fn read_request(stream: &mut UnixStream) -> Result<ControlRequest, ReadRequestError> {
    let mut length = [0_u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(ReadRequestError::Io)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_CONTROL_REQUEST_BYTES {
        return Err(ReadRequestError::Oversized);
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .map_err(ReadRequestError::Io)?;
    serde_json::from_slice(&bytes).map_err(ReadRequestError::Json)
}

fn write_response(stream: &mut UnixStream, response: &ControlResponse) -> io::Result<()> {
    let bytes = serde_json::to_vec(response).map_err(io::Error::other)?;
    if bytes.len() > MAX_CONTROL_RESPONSE_BYTES {
        return Err(io::Error::other("control response exceeded its bound"));
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)
}

fn handle_request(
    desktop: &Desktop,
    request: ControlRequest,
    gate: &ControlGate,
) -> ControlResponse {
    let version = request.version;
    let mut response = handle_versioned_request(desktop, request, gate);
    // The new server accepts the original v1 request shape and responds in that version.
    // Password-capable clients require v2; the native C ABI has its own version contract.
    if version == 1 || version == CONTROL_VERSION {
        response.version = version;
    }
    response
}

fn handle_versioned_request(
    desktop: &Desktop,
    request: ControlRequest,
    gate: &ControlGate,
) -> ControlResponse {
    if request.version != 1 && request.version != CONTROL_VERSION {
        return ControlResponse::failure(
            request.request_id,
            "unsupported_version",
            format!("expected local control protocol version {CONTROL_VERSION}"),
        );
    }
    if request.request_id == 0 {
        return ControlResponse::failure(0, "invalid_request_id", "request_id must be nonzero");
    }
    let request_id = request.request_id;
    let admission = match gate.admit(
        ControlOrigin::LocalOwner,
        &request.command,
        request.password.as_ref(),
    ) {
        Ok(admission) => admission,
        Err(error) => return ControlResponse::failure(request_id, error.code(), error.to_string()),
    };
    let read = crate::control_policy::is_read(&request.command);
    let capabilities = matches!(request.command, ControlCommand::Capabilities);
    match dispatch(desktop, request, &admission) {
        Ok(result) if !read => ControlResponse::success(request_id, result),
        Ok(mut result) => match admission.check() {
            Err(error) => ControlResponse::failure(request_id, error.code(), error.to_string()),
            Ok(guard) => {
                if capabilities {
                    result["authorization"] = json!(guard.policy().authentication());
                    result["access"] = json!(guard.policy().access());
                }
                ControlResponse::success(request_id, result)
            }
        },
        Err((code, message)) => ControlResponse::failure(request_id, code, message),
    }
}

fn dispatch(
    desktop: &Desktop,
    request: ControlRequest,
    admission: &ControlAdmission,
) -> Result<Value, (&'static str, String)> {
    match request.command {
        ControlCommand::AgentMessages {
            request: message_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::AgentMessages {
                request: message_request,
            },
        ),
        ControlCommand::AgentMessagesStatus {
            request_id: message_id,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::AgentMessagesStatus {
                request_id: message_id,
            },
        ),
        ControlCommand::Windows {
            request: window_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Windows {
                request: window_request,
            },
        ),
        ControlCommand::WindowStatus {
            epoch,
            operation_id,
        } => {
            let snapshot = current_snapshot(desktop)?;
            snapshot
                .window_operations
                .iter()
                .find(|receipt| receipt.epoch == epoch && receipt.operation_id == operation_id)
                .map(|receipt| json!(receipt))
                .ok_or((
                    "window_operation_not_found",
                    "window operation expired or belongs to another runtime".into(),
                ))
        }
        ControlCommand::SurfaceOrder {
            request: order_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::SurfaceOrder {
                request: order_request,
            },
        ),
        ControlCommand::WorkspaceGroup {
            request: group_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::WorkspaceGroup {
                request: group_request,
            },
        ),
        ControlCommand::WorkspaceOrder {
            request: order_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::WorkspaceOrder {
                request: order_request,
            },
        ),
        ControlCommand::LocalTmux {
            request: tmux_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::LocalTmux {
                request: tmux_request,
            },
        ),
        ControlCommand::LocalTmuxStatus {
            operation_id,
            epoch,
        } => {
            let snapshot = current_snapshot(desktop)?;
            if snapshot.local_tmux_epoch != epoch {
                return Err((
                    "local_tmux_operation_not_found",
                    "local tmux runtime owner changed".into(),
                ));
            }
            snapshot
                .local_tmux_operations
                .iter()
                .find(|r| r.operation_id == operation_id)
                .map(|r| json!(r))
                .ok_or((
                    "local_tmux_operation_not_found",
                    "local tmux receipt expired or missing".into(),
                ))
        }
        ControlCommand::Downloads {
            request: download_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Downloads {
                request: download_request,
            },
        ),
        ControlCommand::DownloadWaitStatus { epoch, ticket_id } => {
            let snapshot = current_snapshot(desktop)?;
            snapshot
                .download_waits
                .iter()
                .find(|t| t.epoch == epoch && t.ticket_id == ticket_id)
                .map(|t| json!(t))
                .ok_or((
                    "download_wait_not_found",
                    "Download wait ticket expired or belongs to another runtime".into(),
                ))
        }
        ControlCommand::BrowserProfiles { action } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Profiles { action },
        ),
        ControlCommand::BrowserProfileReconcile { profile, intent } => {
            let _guard = admission.check().map_err(|e| (e.code(), e.to_string()))?;
            desktop
                .profile_reconcile(profile, intent)
                .map(|v| json!(v))
                .map_err(desktop_error)
        }
        ControlCommand::BrowserProfileReconcileStatus { id } => desktop
            .profile_reconcile_status(id)
            .map(|v| json!(v))
            .map_err(desktop_error),
        ControlCommand::BrowserProfileIntentStatus { profile, intent } => desktop
            .profile_intent_status(profile, intent)
            .map(|v| json!(v))
            .map_err(desktop_error),
        ControlCommand::BrowserProfilesStatus {
            operation_id,
            epoch,
        } => {
            let snapshot = current_snapshot(desktop)?;
            if operation_id.is_some() && epoch != Some(snapshot.browser_profiles.epoch) {
                return Err((
                    "profile_operation_not_found",
                    "profile runtime owner changed or is missing".into(),
                ));
            }
            if let Some(id) = operation_id {
                snapshot
                    .browser_profiles
                    .operations
                    .iter()
                    .find(|op| op.operation_id == id)
                    .map(|op| json!(op))
                    .ok_or((
                        "profile_operation_not_found",
                        "profile receipt expired or missing".into(),
                    ))
            } else {
                Ok(json!(snapshot.browser_profiles))
            }
        }
        ControlCommand::Session { action } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Session { action },
        ),
        ControlCommand::SessionStatus { operation_id } => {
            let snapshot = current_snapshot(desktop)?;
            if let Some(id) = operation_id {
                let operation = snapshot
                    .persistence
                    .operations
                    .iter()
                    .find(|op| op.operation_id == id)
                    .ok_or_else(|| {
                        (
                            "operation_not_found",
                            "session operation receipt expired or does not exist".into(),
                        )
                    })?;
                Ok(json!(operation))
            } else {
                Ok(json!(snapshot.persistence))
            }
        }
        ControlCommand::Browser {
            request: browser_request,
        } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Browser {
                request: browser_request,
            },
        ),
        ControlCommand::BrowserStatus { operation_id } => current_snapshot(desktop)?
            .browser_operations
            .iter()
            .find(|op| op.operation_id == operation_id)
            .map(|op| json!(op))
            .ok_or_else(|| {
                (
                    "browser_operation_not_found",
                    "browser operation receipt expired or does not exist".into(),
                )
            }),
        ControlCommand::Targeted {
            request: target_request,
        } => match target_request.operation {
            TargetOperation::ReadScreen
            | TargetOperation::ListPanes
            | TargetOperation::ListPaneSurfaces => {
                let snapshot = current_snapshot(desktop)?;
                target_request
                    .validate_revision(&snapshot.model)
                    .map_err(|e| (e.code(), e.to_string()))?;
                if matches!(target_request.operation, TargetOperation::ListPanes)
                    && target_request.scope.window.is_none()
                    && target_request.caller.window.is_none()
                    && target_request.scope.workspace.is_none()
                    && target_request.scope.pane.is_none()
                    && target_request.scope.surface.is_none()
                    && target_request.caller.workspace.is_none()
                    && target_request.caller.surface.is_none()
                    && target_request.caller.local_tmux.is_none()
                {
                    return list_panes(&snapshot, None);
                }
                let target = target_request
                    .scope
                    .resolve(&snapshot.model, &target_request.caller)
                    .map_err(|e| (e.code(), e.to_string()))?;
                match target_request.operation {
                    TargetOperation::ReadScreen => read_screen_surface(&snapshot, target.surface),
                    TargetOperation::ListPanes => list_panes(&snapshot, Some(target.workspace)),
                    _ => list_surfaces(&snapshot, target.pane),
                }
            }
            TargetOperation::OpenItems { .. } => {
                open_items(desktop, admission, request.request_id, target_request)
            }
            _ => mutate(
                desktop,
                admission,
                request.request_id,
                Command::Targeted {
                    request: target_request,
                },
            ),
        },
        ControlCommand::ListUploads => Ok(json!({"uploads": current_snapshot(desktop)?.uploads})),
        ControlCommand::Transfer { action } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Transfer { action },
        ),
        ControlCommand::Action { action } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action { action },
        ),
        ControlCommand::Attention { action } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Attention { action },
        ),
        ControlCommand::ListNotifications => {
            let snapshot = current_snapshot(desktop)?;
            Ok(
                json!({"revision": snapshot.revision, "epoch": snapshot.native_notifications.epoch, "cursor": snapshot.notification_cursor, "notifications": snapshot.notifications,
                "dropped": snapshot.notifications_dropped, "unread": snapshot.notifications.iter().filter(|n| !n.read).count()}),
            )
        }
        ControlCommand::RecentlyClosed => {
            let snapshot = current_snapshot(desktop)?;
            Ok(json!({"revision": snapshot.revision, "closed_items": snapshot.model.closed_items}))
        }
        ControlCommand::ListPaneSurfaces { pane_id } => {
            let snapshot = current_snapshot(desktop)?;
            list_surfaces(&snapshot, pane_id)
        }
        ControlCommand::Capabilities => Ok(json!({
            "protocol": "zmux-local-control", "version": CONTROL_VERSION,
            "accepted_request_versions": [1, CONTROL_VERSION],
            "model_version": zmux_workspace_core::MODEL_VERSION,
            "authorization": "same_uid", "remote_relay": false,
            "target_kinds": ["workspace", "pane", "surface"],
            "target_formats": ["uuid", "typed_ref", "legacy_id", "index"],
            "revision_guard": "expected_model_revision",
            "max_request_bytes": MAX_CONTROL_REQUEST_BYTES,
            "max_response_bytes": MAX_CONTROL_RESPONSE_BYTES,
            "max_terminals": crate::MAX_TERMINALS,
            "max_browsers": crate::MAX_BROWSERS,
            "agent_messages": {
                "durable_local_inbox": true, "max_body_bytes": crate::agent_messages::MAX_BODY_BYTES,
                "max_messages":128,"max_page":crate::agent_messages::MAX_PAGE,
                "lease_ttl_ms":10000,"max_claim":8,"max_pending_operations":8,
                "settled_request_horizon":256,"delivery":"at_least_once",
                "consumer_identity":"local_capability_not_authenticated_provider",
                "provider_hooks":false,"wake":false,"native_inbox":false,
            },
            "file_panel": {
                "max_file_surfaces": crate::MAX_FILE_SURFACES,
                "max_open_items": crate::files::MAX_OPEN_ITEMS,
                "max_text_bytes": crate::file_io::MAX_TEXT_BYTES,
                "max_preview_bytes": crate::file_io::MAX_PREVIEW_BYTES,
                "poll_interval_ms": desktop.files_config().poll_interval.as_millis() as u64,
                "open_probe_timeout_ms": desktop.files_config().probe_timeout.as_millis() as u64,
            },
            "browser_automation": {
                "native_host_required": true,
                "document_generation_fencing": true,
                "max_pending_operations": crate::browser::MAX_PENDING_BROWSER_OPERATIONS,
                "max_result_bytes": crate::browser::MAX_BROWSER_RESULT_BYTES,
                "max_script_bytes": crate::browser::MAX_SCRIPT_BYTES,
                "max_timeout_ms": crate::browser::MAX_OPERATION_TIMEOUT_MS,
            },
        })),
        ControlCommand::Status => {
            let snapshot = current_snapshot(desktop)?;
            Ok(status(&snapshot))
        }
        ControlCommand::Current { limit } => {
            let snapshot = current_snapshot(desktop)?;
            crate::inspection::current(&snapshot, limit)
        }
        ControlCommand::ListWorkspaces => {
            let snapshot = current_snapshot(desktop)?;
            Ok(json!({
                "revision": snapshot.revision,
                "focused_window": snapshot.model.focused_window,
                "focused_workspace": snapshot.model.focused_workspace,
                "windows": snapshot.model.windows,
                "workspaces": snapshot.model.workspaces,
                "identity_namespace": snapshot.model.identity_namespace,
                "model_revision": snapshot.model.revision,
                "targets": snapshot.model.target_catalog(),
            }))
        }
        ControlCommand::ListPanes { workspace_id } => {
            let snapshot = current_snapshot(desktop)?;
            list_panes(&snapshot, workspace_id)
        }
        ControlCommand::ReadScreen { pane_id } => {
            let snapshot = current_snapshot(desktop)?;
            read_screen(&snapshot, pane_id)
        }
        ControlCommand::NewWorkspace { name } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action {
                action: Action::CreateWorkspace { name },
            },
        ),
        ControlCommand::RenameWorkspace { workspace_id, name } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action {
                action: Action::RenameWorkspace { workspace_id, name },
            },
        ),
        ControlCommand::FocusPane { pane_id } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action {
                action: Action::FocusPane { pane_id },
            },
        ),
        ControlCommand::SplitPane { pane_id, axis } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action {
                action: Action::SplitPane { pane_id, axis },
            },
        ),
        ControlCommand::ClosePane { pane_id } => mutate(
            desktop,
            admission,
            request.request_id,
            Command::Action {
                action: Action::ClosePane { pane_id },
            },
        ),
        ControlCommand::SendText { pane_id, text } => {
            let pane = pane_id.0.to_string().parse().map_err(
                |e: zmux_workspace_core::targets::TargetError| (e.code(), e.to_string()),
            )?;
            mutate(
                desktop,
                admission,
                request.request_id,
                Command::Targeted {
                    request: TargetRequest {
                        scope: zmux_workspace_core::targets::TargetScope {
                            pane: Some(pane),
                            ..Default::default()
                        },
                        operation: TargetOperation::SendText { text },
                        ..Default::default()
                    },
                },
            )
        }
    }
}

fn list_surfaces(snapshot: &Snapshot, pane_id: PaneId) -> Result<Value, (&'static str, String)> {
    let pane = snapshot
        .model
        .workspaces
        .iter()
        .find_map(|w| {
            w.root
                .panes()
                .into_iter()
                .find(|p| p.id == pane_id)
                .map(|pane| (w, pane))
        })
        .ok_or_else(|| ("unknown_pane", format!("pane {} does not exist", pane_id.0)))?;
    let (workspace, pane) = pane;
    let window = owning_window(&snapshot.model, workspace.id);
    let surfaces: Vec<_> = pane.surfaces.iter().map(|surface| {
                let frame = surface.session_id.and_then(|id| frame_for(snapshot, id));
                json!({"window_id":window, "window_uuid":snapshot.model.object_uuid(TargetKind::Window, window.0), "workspace_id":workspace.id, "pane_id":pane_id, "surface_id": surface.id, "surface_uuid": snapshot.model.object_uuid(TargetKind::Surface, surface.id.0), "surface_ref": format!("surface:{}", surface.id.0), "session_id": surface.session_id, "title": surface.title,
                    "cwd": surface.cwd, "remote": surface.remote, "kind": surface.kind, "url": surface.url, "browser_profile_id": surface.browser_profile_id, "file": surface.file, "selected": pane.selected_surface == surface.id,
                    "process_id": frame.and_then(|f| f.process_id), "starting": surface.session_id.is_some() && frame.is_none_or(|f| f.starting),
                    "state": frame.and_then(|f| f.snapshot.as_ref()).map(|s| state_name(s.state)),
                    "failure": frame.and_then(|f| f.failure.clone())})
            }).collect();
    Ok(
        json!({"revision": snapshot.revision, "window_id":window, "window_uuid":snapshot.model.object_uuid(TargetKind::Window, window.0), "workspace_id":workspace.id, "pane_id": pane_id, "surfaces": surfaces}),
    )
}

fn current_snapshot(desktop: &Desktop) -> Result<Arc<Snapshot>, (&'static str, String)> {
    let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
    loop {
        match desktop.snapshot() {
            Ok(snapshot) => return Ok(snapshot),
            Err(DesktopError::Busy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(DesktopError::Busy) => {
                return Err((
                    "desktop_busy",
                    "desktop snapshot remained busy until the read deadline".into(),
                ));
            }
            Err(error) => return Err(desktop_error(error)),
        }
    }
}

/// `open` (design D16). Admission already refused read-only sockets, so no file is touched for
/// them. A stale revision fails the whole request before any probe; the executor checks it again
/// when it resolves the target, which is the check that decides.
fn open_items(
    desktop: &Desktop,
    admission: &ControlAdmission,
    request_id: u64,
    request: TargetRequest,
) -> Result<Value, (&'static str, String)> {
    let TargetRequest {
        scope,
        caller,
        expected_model_revision,
        operation,
    } = request;
    let TargetOperation::OpenItems {
        items,
        focus,
        force_mode,
    } = operation
    else {
        return Err(("invalid_request", "not an open request".into()));
    };
    if let Some(expected) = expected_model_revision {
        let model = current_snapshot(desktop)?.model.revision;
        if expected != model {
            let error = zmux_workspace_core::targets::TargetError::StaleRevision {
                expected,
                actual: model,
            };
            return Err((error.code(), error.to_string()));
        }
    }
    let plans = desktop
        .probe_open_items(&items, force_mode)
        .map_err(|failure| ("invalid_request", failure.message))?;
    mutate(
        desktop,
        admission,
        request_id,
        Command::OpenFiles {
            batch: crate::files::OpenBatch {
                focus,
                scope,
                caller,
                expected_model_revision,
                items: plans,
            },
        },
    )
}

fn mutate(
    desktop: &Desktop,
    admission: &ControlAdmission,
    request_id: u64,
    payload: Command,
) -> Result<Value, (&'static str, String)> {
    let execution = desktop
        .execute_authorized(
            Request {
                version: CONTROL_VERSION,
                request_id,
                payload,
            },
            COMMAND_TIMEOUT,
            admission.clone(),
        )
        .map_err(desktop_error)?;
    execution.result.map_err(|error| match error {
        DesktopError::Policy(error) => (error.code(), error.to_string()),
        DesktopError::Target(error) => (error.code(), error.to_string()),
        DesktopError::Browser(error) => (error.code(), error.to_string()),
        DesktopError::Profile(error) => (error.code(), error.to_string()),
        DesktopError::Download(error) => (error.code, error.to_string()),
        DesktopError::Attention(error) => (error.code(), error.to_string()),
        DesktopError::Metadata(error) => (error.code(), error.to_string()),
        DesktopError::NotTerminal => ("not_terminal", error.to_string()),
        DesktopError::TerminalUnavailable => ("terminal_not_ready", error.to_string()),
        other => ("command_rejected", other.to_string()),
    })?;
    if let Some(receipt) = execution.agent_message_result {
        return Ok(serde_json::json!(receipt));
    }
    if let Some(order) = execution.surface_order_result {
        return Ok(order);
    }
    if let Some(group) = execution.workspace_group_result {
        return Ok(group);
    }
    if let Some(order) = execution.workspace_order_result {
        return Ok(order);
    }
    if let Some(window) = execution.window_result {
        return Ok(window);
    }
    if let Some(download) = execution.download_result {
        return Ok(download);
    }
    if let Some(metadata) = execution.metadata_result {
        return Ok(json!(metadata));
    }
    if let Some(navigation) = execution.notification_navigation {
        return Ok(json!(navigation));
    }
    if let Some(mutation) = execution.notification_mutation {
        return Ok(json!(mutation));
    }
    if let Some(operation_id) = execution.browser_operation_id {
        return execution
            .snapshot
            .browser_operations
            .iter()
            .find(|op| op.operation_id == operation_id)
            .map(|op| json!(op))
            .ok_or_else(|| {
                (
                    "browser_operation_not_found",
                    "browser admission receipt missing".into(),
                )
            });
    }
    if let Some(id) = execution.tmux_operation_id {
        return execution
            .snapshot
            .local_tmux_operations
            .iter()
            .find(|r| r.operation_id == id)
            .map(|r| json!(r))
            .ok_or((
                "local_tmux_operation_not_found",
                "admitted local tmux receipt missing".into(),
            ));
    }
    if let Some(operation_id) = execution.profile_operation_id {
        return Ok(
            json!({"operation_id":operation_id,"state":"pending","epoch":execution.snapshot.browser_profiles.epoch}),
        );
    }
    if let Some(operation_id) = execution.session_operation_id {
        return Ok(
            json!({"operation_id": operation_id, "state": "pending", "revision": execution.snapshot.revision}),
        );
    }
    let workspace = execution.snapshot.model.focused_workspace.and_then(|id| {
        execution
            .snapshot
            .model
            .workspaces
            .iter()
            .find(|item| item.id == id)
    });
    let selected_surface: Option<SurfaceId> = workspace
        .and_then(|w| w.root.panes().into_iter().find(|p| p.id == w.focused_pane))
        .map(|p| p.selected_surface);
    let model = &execution.snapshot.model;
    let mut created_panes = Vec::new();
    let mut created_surfaces = Vec::new();
    for workspace in &model.workspaces {
        for pane in workspace.root.panes() {
            if execution.created_panes.contains(&pane.id) {
                created_panes.push(object_receipt(
                    model,
                    workspace.id,
                    pane.id,
                    pane.selected_surface,
                ));
            }
            for surface in &pane.surfaces {
                if execution.created_surfaces.contains(&surface.id) {
                    created_surfaces.push(object_receipt(model, workspace.id, pane.id, surface.id));
                }
            }
        }
    }
    let open_result = execution.open_result.clone();
    let mut receipt = json!({
        "created_uploads": execution.created_uploads,
        "uploads": execution.snapshot.uploads,
        "created_workspaces": execution.created_workspaces.iter().filter_map(|id| execution.snapshot.model.workspaces.iter().find(|w| w.id == *id)).map(|w| {
            let pane = w.root.panes().into_iter().find(|pane| pane.id == w.focused_pane).expect("workspace focused pane exists");
            object_receipt(model, w.id, pane.id, pane.selected_surface)
        }).collect::<Vec<_>>(),
        "created_panes": created_panes,
        "created_surfaces": created_surfaces,
        "focused_window": model.focused_window,
        "focused_window_uuid": model.focused_window.map(|id| model.object_uuid(TargetKind::Window, id.0)),
        "focused_surface": selected_surface,
        "focused_surface_uuid": selected_surface.map(|id| execution.snapshot.model.object_uuid(TargetKind::Surface, id.0)),
        "focused_workspace_uuid": workspace.map(|w| execution.snapshot.model.object_uuid(TargetKind::Workspace, w.id.0)),
        "focused_pane_uuid": workspace.map(|w| execution.snapshot.model.object_uuid(TargetKind::Pane, w.focused_pane.0)),
        "model_revision": execution.snapshot.model.revision,
        "pending_launches": execution.snapshot.pending_launches,
        "revision": execution.snapshot.revision,
        "focused_workspace": workspace.map(|item| item.id),
        "focused_pane": workspace.map(|item| item.focused_pane),
    });
    if let Some(open) = open_result {
        receipt["ok"] = open["ok"].clone();
        receipt["items"] = open["items"].clone();
    }
    Ok(receipt)
}

fn object_receipt(
    model: &zmux_workspace_core::Snapshot,
    workspace: WorkspaceId,
    pane: PaneId,
    surface: SurfaceId,
) -> Value {
    let window = owning_window(model, workspace);
    json!({
        "window_id": window, "window_uuid":model.object_uuid(TargetKind::Window, window.0),
        "workspace_id": workspace, "workspace_uuid": model.object_uuid(TargetKind::Workspace, workspace.0),
        "pane_id": pane, "pane_uuid": model.object_uuid(TargetKind::Pane, pane.0),
        "surface_id": surface, "surface_uuid": model.object_uuid(TargetKind::Surface, surface.0)
    })
}

fn owning_window(
    model: &zmux_workspace_core::Snapshot,
    workspace: WorkspaceId,
) -> zmux_workspace_core::WindowId {
    model
        .windows
        .iter()
        .find(|window| window.workspace_ids.contains(&workspace))
        .expect("canonical workspace owner")
        .id
}

fn desktop_error(error: DesktopError) -> (&'static str, String) {
    let code = match error {
        DesktopError::Attention(ref error) => error.code(),
        DesktopError::Metadata(ref error) => error.code(),
        DesktopError::Profile(ref error) => error.code(),
        DesktopError::Policy(ref error) => error.code(),
        DesktopError::NotTerminal => "not_terminal",
        DesktopError::TerminalUnavailable => "terminal_not_ready",
        DesktopError::QueueFull => "overloaded",
        DesktopError::Closed => "desktop_closed",
        DesktopError::Busy => "desktop_busy",
        DesktopError::Timeout => "outcome_unknown",
        DesktopError::Target(ref error) => error.code(),
        DesktopError::Browser(ref error) => error.code(),
        DesktopError::Failure(_) => "desktop_error",
        DesktopError::Download(ref error) => error.code,
        DesktopError::File(_) => "file_error",
    };
    (code, error.to_string())
}

fn status(snapshot: &Snapshot) -> Value {
    let pane_count: usize = snapshot
        .model
        .workspaces
        .iter()
        .map(|workspace| workspace.root.panes().len())
        .sum();
    let running_sessions = snapshot
        .control_frames
        .iter()
        .filter(|frame| {
            frame
                .snapshot
                .as_ref()
                .is_some_and(|terminal| terminal.state == State::Running)
        })
        .count();
    json!({
        "revision": snapshot.revision,
        "window_count": snapshot.model.windows.len(),
        "focused_window": snapshot.model.focused_window,
        "workspace_count": snapshot.model.workspaces.len(),
        "pane_count": pane_count,
        "running_sessions": running_sessions,
        "pending_launches": snapshot.pending_launches,
        "closed": snapshot.closed,
        "protocol_version": CONTROL_VERSION,
        "model_version": zmux_workspace_core::MODEL_VERSION,
        "identity_namespace": snapshot.model.identity_namespace,
        "model_revision": snapshot.model.revision,
    })
}

fn list_panes(
    snapshot: &Snapshot,
    requested_workspace: Option<WorkspaceId>,
) -> Result<Value, (&'static str, String)> {
    if let Some(id) = requested_workspace
        && !snapshot
            .model
            .workspaces
            .iter()
            .any(|workspace| workspace.id == id)
    {
        return Err((
            "unknown_workspace",
            format!("workspace {} does not exist", id.0),
        ));
    }
    let panes: Vec<_> = snapshot
        .model
        .workspaces
        .iter()
        .filter(|workspace| requested_workspace.is_none_or(|id| workspace.id == id))
        .flat_map(|workspace| {
            workspace.root.panes().into_iter().map(|pane| {
                let frame = pane.session_id().and_then(|id| frame_for(snapshot, id));
                let window = owning_window(&snapshot.model, workspace.id);
                json!({
                    "window_id": window,
                    "window_uuid": snapshot.model.object_uuid(TargetKind::Window, window.0),
                    "workspace_id": workspace.id,
                    "workspace_name": workspace.name,
                    "pane_id": pane.id,
                    "pane_uuid": snapshot.model.object_uuid(TargetKind::Pane, pane.id.0),
                    "pane_ref": format!("pane:{}", pane.id.0),
                    "workspace_uuid": snapshot.model.object_uuid(TargetKind::Workspace, workspace.id.0),
                    "selected_surface": pane.selected_surface,
                    "surface_count": pane.surfaces.len(),
                    "session_id": pane.session_id(),
                    "focused": workspace.focused_pane == pane.id,
                    "process_id": frame.and_then(|item| item.process_id),
                    "starting": pane.session_id().is_some() && frame.is_none_or(|item| item.starting),
                    "state": frame.and_then(|item| item.snapshot.as_ref()).map(|item| state_name(item.state)),
                    "failure": frame.and_then(|item| item.failure.as_ref()),
                })
            })
        })
        .collect();
    Ok(json!({"revision": snapshot.revision, "panes": panes}))
}

fn read_screen(snapshot: &Snapshot, pane_id: PaneId) -> Result<Value, (&'static str, String)> {
    let pane = snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .find(|p| p.id == pane_id)
        .ok_or_else(|| ("unknown_pane", format!("pane {} does not exist", pane_id.0)))?;
    read_screen_surface(snapshot, pane.selected_surface)
}

fn read_screen_surface(
    snapshot: &Snapshot,
    surface_id: SurfaceId,
) -> Result<Value, (&'static str, String)> {
    let (workspace, pane, surface) = snapshot
        .model
        .workspaces
        .iter()
        .find_map(|w| {
            w.root.panes().into_iter().find_map(|p| {
                p.surfaces
                    .iter()
                    .find(|s| s.id == surface_id)
                    .map(|s| (w, p, s))
            })
        })
        .ok_or_else(|| ("target_not_found", "surface no longer exists".into()))?;
    let session_id = surface
        .session_id
        .ok_or_else(|| ("not_terminal", "target surface is a browser".into()))?;
    let frame = frame_for(snapshot, session_id)
        .ok_or_else(|| ("screen_unavailable", "terminal frame is unavailable".into()))?;
    let terminal = frame.snapshot.as_ref().ok_or_else(|| {
        let message = frame
            .failure
            .clone()
            .unwrap_or_else(|| "terminal is still starting".into());
        ("screen_unavailable", message)
    })?;
    Ok(json!({
        "revision": snapshot.revision,
        "window_id": owning_window(&snapshot.model, workspace.id),
        "window_uuid": snapshot.model.object_uuid(TargetKind::Window, owning_window(&snapshot.model, workspace.id).0),
        "workspace_id": workspace.id,
        "workspace_name": workspace.name,
        "pane_id": pane.id,
        "surface_id": surface.id,
        "surface_uuid": snapshot.model.object_uuid(TargetKind::Surface, surface.id.0),
        "session_id": session_id,
        "state": state_name(terminal.state),
        "rows": terminal.viewport.size.rows,
        "cols": terminal.viewport.size.cols,
        "cursor_row": terminal.viewport.cursor_row,
        "cursor_col": terminal.viewport.cursor_col,
        "contents": terminal.viewport.contents,
        "bytes_read": terminal.bytes_read,
        "exit_code": terminal.exit_code,
        "failure": terminal.failure,
    }))
}

fn frame_for(snapshot: &Snapshot, session_id: SessionId) -> Option<&crate::Frame> {
    snapshot
        .control_frames
        .iter()
        .find(|frame| frame.session_id == session_id)
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Running => "running",
        State::Exited => "exited",
        State::Closed => "closed",
        State::Failed => "failed",
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> nix::Result<Uid> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    getsockopt(stream, PeerCredentials).map(|credentials| Uid::from_raw(credentials.uid()))
}

#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> nix::Result<Uid> {
    nix::unistd::getpeereid(stream).map(|(uid, _)| uid)
}

#[cfg(test)]
mod tests {
    use crate as desktop_runtime;
    mod desktop_snapshot {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/desktop_snapshot.rs"
        ));
    }
    use super::*;
    use crate::Shell;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    fn test_socket(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        std::env::temp_dir()
            .join(format!(
                "zc{:x}{nonce:x}{:x}",
                std::process::id(),
                NEXT_PATH.fetch_add(1, Ordering::Relaxed)
            ))
            .join(&label[..1])
    }

    fn desktop() -> Arc<Desktop> {
        Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: None,
            })
            .unwrap(),
        )
    }

    fn exchange_raw(path: &Path, length: u32, body: &[u8]) -> ControlResponse {
        let mut stream = UnixStream::connect(path).unwrap();
        stream.write_all(&length.to_be_bytes()).unwrap();
        stream.write_all(body).unwrap();
        let mut response_length = [0; 4];
        stream.read_exact(&mut response_length).unwrap();
        let mut response = vec![0; u32::from_be_bytes(response_length) as usize];
        stream.read_exact(&mut response).unwrap();
        serde_json::from_slice(&response).unwrap()
    }

    fn cleanup(path: &Path, server: ControlServer, desktop: Arc<Desktop>) {
        server.shutdown(Duration::from_secs(2)).unwrap();
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        if let Some(parent) = path.parent() {
            fs::remove_dir(parent).unwrap();
        }
    }

    #[test]
    fn oversized_frames_are_rejected_before_allocation() {
        let path = test_socket("oversized");
        let desktop = desktop();
        let server = ControlServer::start(&path, Arc::clone(&desktop)).unwrap();
        let response = exchange_raw(&path, (MAX_CONTROL_REQUEST_BYTES + 1) as u32, &[]);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "request_too_large");
        cleanup(&path, server, desktop);
    }

    #[test]
    fn legacy_v1_mutations_work_but_cannot_bypass_new_password_policy() {
        use crate::control_policy::{ControlAccess, SocketPassword};
        let path = test_socket("legacy");
        let desktop = desktop();
        let server = ControlServer::start(&path, Arc::clone(&desktop)).unwrap();
        let body = br#"{"version":1,"request_id":19,"command":{"method":"new_workspace","name":"Legacy client"}}"#;
        let response = exchange_raw(&path, body.len() as u32, body);
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.version, 1);
        let before = desktop_snapshot::read(&desktop).model.clone();
        assert!(before.workspaces.iter().any(|w| w.name == "Legacy client"));
        server
            .set_policy(ControlPolicy::password(
                ControlAccess::Full,
                SocketPassword::new("synthetic-test-only".into()).unwrap(),
            ))
            .unwrap();
        let denied = exchange_raw(&path, body.len() as u32, body);
        assert!(!denied.ok);
        assert_eq!(denied.version, 1);
        assert_eq!(denied.error.unwrap().code, "unauthorized");
        assert_eq!(desktop_snapshot::read(&desktop).model, before);
        cleanup(&path, server, desktop);
    }

    #[test]
    fn accepted_nonblocking_stream_waits_for_a_delayed_request_frame() {
        let desktop = desktop();
        let (mut client, server) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(25)))
            .unwrap();
        let server_desktop = Arc::clone(&desktop);
        let handler = thread::spawn(move || {
            serve_connection(
                server,
                &server_desktop,
                geteuid(),
                &ControlGate::new(ControlPolicy::default()),
            )
        });
        let mut length = [0; 4];
        let pending = client.read_exact(&mut length);
        assert!(
            pending.is_err(),
            "server rejected a connection before any frame was sent"
        );
        assert!(matches!(
            pending.unwrap_err().kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = br#"{"version":1,"request_id":12,"command":{"method":"status"}}"#;
        client
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        client.write_all(request).unwrap();
        client.read_exact(&mut length).unwrap();
        let mut body = vec![0; u32::from_be_bytes(length) as usize];
        client.read_exact(&mut body).unwrap();
        let response: ControlResponse = serde_json::from_slice(&body).unwrap();
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.request_id, 12);
        assert_eq!(response.version, 1, "legacy v1 requests need a v1 response");
        handler.join().unwrap();
        desktop.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn peer_with_unexpected_uid_is_rejected_before_request_dispatch() {
        let path = test_socket("unauthorized");
        let desktop = desktop();
        let unexpected_uid = Uid::from_raw(geteuid().as_raw().wrapping_add(1));
        prepare_private_directory(path.parent().unwrap(), geteuid()).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server_stream, _) = listener.accept().unwrap();
        let desktop_for_server = Arc::clone(&desktop);
        let handler = thread::spawn(move || {
            serve_connection(
                server_stream,
                &desktop_for_server,
                unexpected_uid,
                &ControlGate::new(ControlPolicy::default()),
            )
        });
        let mut client = client;
        // Peer credentials are rejected before reading any request. Sending a body here
        // races the intentional close and can produce BrokenPipe on macOS.
        let mut response_length = [0; 4];
        client.read_exact(&mut response_length).unwrap();
        let mut response = vec![0; u32::from_be_bytes(response_length) as usize];
        client.read_exact(&mut response).unwrap();
        let response: ControlResponse = serde_json::from_slice(&response).unwrap();
        assert_eq!(response.error.unwrap().code, "unauthorized");
        handler.join().unwrap();
        fs::remove_file(&path).unwrap();
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn message_io_and_queued_policy_writer_do_not_block_the_workspace_executor() {
        use crate::{
            agent_messages::{MessageAction, MessageRequest},
            control_policy::ControlAccess,
        };
        use std::sync::mpsc;
        let socket = test_socket("policy-io");
        let root = socket.parent().unwrap();
        fs::create_dir_all(root).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let state = root.join("state");
        let desktop = Arc::new(
            Desktop::with_persistence(
                Shell {
                    program: "/bin/sh".into(),
                    args: vec!["-i".into()],
                    cwd: None,
                },
                state.clone(),
            )
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let initial = loop {
            let snapshot = desktop_snapshot::read_until(&desktop, deadline);
            if !snapshot.persistence.startup_pending && !snapshot.model.workspaces.is_empty() {
                break snapshot;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        let surface = initial.model.workspaces[0].root.panes()[0].selected_surface;
        let target = initial
            .model
            .object_uuid(TargetKind::Surface, surface.0)
            .to_string();
        let state_fd = crate::persistence::state_directory(&state).unwrap();
        let journal_lock =
            crate::persistence::lock_named(&state_fd, crate::agent_messages::LOCK_NAME).unwrap();
        let message_id = uuid::Uuid::new_v4();
        let command = ControlCommand::AgentMessages {
            request: MessageRequest {
                request_id: message_id,
                caller: Default::default(),
                action: MessageAction::Send {
                    target: Some(target),
                    reply_to: None,
                    body: "blocked only on the journal worker".into(),
                    sender_name: None,
                    thread_id: None,
                },
            },
        };
        let body = serde_json::to_vec(&ControlRequest {
            version: CONTROL_VERSION,
            request_id: 500,
            password: None,
            command,
        })
        .unwrap();
        let response = exchange_raw(&socket, body.len() as u32, &body);
        assert!(response.ok);
        assert_eq!(response.result.unwrap()["state"], "pending");
        // Linux descriptor observation proves the real worker opened its state directory
        // after acquiring socket authorization and while our journal lock prevents commit.
        // Unlike its transient lock-open attempts, this descriptor spans the whole wait.
        let expected = state.clone();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let owners = fs::read_dir("/proc/self/fd")
                .unwrap()
                .filter_map(Result::ok)
                .filter_map(|e| fs::read_link(e.path()).ok())
                .filter(|p| p == &expected)
                .count();
            if owners >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual journal worker did not open state while waiting on its held file lock"
            );
            thread::sleep(Duration::from_millis(1));
        }
        let action = Action::CreateWorkspace {
            name: "authorized must fail fast".into(),
        };
        let action_admission = server
            .gate
            .admit(
                ControlOrigin::LocalOwner,
                &ControlCommand::Action {
                    action: action.clone(),
                },
                None,
            )
            .unwrap();
        let status_admission = server
            .gate
            .admit(
                ControlOrigin::LocalOwner,
                &ControlCommand::AgentMessagesStatus {
                    request_id: message_id,
                },
                None,
            )
            .unwrap();
        let gate = server.gate.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = gate.replace(ControlPolicy::same_user(ControlAccess::ReadOnly));
            done_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        // The writer cannot finish while the real journal transaction owns its guard.
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        let action_result = desktop.execute_authorized(
            crate::Request {
                version: CONTROL_VERSION,
                request_id: 501,
                payload: Command::Action { action },
            },
            Duration::from_millis(250),
            action_admission,
        );
        let status_result = desktop.execute_authorized(
            crate::Request {
                version: CONTROL_VERSION,
                request_id: 502,
                payload: Command::AgentMessagesStatus {
                    request_id: message_id,
                },
            },
            Duration::from_millis(250),
            status_admission,
        );
        desktop
            .submit(crate::Request {
                version: CONTROL_VERSION,
                request_id: 503,
                payload: Command::Action {
                    action: Action::CreateWorkspace {
                        name: "trusted executor remains live".into(),
                    },
                },
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(250);
        let trusted_live = loop {
            let s = desktop_snapshot::read_until(&desktop, deadline);
            if s.model
                .workspaces
                .iter()
                .any(|w| w.name == "trusted executor remains live")
            {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(1));
        };
        drop(journal_lock);
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        writer.join().unwrap();
        server.shutdown(Duration::from_secs(2)).unwrap();
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            matches!(
                action_result,
                Ok(crate::Execution {
                    result: Err(DesktopError::Policy(
                        crate::control_policy::PolicyError::Unavailable
                    )),
                    ..
                })
            ),
            "executor authorization waited behind a policy writer"
        );
        assert!(
            matches!(
                status_result,
                Ok(crate::Execution {
                    result: Err(DesktopError::Policy(
                        crate::control_policy::PolicyError::Unavailable
                    )),
                    ..
                })
            ),
            "message status authorization waited behind a policy writer"
        );
        assert!(
            trusted_live,
            "journal I/O stalled unrelated trusted executor work"
        );
    }
}
