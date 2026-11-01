//! Private ownership of one OpenSSH multiplexed connection.
//!
//! Create the owner on a launch worker and retain it until every process using
//! the socket has finished. Drop queues non-recursive cleanup on a bounded worker;
//! it never waits for filesystem I/O on the core/UI executor.

use crate::{MAX_COMMAND_BYTES, ProcessSpec, SSH_PROGRAM, SshDescriptor};
use std::{
    fs, io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;

const CONTROL_ROOT: &str = "/tmp";
const CREATE_ATTEMPTS: u64 = 256;
const SOCKET_NAME: &str = "c";

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(0);

/// Errors creating, describing, or cleaning up a managed SSH connection.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("SSH cleanup worker is unavailable: {0}")]
    CleanupWorker(String),
    #[error("failed to create private SSH control directory {path:?}: {source}")]
    CreateDirectory { path: PathBuf, source: io::Error },
    #[error("could not allocate a unique private SSH control directory after {attempts} attempts")]
    DirectoryCollisions { attempts: u64 },
    #[error("failed to enforce private permissions on SSH control directory {path:?}: {source}")]
    SecureDirectory { path: PathBuf, source: io::Error },
    #[error("SSH operation command is blank")]
    EmptyOperationCommand,
    #[error("SSH operation command contains a control character")]
    InvalidOperationCommand,
    #[error("SSH operation command exceeds {MAX_COMMAND_BYTES} bytes")]
    OperationCommandTooLarge,
    #[error("managed SSH shell integration is unavailable for this connection")]
    ShellIntegrationUnavailable,
    #[error("OpenSSH could not resolve the control-master route for this connection")]
    ControlRouteUnavailable,
    #[error("failed to remove SSH control socket {path:?}: {source}")]
    RemoveSocket { path: PathBuf, source: io::Error },
    #[error("failed to remove private SSH control directory {path:?}: {source}")]
    RemoveDirectory { path: PathBuf, source: io::Error },
    #[cfg(not(unix))]
    #[error("managed SSH connections require a Unix host")]
    UnsupportedHost,
}

/// Immutable owner of one terminal's private OpenSSH control socket.
///
/// Wrap this value in an `Arc` when terminal and operation workers need shared
/// access. Exactly one live terminal process owns the master connection. Other
/// process specifications produced here can only use that master's socket.
#[derive(Debug)]
pub struct ManagedConnection {
    descriptor: SshDescriptor,
    integrate_shell: bool,
    route: Option<ControlRoute>,
    root: PathBuf,
    control_path: PathBuf,
}

#[derive(Clone, Debug)]
struct ControlRoute {
    host: String,
    user: String,
    port: u16,
}

impl ManagedConnection {
    /// Allocate an exclusive, mode-0700 control directory below `/tmp`.
    ///
    /// This method performs filesystem calls and belongs on a launch worker.
    pub fn create(descriptor: SshDescriptor) -> Result<Self, ConnectionError> {
        #[cfg(not(unix))]
        {
            let _ = descriptor;
            return Err(ConnectionError::UnsupportedHost);
        }

        #[cfg(unix)]
        {
            cleanup_sender().map_err(ConnectionError::CleanupWorker)?;
            let (integrate_shell, route) = inspect_connection(&descriptor);
            let root = create_private_root()?;
            let control_path = root.join(SOCKET_NAME);
            Ok(Self {
                descriptor,
                integrate_shell,
                route,
                root,
                control_path,
            })
        }
    }

    /// The private path shared by the terminal master and auxiliary clients.
    pub fn control_path(&self) -> &Path {
        &self.control_path
    }

    /// Whether the validated descriptor can safely use Zmux's managed login
    /// shell hook rather than an explicit/configured remote command.
    pub fn shell_integration_enabled(&self) -> bool {
        self.integrate_shell
    }

    /// Build the sole master process, retaining the descriptor's route,
    /// forwarding, destination, and remote command exactly as validated.
    pub fn terminal_spec(
        &self,
        nonce: &zmux_session_runtime::shell_integration::OpenRequestNonce,
    ) -> ProcessSpec {
        let original = self.descriptor.process_spec();
        let mut args = vec![
            "-S".into(),
            self.control_path_string(),
            "-o".into(),
            "ControlMaster=yes".into(),
            "-o".into(),
            "ControlPersist=no".into(),
            "-o".into(),
            "ForkAfterAuthentication=no".into(),
            "-tt".into(),
        ];
        args.extend(original.args);
        if self.integrate_shell {
            args.push(zmux_session_runtime::shell_integration::remote_command(
                nonce,
            ));
        }
        ProcessSpec {
            program: SSH_PROGRAM.into(),
            args,
        }
    }

    /// Build an auxiliary remote command that can only use the live master.
    ///
    /// Authentication, proxy, forwarding, and command settings are not copied.
    /// Only the resolved host/user/port are carried so OpenSSH addresses the
    /// same master socket. Ignoring configuration and fixing `ProxyCommand` to
    /// `/usr/bin/false` makes a missing master fail instead of reconnecting.
    pub fn operation_spec(&self, command: &str) -> Result<ProcessSpec, ConnectionError> {
        validate_operation_command(command)?;
        let route = self
            .route
            .as_ref()
            .ok_or(ConnectionError::ControlRouteUnavailable)?;
        Ok(ProcessSpec {
            program: SSH_PROGRAM.into(),
            args: vec![
                "-F".into(),
                "none".into(),
                "-S".into(),
                self.control_path_string(),
                "-o".into(),
                "ControlMaster=no".into(),
                "-o".into(),
                "ControlPersist=no".into(),
                "-o".into(),
                "BatchMode=yes".into(),
                "-o".into(),
                "ProxyCommand=/usr/bin/false".into(),
                "-o".into(),
                "PermitLocalCommand=no".into(),
                "-T".into(),
                "-p".into(),
                route.port.to_string(),
                "-l".into(),
                route.user.clone(),
                "--".into(),
                route.host.clone(),
                command.into(),
            ],
        })
    }

    /// Build the lifecycle reader on this connection's existing ControlMaster.
    /// The nonce-derived path and token contain only fixed ASCII hex and are
    /// scoped to the matching managed terminal bootstrap. A descriptor with a
    /// configured remote command cannot provide this adapter and fails closed.
    pub fn agent_lifecycle_spec(
        &self,
        nonce: &zmux_session_runtime::shell_integration::OpenRequestNonce,
    ) -> Result<ProcessSpec, ConnectionError> {
        if !self.integrate_shell {
            return Err(ConnectionError::ShellIntegrationUnavailable);
        }
        let root = nonce.remote_integration_directory();
        let token = nonce.remote_lifecycle_token();
        self.operation_spec(&format!(
            "ZMUX_SHELL_INTEGRATION_DIRECTORY={root} python3 {root}/claude-lifecycle-stream.py {token}"
        ))
    }

    /// Remove only this owner's known socket and then its now-empty directory.
    ///
    /// Call this after all terminal and operation processes have finished. An
    /// unexpected entry prevents directory removal and is never deleted.
    pub fn cleanup(&self) -> Result<(), ConnectionError> {
        match fs::remove_file(&self.control_path) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ConnectionError::RemoveSocket {
                    path: self.control_path.clone(),
                    source,
                });
            }
        }
        match fs::remove_dir(&self.root) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ConnectionError::RemoveDirectory {
                path: self.root.clone(),
                source,
            }),
        }
    }

    fn control_path_string(&self) -> String {
        // The path is assembled only from the ASCII constant `/tmp`, the process
        // id, a numeric counter, and `c`.
        self.control_path
            .to_str()
            .expect("generated SSH control path must be UTF-8")
            .into()
    }
}

/// Resolve only whether OpenSSH already owns the remote command. This bounded
/// probe runs on the launch worker; terminal_spec remains pure. In particular,
/// never override a configured RemoteCommand or append a conflicting command.
fn inspect_connection(descriptor: &SshDescriptor) -> (bool, Option<ControlRoute>) {
    let mut command = Command::new("/usr/bin/ssh");
    command
        .arg("-G")
        .args(descriptor.endpoint().transport_args())
        .arg("--")
        .arg(descriptor.endpoint().destination().to_string())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match crate::bounded_command::run(command, None) {
        Ok(output) if output.success => {
            let route = parse_control_route(&output.stdout);
            let remote_command = output.stdout.lines().any(|line| {
                line.split_once(' ')
                    .is_some_and(|(key, value)| key == "remotecommand" && value != "none")
            });
            let disabled = std::env::var_os("ZMUX_SHELL_INTEGRATION_DISABLED").as_deref()
                == Some(std::ffi::OsStr::new("1"));
            let integrate_shell =
                route.is_some() && descriptor.command().is_none() && !disabled && !remote_command;
            (integrate_shell, route)
        }
        _ => {
            // Keep the ordinary SSH launch and its own config diagnostics when
            // config cannot be resolved. Auxiliary commands fail closed because
            // they cannot prove the exact target identity. Do not print config.
            eprintln!(
                "zmux: SSH config probe unavailable; managed shell and operation integration disabled for this connection"
            );
            (false, None)
        }
    }
}

fn parse_control_route(output: &str) -> Option<ControlRoute> {
    let mut host = None;
    let mut user = None;
    let mut port = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        match key {
            "hostname" => host = Some(value.to_owned()),
            "user" => user = Some(value.to_owned()),
            "port" => port = value.parse::<u16>().ok(),
            _ => {}
        }
    }
    let host = host?;
    let user = user?;
    let port = port?;
    if host.is_empty()
        || host.len() > 255
        || user.is_empty()
        || user.len() > 255
        || host.chars().any(char::is_control)
        || user.chars().any(char::is_control)
    {
        return None;
    }
    Some(ControlRoute { host, user, port })
}

impl Drop for ManagedConnection {
    fn drop(&mut self) {
        match cleanup_sender() {
            Ok(sender) => {
                if let Err(error) =
                    sender.try_send(Cleanup::Paths(self.control_path.clone(), self.root.clone()))
                {
                    record_cleanup_issue(format!(
                        "SSH cleanup queue rejected {}: {error}",
                        self.root.display()
                    ));
                }
            }
            Err(error) => record_cleanup_issue(format!(
                "SSH cleanup unavailable for {}: {error}",
                self.root.display()
            )),
        }
    }
}

enum Cleanup {
    Paths(PathBuf, PathBuf),
    Barrier(SyncSender<CleanupReport>),
}
/// Process-wide bounded sticky diagnostics. A barrier proves preceding queued
/// work finished, not successful deletion; rejected or failed paths stay reported.
#[derive(Clone, Debug, Default)]
pub struct CleanupReport {
    pub issues: Vec<String>,
    pub issues_dropped: u64,
}
static CLEANUP_ISSUES: Mutex<CleanupReport> = Mutex::new(CleanupReport {
    issues: Vec::new(),
    issues_dropped: 0,
});
fn record_cleanup_issue(message: String) {
    eprintln!("zmux {message}");
    let mut report = CLEANUP_ISSUES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if report.issues.len() < 16 {
        report.issues.push(message.chars().take(512).collect());
    } else {
        report.issues_dropped = report.issues_dropped.saturating_add(1);
    }
}
static CLEANUP: OnceLock<Result<SyncSender<Cleanup>, String>> = OnceLock::new();
fn cleanup_sender() -> Result<&'static SyncSender<Cleanup>, String> {
    CLEANUP
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<Cleanup>(64);
            std::thread::Builder::new()
                .name("zmux-ssh-cleanup".into())
                .spawn(move || {
                    while let Ok(message) = receiver.recv() {
                        let (socket, root) = match message {
                            Cleanup::Paths(socket, root) => (socket, root),
                            Cleanup::Barrier(ack) => {
                                let report = CLEANUP_ISSUES
                                    .lock()
                                    .unwrap_or_else(|poison| poison.into_inner())
                                    .clone();
                                let _ = ack.try_send(report);
                                continue;
                            }
                        };
                        for (path, result) in [
                            (&socket, fs::remove_file(&socket)),
                            (&root, fs::remove_dir(&root)),
                        ] {
                            if let Err(error) = result
                                && error.kind() != io::ErrorKind::NotFound
                            {
                                record_cleanup_issue(format!(
                                    "SSH cleanup failed for {}: {error}",
                                    path.display()
                                ));
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(sender)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Wait for previously queued cleanup only from a shutdown/background worker.
pub fn drain_cleanup(timeout: std::time::Duration) -> Result<(), ConnectionError> {
    let report = request_cleanup_barrier()?
        .recv_timeout(timeout)
        .map_err(|e| ConnectionError::CleanupWorker(e.to_string()))?;
    if report.issues.is_empty() && report.issues_dropped == 0 {
        Ok(())
    } else {
        Err(ConnectionError::CleanupWorker(format!(
            "process-wide prior cleanup failures: {:?}; additional={}",
            report.issues, report.issues_dropped
        )))
    }
}

/// Retain this one barrier while polling; sending a replacement cannot acknowledge
/// the original cleanup operation. No wait occurs on the action executor.
pub fn request_cleanup_barrier() -> Result<mpsc::Receiver<CleanupReport>, ConnectionError> {
    let sender = cleanup_sender().map_err(ConnectionError::CleanupWorker)?;
    let (ack, receiver) = mpsc::sync_channel(1);
    sender
        .try_send(Cleanup::Barrier(ack))
        .map_err(|e| ConnectionError::CleanupWorker(e.to_string()))?;
    Ok(receiver)
}

#[cfg(unix)]
fn create_private_root() -> Result<PathBuf, ConnectionError> {
    let process_id = std::process::id();
    for _ in 0..CREATE_ATTEMPTS {
        let id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let path = Path::new(CONTROL_ROOT).join(format!("zmx-{process_id:x}-{nonce:x}-{id:x}"));
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => {
                if let Err(source) = fs::set_permissions(
                    &path,
                    <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
                ) {
                    if let Err(error) = fs::remove_dir(&path) {
                        eprintln!("zmux SSH directory cleanup failed: {error}");
                    }
                    return Err(ConnectionError::SecureDirectory { path, source });
                }
                return Ok(path);
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(ConnectionError::CreateDirectory { path, source }),
        }
    }
    Err(ConnectionError::DirectoryCollisions {
        attempts: CREATE_ATTEMPTS,
    })
}

fn validate_operation_command(command: &str) -> Result<(), ConnectionError> {
    if command.trim().is_empty() {
        return Err(ConnectionError::EmptyOperationCommand);
    }
    if command.len() > MAX_COMMAND_BYTES {
        return Err(ConnectionError::OperationCommandTooLarge);
    }
    if command.chars().any(char::is_control) {
        return Err(ConnectionError::InvalidOperationCommand);
    }
    Ok(())
}
