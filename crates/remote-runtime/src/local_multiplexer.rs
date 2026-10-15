//! Explicit user-owned local tmux sessions. Filesystem/process waits belong on a
//! CLI/background worker, never the desktop action executor. GUI binding is separate.
use crate::ProcessSpec;
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::{Pid, geteuid},
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_SESSIONS: usize = 64;
pub const MAX_REGISTRY_BYTES: usize = 128 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const MARKER: &str = "@zmux_local_tmux_server";
const REJECT: &str = "__zmux_stale_local_tmux_binding__";

#[derive(Debug, Error)]
pub enum MultiplexerError {
    #[error("{0}")]
    Invalid(String),
    #[error("unsafe local tmux profile: {0}")]
    Unsafe(String),
    #[error("local tmux operation timed out; inspect status before retrying mutations")]
    Timeout,
    #[error("local tmux profile is busy")]
    Busy,
    #[error("local tmux session was not found")]
    NotFound,
    #[error("local tmux binding is stale; cleanup previews stale records before pruning")]
    Stale,
    #[error(
        "multiple tmux clients are attached; use --all explicitly (individual --client selection is unavailable)"
    )]
    AmbiguousClients,
    #[error("{0}")]
    Failure(String),
}
type Result<T> = std::result::Result<T, MultiplexerError>;
fn failure(context: &str, error: impl std::fmt::Display) -> MultiplexerError {
    MultiplexerError::Failure(format!("{context}: {error}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SessionSelector {
    Name(String),
    Id(Uuid),
}
impl SessionSelector {
    pub fn name(value: &str) -> Result<Self> {
        validate_name(value)?;
        Ok(Self::Name(value.into()))
    }
}
/// Durable identity only: no mutable name, command, executable or socket path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalTmuxIdentity {
    pub id: Uuid,
    pub server_incarnation: Uuid,
    pub tmux_session_id: String,
    pub created_at: u64,
}
impl LocalTmuxIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil()
            || self.server_incarnation.is_nil()
            || !valid_session_id(&self.tmux_session_id)
            || self.created_at == 0
        {
            return Err(MultiplexerError::Invalid(
                "invalid immutable local tmux identity".into(),
            ));
        }
        Ok(())
    }
}
/// A routing hint checked against the authenticated desktop's live model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuiBinding {
    pub identity_namespace: Uuid,
    pub workspace: Uuid,
    pub surface: Uuid,
}
impl GuiBinding {
    fn validate(&self) -> Result<()> {
        if self.identity_namespace.is_nil() || self.workspace.is_nil() || self.surface.is_nil() {
            return Err(MultiplexerError::Invalid(
                "invalid local tmux GUI binding".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: Uuid,
    name: String,
    server_incarnation: Uuid,
    tmux_session_id: String,
    created_at: u64,
    cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gui_binding: Option<GuiBinding>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    sessions: Vec<Record>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ClientInfo {
    pub id: String,
    pub process_id: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct SessionStatus {
    pub id: Uuid,
    pub name: String,
    pub state: &'static str,
    pub server_incarnation: Uuid,
    pub tmux_session_id: String,
    pub created_at: u64,
    pub cwd: String,
    pub clients: Vec<ClientInfo>,
    pub gui_binding: Option<GuiBinding>,
}
impl SessionStatus {
    pub fn identity(&self) -> LocalTmuxIdentity {
        LocalTmuxIdentity {
            id: self.id,
            server_incarnation: self.server_incarnation,
            tmux_session_id: self.tmux_session_id.clone(),
            created_at: self.created_at,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct UnmanagedSession {
    pub name: String,
    pub tmux_session_id: String,
    pub created_at: u64,
}
#[derive(Debug, Serialize)]
pub struct SessionList {
    pub socket_path: PathBuf,
    pub server_running: bool,
    pub sessions: Vec<SessionStatus>,
    pub unmanaged: Vec<UnmanagedSession>,
}
#[derive(Debug, Serialize)]
pub struct CleanupResult {
    pub stale_ids: Vec<Uuid>,
    pub pruned: bool,
}
#[derive(Debug, Serialize)]
pub struct DetachResult {
    pub id: Uuid,
    pub detached_clients: usize,
}
struct LiveSession {
    id: String,
    created: u64,
    name: String,
    clients: Vec<ClientInfo>,
}
struct Server {
    marker: Option<Uuid>,
    sessions: Vec<LiveSession>,
}

/// A profile owns one private namespace; inspection and cleanup never use tmux's
/// default server. Explicit configuration is also suitable for background/native owners.
#[derive(Clone)]
pub struct LocalTmux {
    root: PathBuf,
    binary: PathBuf,
    cancellation: Option<Arc<AtomicBool>>,
}
impl LocalTmux {
    pub fn from_environment() -> Result<Self> {
        if let Some(root) = env::var_os("ZMUX_LOCAL_TMUX_ROOT") {
            return Self::new(PathBuf::from(root), selected_binary()?);
        }
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| {
                MultiplexerError::Invalid("HOME must be an absolute user directory".into())
            })?;
        let home = home
            .canonicalize()
            .map_err(|e| failure("resolve user directory", e))?;
        validate_directory(&home, false)?;
        let config = home.join(".config");
        ensure_directory(&config, false)?;
        let product = config.join("zmux");
        ensure_directory(&product, false)?;
        let binary = selected_binary()?;
        Self::new(product.join("local-tmux"), binary)
    }
    /// The durable owner exports only its owned profile path for later caller routing.
    /// No app surface identity or credential is inherited by its shell.
    pub fn for_caller_environment() -> Result<Self> {
        if let Some(root) = env::var_os("ZMUX_LOCAL_TMUX_PROFILE") {
            Self::new(PathBuf::from(root), selected_binary()?)
        } else {
            Self::from_environment()
        }
    }
    pub fn new(root: PathBuf, binary: PathBuf) -> Result<Self> {
        if !root.is_absolute()
            || !binary.is_absolute()
            || root
                .to_str()
                .is_none_or(|s| s.chars().any(char::is_control))
            || binary
                .to_str()
                .is_none_or(|s| s.chars().any(char::is_control))
        {
            return Err(MultiplexerError::Invalid(
                "tmux profile and executable must be absolute paths".into(),
            ));
        }
        let metadata = fs::metadata(&binary).map_err(|e| failure("inspect tmux executable", e))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(MultiplexerError::Invalid(
                "tmux executable must be an executable regular file".into(),
            ));
        }
        let parent = root.parent().ok_or_else(|| {
            MultiplexerError::Invalid("tmux profile needs a parent directory".into())
        })?;
        validate_directory(parent, false)?;
        if root.join("server.sock").as_os_str().len() >= 100 {
            return Err(MultiplexerError::Invalid(
                "tmux socket path exceeds the portable 99-byte bound".into(),
            ));
        }
        Ok(Self {
            root,
            binary,
            cancellation: None,
        })
    }
    /// Background owners may cancel a queued/active invocation without killing its durable server.
    pub fn with_cancellation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
    fn check_cancelled(&self) -> Result<()> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Acquire))
        {
            return Err(MultiplexerError::Failure(
                "local tmux operation cancelled; inspect list if owner creation was pending".into(),
            ));
        }
        Ok(())
    }
    pub fn socket_path(&self) -> PathBuf {
        self.root.join("server.sock")
    }
    pub fn list(&self) -> Result<SessionList> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        Ok(self.catalog(&registry, server.as_ref()))
    }
    pub fn status(&self, selector: &SessionSelector) -> Result<SessionStatus> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        Ok(status(find_record(&registry, selector)?, server.as_ref()))
    }
    /// Resolve a saved identity without allowing a registry edit or name reuse to retarget it.
    pub fn status_identity(&self, identity: &LocalTmuxIdentity) -> Result<SessionStatus> {
        identity.validate()?;
        let status = self.status(&SessionSelector::Id(identity.id))?;
        if status.identity() != *identity {
            return Err(MultiplexerError::Stale);
        }
        Ok(status)
    }
    pub fn remember_gui(&self, identity: &LocalTmuxIdentity, binding: GuiBinding) -> Result<()> {
        identity.validate()?;
        binding.validate()?;
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        let record = find_record(&registry, &SessionSelector::Id(identity.id))?;
        if status(record, server.as_ref()).identity() != *identity
            || live_record(record, server.as_ref()).is_none()
        {
            return Err(MultiplexerError::Stale);
        }
        registry
            .sessions
            .iter_mut()
            .find(|r| r.id == identity.id)
            .expect("validated record")
            .gui_binding = Some(binding);
        self.save(&registry)
    }
    /// Resolve the caller's actual pane, accepting only this owned profile socket.
    /// Shell environments survive client surfaces, so saved app surface IDs are not authority.
    pub fn pane_identity(&self, socket: &Path, pane: &str) -> Result<LocalTmuxIdentity> {
        if socket != self.socket_path()
            || !pane.strip_prefix('%').is_some_and(|n| {
                !n.is_empty() && n.len() <= 20 && n.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(MultiplexerError::Invalid(
                "caller is not a pane on the owned local tmux profile".into(),
            ));
        }
        let (_lock, registry) = self.lock_registry()?;
        let server = self.query()?.ok_or(MultiplexerError::Stale)?;
        let marker = server.marker.ok_or(MultiplexerError::Stale)?;
        let sid = self.run(&[
            "-N".into(),
            "if-shell".into(),
            "-F".into(),
            "-t".into(),
            pane.into(),
            format!("#{{==:#{{{MARKER}}},{marker}}}"),
            tmux_command(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{session_id}".into(),
            ]),
            REJECT.into(),
        ])?;
        let record = registry
            .sessions
            .iter()
            .find(|r| r.server_incarnation == marker && r.tmux_session_id == sid.trim())
            .ok_or(MultiplexerError::NotFound)?;
        if live_record(record, Some(&server)).is_none() {
            return Err(MultiplexerError::Stale);
        }
        Ok(status(record, Some(&server)).identity())
    }
    pub fn start(&self, name: &str, cwd: &Path, command: Option<&str>) -> Result<SessionStatus> {
        validate_name(name)?;
        if !cwd.is_absolute()
            || cwd
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(MultiplexerError::Invalid("tmux cwd must be an absolute UTF-8 path without control characters, at most 4096 bytes".into()));
        }
        if command.is_some_and(|c| c.trim().is_empty() || c.len() > 8192 || c.contains('\0')) {
            return Err(MultiplexerError::Invalid(
                "tmux command must be nonempty, contain no NUL and fit 8192 bytes".into(),
            ));
        }
        let cwd = cwd
            .canonicalize()
            .map_err(|e| failure("resolve tmux cwd", e))?;
        if !cwd.is_dir()
            || cwd
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err(MultiplexerError::Invalid(
                "resolved tmux cwd must be a UTF-8 directory without controls and fit 4096 bytes"
                    .into(),
            ));
        }
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        if registry.sessions.len() >= MAX_SESSIONS {
            return Err(MultiplexerError::Invalid(
                "local tmux registry has 64 records; cleanup stale records or close a session"
                    .into(),
            ));
        }
        if registry.sessions.iter().any(|r| r.name == name)
            || server
                .as_ref()
                .is_some_and(|s| s.sessions.iter().any(|r| r.name == name))
        {
            return Err(MultiplexerError::Invalid(
                "tmux session name already exists; inspect status or cleanup stale records".into(),
            ));
        }
        let id = Uuid::new_v4();
        let marker = match &server {
            Some(s) => s.marker.ok_or_else(|| {
                MultiplexerError::Unsafe(
                    "existing profile server has no valid incarnation marker".into(),
                )
            })?,
            None => Uuid::new_v4(),
        };
        let mut reserved = registry.clone();
        reserved.sessions.push(Record {
            id,
            name: name.into(),
            server_incarnation: marker,
            tmux_session_id: "$18446744073709551615".into(),
            created_at: u64::MAX,
            cwd: cwd.to_string_lossy().into_owned(),
            gui_binding: None,
        });
        if serde_json::to_vec(&reserved)
            .map_err(|e| failure("reserve registry capacity", e))?
            .len()
            > MAX_REGISTRY_BYTES
        {
            return Err(MultiplexerError::Invalid("local tmux registry byte capacity reached; cleanup stale records or close a session".into()));
        }
        let mut create = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            name.into(),
            "-c".into(),
            cwd.to_str().expect("validated cwd").replace('#', "##"),
        ];
        if let Some(command) = command {
            create.push(command.into());
        }
        if server.is_some() {
            self.run(&[
                "-N".into(),
                "if-shell".into(),
                "-F".into(),
                format!("#{{==:#{{{MARKER}}},{marker}}}"),
                tmux_command(&create),
                REJECT.into(),
            ])?;
        } else {
            let mut args = vec![
                "start-server".into(),
                ";".into(),
                "set-option".into(),
                "-g".into(),
                "history-limit".into(),
                "10000".into(),
                ";".into(),
                "set-option".into(),
                "-g".into(),
                MARKER.into(),
                marker.to_string(),
                ";".into(),
            ];
            args.extend(create);
            self.run(&args)?;
            // Only a server created by this locked operation receives private mode.
            let path = self.socket_path();
            validate_socket(&path, false)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|e| failure("secure tmux socket", e))?;
        }
        let live = self.query()?.ok_or_else(|| {
            MultiplexerError::Failure(
                "new tmux session exited before registration; no live session was recorded".into(),
            )
        })?;
        if live.marker != Some(marker) {
            return Err(MultiplexerError::Stale);
        }
        let session = live
            .sessions
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                MultiplexerError::Failure(
                    "new tmux session exited before registration; no live session was recorded"
                        .into(),
                )
            })?;
        let record = Record {
            id,
            name: name.into(),
            server_incarnation: marker,
            tmux_session_id: session.id.clone(),
            created_at: session.created,
            cwd: cwd.to_string_lossy().into_owned(),
            gui_binding: None,
        };
        registry.sessions.push(record.clone());
        // If a durable write fails, the started session remains discoverable as
        // unmanaged; never kill it blindly or claim that start had no side effect.
        self.save(&registry).map_err(|e|failure("tmux session started but registry save failed; inspect list for unmanaged sessions",e))?;
        Ok(status(&record, Some(&live)))
    }
    pub fn detach(&self, selector: &SessionSelector, all: bool) -> Result<DetachResult> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        let record = find_record(&registry, selector)?;
        let live = live_record(record, server.as_ref()).ok_or(MultiplexerError::Stale)?;
        let count = live.clients.len();
        if !all && count > 1 {
            return Err(MultiplexerError::AmbiguousClients);
        }
        // Recheck the attached count in the same server request, so a new client
        // cannot turn an implicit single detach into a silent detach-all.
        let condition = if all {
            binding_condition(record)
        } else {
            format!(
                "#{{&&:{},#{{==:#{{session_attached}},{count}}}}}",
                binding_condition(record)
            )
        };
        let action = format!(
            "{} ; {}",
            tmux_command(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                record.tmux_session_id.clone(),
                "#{session_attached}".into()
            ]),
            tmux_command(&[
                "detach-client".into(),
                "-s".into(),
                record.tmux_session_id.clone()
            ])
        );
        let actual = self.guarded_output(record, &condition, action)?;
        let detached_clients = actual.trim().parse().map_err(|e| {
            failure(
                "detach completed but count receipt was invalid; inspect status",
                e,
            )
        })?;
        Ok(DetachResult {
            id: record.id,
            detached_clients,
        })
    }
    pub fn close(&self, selector: &SessionSelector) -> Result<Uuid> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        let record = find_record(&registry, selector)?.clone();
        if live_record(&record, server.as_ref()).is_none() {
            return Err(MultiplexerError::Stale);
        }
        self.guarded(
            &record,
            &binding_condition(&record),
            vec![
                "kill-session".into(),
                "-t".into(),
                record.tmux_session_id.clone(),
            ],
        )?;
        registry.sessions.retain(|r| r.id != record.id);
        self.save(&registry)?;
        Ok(record.id)
    }
    pub fn cleanup(&self, prune: bool) -> Result<CleanupResult> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        let names_changed = refresh_names(&mut registry, server.as_ref());
        let stale_ids: Vec<_> = registry
            .sessions
            .iter()
            .filter(|r| live_record(r, server.as_ref()).is_none())
            .map(|r| r.id)
            .collect();
        if prune {
            registry.sessions.retain(|r| !stale_ids.contains(&r.id));
        }
        if names_changed || (prune && !stale_ids.is_empty()) {
            self.save(&registry)?;
        }
        Ok(CleanupResult {
            stale_ids,
            pruned: prune,
        })
    }
    /// Execute this plan directly on the user's terminal. Its identity condition
    /// runs inside the attach request, after any preflight/exec scheduling delay.
    pub fn attach_spec(&self, selector: &SessionSelector) -> Result<ProcessSpec> {
        let (_lock, mut registry) = self.lock_registry()?;
        let server = self.query()?;
        if refresh_names(&mut registry, server.as_ref()) {
            self.save(&registry)?;
        }
        let record = find_record(&registry, selector)?;
        if live_record(record, server.as_ref()).is_none() {
            return Err(MultiplexerError::Stale);
        }
        Ok(self.record_attach_spec(record))
    }
    pub fn attach_identity(&self, identity: &LocalTmuxIdentity) -> Result<ProcessSpec> {
        identity.validate()?;
        let (_lock, registry) = self.lock_registry()?;
        let server = self.query()?;
        let record = find_record(&registry, &SessionSelector::Id(identity.id))?;
        if status(record, server.as_ref()).identity() != *identity
            || live_record(record, server.as_ref()).is_none()
        {
            return Err(MultiplexerError::Stale);
        }
        Ok(self.record_attach_spec(record))
    }
    fn record_attach_spec(&self, record: &Record) -> ProcessSpec {
        let mut args = self.base_args();
        args.extend([
            "-N".into(),
            "if-shell".into(),
            "-F".into(),
            "-t".into(),
            record.tmux_session_id.clone(),
            binding_condition(record),
            tmux_command(&[
                "attach-session".into(),
                "-t".into(),
                record.tmux_session_id.clone(),
            ]),
            REJECT.into(),
        ]);
        ProcessSpec {
            program: self.binary.to_string_lossy().into_owned(),
            args,
        }
    }
    fn guarded(&self, record: &Record, condition: &str, command: Vec<String>) -> Result<()> {
        self.guarded_output(record, condition, tmux_command(&command))
            .map(|_| ())
    }
    fn guarded_output(&self, record: &Record, condition: &str, command: String) -> Result<String> {
        self.run(&[
            "-N".into(),
            "if-shell".into(),
            "-F".into(),
            "-t".into(),
            record.tmux_session_id.clone(),
            condition.into(),
            command,
            REJECT.into(),
        ])
    }
    fn catalog(&self, registry: &Registry, server: Option<&Server>) -> SessionList {
        let sessions = registry
            .sessions
            .iter()
            .map(|r| status(r, server))
            .collect();
        let unmanaged = server
            .into_iter()
            .flat_map(|s| &s.sessions)
            .filter(|s| {
                !registry
                    .sessions
                    .iter()
                    .any(|r| live_record(r, server).is_some_and(|live| live.id == s.id))
            })
            .map(|s| UnmanagedSession {
                name: s.name.clone(),
                tmux_session_id: s.id.clone(),
                created_at: s.created,
            })
            .collect();
        SessionList {
            socket_path: self.socket_path(),
            server_running: server.is_some(),
            sessions,
            unmanaged,
        }
    }
    fn query(&self) -> Result<Option<Server>> {
        if !validate_socket(&self.socket_path(), true)? {
            return Ok(None);
        }
        let result = self.run_raw(&[
            "-N".into(),
            "display-message".into(),
            "-p".into(),
            format!("SERVER:#{{{MARKER}}}"),
            ";".into(),
            "list-sessions".into(),
            "-F".into(),
            "SESSION:#{session_id}\t#{session_created}\t#{session_name}".into(),
            ";".into(),
            "list-clients".into(),
            "-F".into(),
            "CLIENT:#{client_name}\t#{session_id}\t#{client_pid}".into(),
        ])?;
        if !result.success {
            if result.stdout.is_empty()
                && (result.stderr.contains("no server running")
                    || (result.stderr.contains("error connecting")
                        && (result.stderr.contains("Connection refused")
                            || result.stderr.contains("No such file or directory"))))
            {
                return Ok(None);
            }
            return Err(failure("query local tmux", result.stderr.trim()));
        }
        let mut marker = None;
        let mut sessions = Vec::new();
        let mut clients = Vec::new();
        let mut saw_marker = false;
        for line in result.stdout.lines() {
            if let Some(value) = line.strip_prefix("SERVER:") {
                if saw_marker {
                    return Err(MultiplexerError::Failure(
                        "tmux returned duplicate server identity".into(),
                    ));
                }
                saw_marker = true;
                marker = Uuid::parse_str(value).ok();
            } else if let Some(value) = line.strip_prefix("SESSION:") {
                let columns: Vec<_> = value.split('\t').collect();
                if columns.len() != 3
                    || !valid_session_id(columns[0])
                    || columns[2].len() > 128
                    || columns[2].chars().any(char::is_control)
                {
                    return Err(MultiplexerError::Failure(
                        "tmux returned invalid session metadata".into(),
                    ));
                }
                sessions.push(LiveSession {
                    id: columns[0].into(),
                    created: columns[1]
                        .parse()
                        .map_err(|e| failure("parse tmux creation time", e))?,
                    name: columns[2].into(),
                    clients: Vec::new(),
                });
            } else if let Some(value) = line.strip_prefix("CLIENT:") {
                let columns: Vec<_> = value.split('\t').collect();
                if columns.len() != 3
                    || columns[0].is_empty()
                    || columns[0].len() > 256
                    || columns[0].chars().any(char::is_control)
                    || !valid_session_id(columns[1])
                {
                    return Err(MultiplexerError::Failure(
                        "tmux returned invalid client metadata".into(),
                    ));
                }
                clients.push((
                    columns[1].to_owned(),
                    ClientInfo {
                        id: columns[0].into(),
                        process_id: columns[2]
                            .parse()
                            .map_err(|e| failure("parse tmux client PID", e))?,
                    },
                ));
            } else {
                return Err(MultiplexerError::Failure(
                    "tmux returned an unexpected response".into(),
                ));
            }
        }
        if !saw_marker {
            return Err(MultiplexerError::Failure(
                "tmux omitted server identity".into(),
            ));
        }
        for (id, client) in clients {
            if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
                session.clients.push(client);
            }
        }
        Ok(Some(Server { marker, sessions }))
    }
    fn lock_registry(&self) -> Result<(File, Registry)> {
        self.check_cancelled()?;
        ensure_directory(&self.root, true)?;
        let path = self.root.join("registry.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| failure("open local tmux lock", e))?;
        validate_file(&file, "registry lock")?;
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        loop {
            self.check_cancelled()?;
            match file.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(fs::TryLockError::WouldBlock) => return Err(MultiplexerError::Busy),
                Err(fs::TryLockError::Error(e)) => {
                    return Err(failure("lock local tmux registry", e));
                }
            }
        }
        let registry_path = self.root.join("registry.json");
        let registry = match OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&registry_path)
        {
            Ok(mut source) => {
                validate_file(&source, "registry")?;
                let mut bytes = Vec::new();
                source
                    .by_ref()
                    .take((MAX_REGISTRY_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|e| failure("read local tmux registry", e))?;
                if bytes.len() > MAX_REGISTRY_BYTES {
                    return Err(MultiplexerError::Unsafe(
                        "registry exceeds its byte bound".into(),
                    ));
                }
                let registry: Registry = serde_json::from_slice(&bytes)
                    .map_err(|e| failure("decode local tmux registry", e))?;
                validate_registry(&registry)?;
                registry
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Registry {
                version: 1,
                sessions: Vec::new(),
            },
            Err(e) => return Err(failure("open local tmux registry", e)),
        };
        Ok((file, registry))
    }
    fn save(&self, registry: &Registry) -> Result<()> {
        use std::io::Write;
        let mut saved = registry.clone();
        saved.version = 2;
        let bytes =
            serde_json::to_vec(&saved).map_err(|e| failure("encode local tmux registry", e))?;
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(MultiplexerError::Unsafe(
                "registry exceeds its byte bound".into(),
            ));
        }
        let temporary = self.root.join(format!("registry-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&temporary)
                .map_err(|e| failure("create registry temporary file", e))?;
            file.write_all(&bytes)
                .map_err(|e| failure("write local tmux registry", e))?;
            file.sync_all()
                .map_err(|e| failure("sync local tmux registry", e))?;
            fs::rename(&temporary, self.root.join("registry.json"))
                .map_err(|e| failure("replace local tmux registry", e))?;
            File::open(&self.root)
                .and_then(|f| f.sync_all())
                .map_err(|e| failure("sync local tmux profile", e))
        })();
        if result.is_err()
            && let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("zmux local tmux temporary cleanup: {error}");
        }
        result
    }
    fn base_args(&self) -> Vec<String> {
        vec![
            "-f".into(),
            "/dev/null".into(),
            "-S".into(),
            self.socket_path().to_string_lossy().into_owned(),
        ]
    }
    fn run(&self, args: &[String]) -> Result<String> {
        let output = self.run_raw(args)?;
        if output.success {
            Ok(output.stdout)
        } else if output.stderr.contains(REJECT) {
            Err(MultiplexerError::Stale)
        } else {
            Err(failure("local tmux command failed", output.stderr.trim()))
        }
    }
    fn run_raw(&self, args: &[String]) -> Result<Output> {
        self.check_cancelled()?;
        let mut command = Command::new(&self.binary);
        command
            .args(self.base_args())
            .args(args)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        clear_context(&mut command);
        command.env("ZMUX_LOCAL_TMUX_ROOT", &self.root);
        command.env("ZMUX_LOCAL_TMUX_PROFILE", &self.root);
        command.env("ZMUX_LOCAL_TMUX_BIN", &self.binary);
        run_bounded(command, self.cancellation.as_deref())
    }
}

fn selected_binary() -> Result<PathBuf> {
    Ok(if let Some(value) = env::var_os("ZMUX_LOCAL_TMUX_BIN") {
        PathBuf::from(value)
    } else {
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).map(|p|p.join("tmux")).find(|p|p.is_absolute() && fs::metadata(p).is_ok_and(|m|m.is_file() && m.permissions().mode()&0o111!=0)).ok_or_else(||MultiplexerError::Invalid("tmux executable was not found; install tmux or set ZMUX_LOCAL_TMUX_BIN to an absolute executable".into()))?
    })
}

/// Attach callers use this same environment policy before exec. No ambient app
/// credential or stale workspace/surface identity is exported into the durable owner.
pub fn clear_context(command: &mut Command) {
    for (name, _) in env::vars_os() {
        if name
            .to_str()
            .is_some_and(|n| n.starts_with("ZMUX_") || n == "TMUX" || n == "TMUX_PANE")
        {
            command.env_remove(name);
        }
    }
    command.env("ZMUX_LOCAL_TMUX", "1");
}
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(MultiplexerError::Invalid(
            "session names must contain 1–128 ASCII letters, digits, underscores or hyphens".into(),
        ))
    } else {
        Ok(())
    }
}
fn valid_session_id(id: &str) -> bool {
    id.strip_prefix('$')
        .is_some_and(|n| !n.is_empty() && n.len() <= 20 && n.bytes().all(|b| b.is_ascii_digit()))
}
fn validate_registry(registry: &Registry) -> Result<()> {
    if !matches!(registry.version, 1 | 2) || registry.sessions.len() > MAX_SESSIONS {
        return Err(MultiplexerError::Unsafe(
            "unsupported or oversized registry".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    let mut bindings = std::collections::HashSet::new();
    for r in &registry.sessions {
        if let Some(binding) = &r.gui_binding {
            binding.validate()?;
        }
        if r.name.is_empty() || r.name.len() > 128 || r.name.chars().any(char::is_control) {
            return Err(MultiplexerError::Unsafe(
                "invalid registry session name".into(),
            ));
        }
        if r.id.is_nil()
            || r.server_incarnation.is_nil()
            || !valid_session_id(&r.tmux_session_id)
            || r.created_at == 0
            || !Path::new(&r.cwd).is_absolute()
            || r.cwd.len() > 4096
            || r.cwd.chars().any(char::is_control)
            || !ids.insert(r.id)
            || !bindings.insert((
                r.server_incarnation,
                r.tmux_session_id.clone(),
                r.created_at,
            ))
        {
            return Err(MultiplexerError::Unsafe(
                "invalid or duplicate registry identity".into(),
            ));
        }
    }
    Ok(())
}
fn live_record<'a>(record: &Record, server: Option<&'a Server>) -> Option<&'a LiveSession> {
    let server = server?;
    if server.marker != Some(record.server_incarnation) {
        return None;
    }
    server
        .sessions
        .iter()
        .find(|s| s.id == record.tmux_session_id && s.created == record.created_at)
}
fn status(record: &Record, server: Option<&Server>) -> SessionStatus {
    let live = live_record(record, server);
    SessionStatus {
        id: record.id,
        name: record.name.clone(),
        state: if live.is_some() { "live" } else { "stale" },
        server_incarnation: record.server_incarnation,
        tmux_session_id: record.tmux_session_id.clone(),
        created_at: record.created_at,
        cwd: record.cwd.clone(),
        clients: live.map(|s| s.clients.clone()).unwrap_or_default(),
        gui_binding: record.gui_binding.clone(),
    }
}
fn find_record<'a>(registry: &'a Registry, selector: &SessionSelector) -> Result<&'a Record> {
    let mut found = registry.sessions.iter().filter(|r| match selector {
        SessionSelector::Name(name) => r.name == *name,
        SessionSelector::Id(id) => r.id == *id,
    });
    let record = found.next().ok_or(MultiplexerError::NotFound)?;
    if found.next().is_some() {
        return Err(MultiplexerError::Invalid(
            "session name is ambiguous; use --id".into(),
        ));
    }
    Ok(record)
}
fn refresh_names(registry: &mut Registry, server: Option<&Server>) -> bool {
    let mut changed = false;
    for record in &mut registry.sessions {
        if let Some(live) = live_record(record, server)
            && record.name != live.name
        {
            record.name = live.name.clone();
            changed = true;
        }
    }
    changed
}
fn binding_condition(record: &Record) -> String {
    format!(
        "#{{&&:#{{==:#{{{MARKER}}},{}}},#{{&&:#{{==:#{{session_id}},{}}},#{{==:#{{session_created}},{}}}}}}}",
        record.server_incarnation, record.tmux_session_id, record.created_at
    )
}
fn tmux_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn tmux_command(args: &[String]) -> String {
    args.iter()
        .map(|v| tmux_quote(v))
        .collect::<Vec<_>>()
        .join(" ")
}
fn ensure_directory(path: &Path, private: bool) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(failure("create local tmux directory", e)),
    }
    validate_directory(path, private)
}
fn validate_directory(path: &Path, private: bool) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| failure("inspect local tmux directory", e))?;
    if !metadata.is_dir()
        || metadata.uid() != geteuid().as_raw()
        || metadata.permissions().mode() & 0o022 != 0
        || (private && metadata.permissions().mode() & 0o777 != 0o700)
    {
        return Err(MultiplexerError::Unsafe("directory must be user-owned, nonsymlink and unwritable by other users; profile mode must be 0700".into()));
    }
    Ok(())
}
fn validate_file(file: &File, label: &str) -> Result<()> {
    let m = file
        .metadata()
        .map_err(|e| failure("inspect profile file", e))?;
    if !m.is_file()
        || m.uid() != geteuid().as_raw()
        || m.permissions().mode() & 0o777 != 0o600
        || m.nlink() != 1
    {
        return Err(MultiplexerError::Unsafe(format!(
            "{label} must be a single-link user-owned regular file with mode 0600"
        )));
    }
    Ok(())
}
fn validate_socket(path: &Path, private: bool) -> Result<bool> {
    let m = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(failure("inspect local tmux socket", e)),
    };
    if !m.file_type().is_socket()
        || m.uid() != geteuid().as_raw()
        || (private && m.permissions().mode() & 0o077 != 0)
    {
        return Err(MultiplexerError::Unsafe(
            "socket must be a user-owned nonsymlink Unix socket with private permissions".into(),
        ));
    }
    Ok(true)
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}
struct Running {
    child: Child,
}
impl Drop for Running {
    fn drop(&mut self) {
        // The tmux server daemon creates its own session. Every remaining process
        // in the launcher's group belongs to this bounded invocation, even after
        // the leader exits or descendants close their inherited output pipes.
        if let Err(e) = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL)
            && e != nix::errno::Errno::ESRCH
        {
            eprintln!("zmux tmux process cancellation: {e}");
        }
        if let Err(e) = self.child.wait() {
            eprintln!("zmux tmux process reap: {e}");
        }
    }
}
fn run_bounded(mut command: Command, cancellation: Option<&AtomicBool>) -> Result<Output> {
    let mut child = Running {
        child: command.spawn().map_err(|e| failure("launch tmux", e))?,
    };
    let mut stdout = FileDescriptor::dup(&child.child.stdout.take().expect("piped stdout"))
        .map_err(|e| failure("open tmux output", e))?;
    let mut stderr = FileDescriptor::dup(&child.child.stderr.take().expect("piped stderr"))
        .map_err(|e| failure("open tmux errors", e))?;
    stdout
        .set_non_blocking(true)
        .map_err(|e| failure("configure tmux output", e))?;
    stderr
        .set_non_blocking(true)
        .map_err(|e| failure("configure tmux errors", e))?;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_eof = false;
    let mut err_eof = false;
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    let mut status = None;
    loop {
        if cancellation.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(MultiplexerError::Failure(
                "local tmux operation cancelled; inspect list if owner creation was pending".into(),
            ));
        }
        for (pipe, bytes, eof) in [
            (&mut stdout, &mut out, &mut out_eof),
            (&mut stderr, &mut err, &mut err_eof),
        ] {
            if *eof {
                continue;
            }
            for _ in 0..4 {
                let mut buffer = [0; 4096];
                match pipe.read(&mut buffer) {
                    Ok(0) => {
                        *eof = true;
                        break;
                    }
                    Ok(n) => {
                        if bytes.len() + n > MAX_OUTPUT_BYTES {
                            return Err(MultiplexerError::Failure(
                                "tmux response exceeded the 64 KiB stream bound".into(),
                            ));
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            || e.kind() == io::ErrorKind::Interrupted =>
                    {
                        break;
                    }
                    Err(e) => return Err(failure("read tmux response", e)),
                }
            }
        }
        if status.is_none() {
            status = child
                .child
                .try_wait()
                .map_err(|e| failure("poll tmux process", e))?;
        }
        if let Some(status) = status
            && out_eof
            && err_eof
        {
            return Ok(Output {
                success: status.success(),
                stdout: String::from_utf8(out).map_err(|e| failure("decode tmux output", e))?,
                stderr: String::from_utf8(err).map_err(|e| failure("decode tmux errors", e))?,
            });
        }
        if Instant::now() >= deadline {
            return Err(MultiplexerError::Timeout);
        }
        let mut fds = Vec::new();
        if !out_eof {
            fds.push(pollfd {
                fd: stdout.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if !err_eof {
            fds.push(pollfd {
                fd: stderr.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if let Err(e) = poll(&mut fds, Some(Duration::from_millis(10)))
            && !matches!(&e,filedescriptor::Error::Poll(e) if e.kind()==io::ErrorKind::Interrupted)
        {
            return Err(failure("poll tmux output", e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_kills_pipe_holding_descendant_after_leader_exits() {
        assert_descendant_cleaned("sleep 60 & echo $! > \"$1\"", true);
    }

    #[test]
    fn completed_command_kills_closed_stdio_descendant_after_leader_exits() {
        assert_descendant_cleaned(
            "sleep 60 </dev/null >/dev/null 2>&1 & echo $! > \"$1\"",
            false,
        );
    }

    fn assert_descendant_cleaned(script: &str, timeout: bool) {
        let root = std::env::temp_dir().join(format!("zmux-runner-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let pid_path = root.join("child.pid");
        // Exercise the real process runner with a shell; no tmux protocol is emulated.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script, "runner-test"]);
        command
            .arg(&pid_path)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let result = run_bounded(command, None);
        let pid: i32 = fs::read_to_string(&pid_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let (alive, state) = loop {
            let state = Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8(state.stdout).unwrap();
            let alive = !state.trim().is_empty() && !state.trim_start().starts_with('Z');
            if !alive || Instant::now() >= deadline {
                break (alive, state);
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // Always clean up this test's child even on the red regression.
        let _ = nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGKILL);
        fs::remove_dir_all(&root).unwrap();
        if timeout {
            assert!(matches!(result, Err(MultiplexerError::Timeout)));
        } else {
            assert!(result.is_ok_and(|output| output.success));
        }
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(
            !alive,
            "child {pid} survived launcher completion with state {state:?}"
        );
    }
}
