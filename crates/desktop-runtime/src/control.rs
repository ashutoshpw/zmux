//! Versioned, same-user local control for a running desktop.

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
use zmux_workspace_core::{Action, Axis, PaneId, SessionId, WorkspaceId};

pub const CONTROL_VERSION: u32 = 1;
pub const MAX_CONTROL_REQUEST_BYTES: usize = 16_384;
pub const MAX_CONTROL_RESPONSE_BYTES: usize = 1_048_576;
const CLIENT_IO_TIMEOUT: Duration = Duration::from_millis(500);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const ACCEPT_POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub version: u32,
    pub request_id: u64,
    pub command: ControlCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlCommand {
    Status,
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
}

impl ControlServer {
    pub fn start(path: &Path, desktop: Arc<Desktop>) -> Result<Self, ControlServerError> {
        Self::start_for_uid(path, desktop, geteuid())
    }

    fn start_for_uid(
        path: &Path,
        desktop: Arc<Desktop>,
        allowed_uid: Uid,
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
        let worker = thread::Builder::new()
            .name("zmux-local-control".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => serve_connection(stream, &desktop, allowed_uid),
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
        })
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

fn serve_connection(mut stream: UnixStream, desktop: &Desktop, allowed_uid: Uid) {
    let _ = stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT));
    let response = match peer_uid(&stream) {
        Ok(uid) if uid == allowed_uid => match read_request(&mut stream) {
            Ok(request) => handle_request(desktop, request),
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

fn handle_request(desktop: &Desktop, request: ControlRequest) -> ControlResponse {
    if request.version != CONTROL_VERSION {
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
    match dispatch(desktop, request) {
        Ok(result) => ControlResponse::success(request_id, result),
        Err((code, message)) => ControlResponse::failure(request_id, code, message),
    }
}

fn dispatch(desktop: &Desktop, request: ControlRequest) -> Result<Value, (&'static str, String)> {
    match request.command {
        ControlCommand::Status => {
            let snapshot = current_snapshot(desktop)?;
            Ok(status(&snapshot))
        }
        ControlCommand::ListWorkspaces => {
            let snapshot = current_snapshot(desktop)?;
            Ok(json!({
                "revision": snapshot.revision,
                "focused_workspace": snapshot.model.focused_workspace,
                "workspaces": snapshot.model.workspaces,
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
            request.request_id,
            Command::Action {
                action: Action::CreateWorkspace { name },
            },
        ),
        ControlCommand::RenameWorkspace { workspace_id, name } => mutate(
            desktop,
            request.request_id,
            Command::Action {
                action: Action::RenameWorkspace { workspace_id, name },
            },
        ),
        ControlCommand::FocusPane { pane_id } => mutate(
            desktop,
            request.request_id,
            Command::Action {
                action: Action::FocusPane { pane_id },
            },
        ),
        ControlCommand::SplitPane { pane_id, axis } => mutate(
            desktop,
            request.request_id,
            Command::Action {
                action: Action::SplitPane { pane_id, axis },
            },
        ),
        ControlCommand::ClosePane { pane_id } => mutate(
            desktop,
            request.request_id,
            Command::Action {
                action: Action::ClosePane { pane_id },
            },
        ),
        ControlCommand::SendText { pane_id, text } => {
            let snapshot = current_snapshot(desktop)?;
            let session_id = pane_session(&snapshot, pane_id)?;
            mutate(
                desktop,
                request.request_id,
                Command::Input { session_id, text },
            )
        }
    }
}

fn current_snapshot(desktop: &Desktop) -> Result<Arc<Snapshot>, (&'static str, String)> {
    desktop.snapshot().map_err(desktop_error)
}

fn mutate(
    desktop: &Desktop,
    request_id: u64,
    payload: Command,
) -> Result<Value, (&'static str, String)> {
    let execution = desktop
        .execute(
            Request {
                version: CONTROL_VERSION,
                request_id,
                payload,
            },
            COMMAND_TIMEOUT,
        )
        .map_err(desktop_error)?;
    execution
        .result
        .map_err(|message| ("command_rejected", message))?;
    let workspace = execution.snapshot.model.focused_workspace.and_then(|id| {
        execution
            .snapshot
            .model
            .workspaces
            .iter()
            .find(|item| item.id == id)
    });
    Ok(json!({
        "revision": execution.snapshot.revision,
        "focused_workspace": workspace.map(|item| item.id),
        "focused_pane": workspace.map(|item| item.focused_pane),
    }))
}

fn desktop_error(error: DesktopError) -> (&'static str, String) {
    let code = match error {
        DesktopError::QueueFull => "overloaded",
        DesktopError::Closed => "desktop_closed",
        DesktopError::Busy => "desktop_busy",
        DesktopError::Timeout => "outcome_unknown",
        DesktopError::Failure(_) => "desktop_error",
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
        "workspace_count": snapshot.model.workspaces.len(),
        "pane_count": pane_count,
        "running_sessions": running_sessions,
        "pending_launches": snapshot.pending_launches,
        "closed": snapshot.closed,
        "protocol_version": CONTROL_VERSION,
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
                let frame = frame_for(snapshot, pane.session_id);
                json!({
                    "workspace_id": workspace.id,
                    "workspace_name": workspace.name,
                    "pane_id": pane.id,
                    "session_id": pane.session_id,
                    "focused": workspace.focused_pane == pane.id,
                    "process_id": frame.and_then(|item| item.process_id),
                    "starting": frame.is_none_or(|item| item.starting),
                    "state": frame.and_then(|item| item.snapshot.as_ref()).map(|item| state_name(item.state)),
                    "failure": frame.and_then(|item| item.failure.as_ref()),
                })
            })
        })
        .collect();
    Ok(json!({"revision": snapshot.revision, "panes": panes}))
}

fn read_screen(snapshot: &Snapshot, pane_id: PaneId) -> Result<Value, (&'static str, String)> {
    let (workspace, session_id) = snapshot
        .model
        .workspaces
        .iter()
        .find_map(|workspace| {
            workspace
                .root
                .panes()
                .into_iter()
                .find(|pane| pane.id == pane_id)
                .map(|pane| (workspace, pane.session_id))
        })
        .ok_or_else(|| ("unknown_pane", format!("pane {} does not exist", pane_id.0)))?;
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
        "workspace_id": workspace.id,
        "workspace_name": workspace.name,
        "pane_id": pane_id,
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

fn pane_session(snapshot: &Snapshot, pane_id: PaneId) -> Result<SessionId, (&'static str, String)> {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .find(|pane| pane.id == pane_id)
        .map(|pane| pane.session_id)
        .ok_or_else(|| ("unknown_pane", format!("pane {} does not exist", pane_id.0)))
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
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "zmux-control-{}-{nonce}-{}-{label}",
                std::process::id(),
                NEXT_PATH.fetch_add(1, Ordering::Relaxed)
            ))
            .join("control.sock")
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
            serve_connection(server_stream, &desktop_for_server, unexpected_uid)
        });
        let mut client = client;
        let request = serde_json::to_vec(&ControlRequest {
            version: CONTROL_VERSION,
            request_id: 9,
            command: ControlCommand::Status,
        })
        .unwrap();
        client
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        client.write_all(&request).unwrap();
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
}
