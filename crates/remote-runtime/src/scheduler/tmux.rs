//! Default-server tmux discovery, target revalidation, and literal pane input.

use super::{
    DeliveryPhase, DispatchCheckpoint, DispatchReceipt, JobFailureCode,
    MAX_DISPATCH_LATENESS_SECONDS, ScheduledJob, TargetDisplay, TargetIdentity,
};
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, poll, pollfd};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    collections::HashMap,
    env,
    io::{self, Read, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zmux_session_runtime::child::{ProcessIdentity, process_identity};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_TARGETS: usize = 100;
const MAX_DISPLAY_LABEL_BYTES: usize = 128;
const BUFFER_PREFIX: &str = "zmux-schedule-";

#[derive(Debug, Error)]
pub(super) enum TmuxError {
    #[error("tmux server is unavailable")]
    Unavailable,
    #[error("tmux target no longer exists")]
    Missing,
    #[error("tmux target left its bound session or server")]
    Changed,
    #[error("tmux command failed: {0}")]
    Rejected(String),
    #[error("tmux command timed out")]
    Timeout,
}

#[derive(Clone, Debug)]
pub(super) struct Tmux {
    socket: PathBuf,
    binary: OsStringOrPath,
}

#[derive(Clone, Debug)]
enum OsStringOrPath {
    SearchPath,
    Path(PathBuf),
}

#[derive(Clone, Debug)]
pub(super) struct TmuxStatus {
    pub socket_id: String,
    pub server_incarnation: Option<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerSnapshot {
    pid: u32,
    start_time: u64,
    socket_id: String,
}

#[derive(Clone, Debug)]
struct PaneSnapshot {
    server: ServerSnapshot,
    session_id: String,
    pane_id: String,
    window_id: String,
    window_index: u32,
    pane_index: u32,
    display: TargetDisplay,
}

struct PaneSnapshotBatch {
    server: ServerSnapshot,
    process: ProcessIdentity,
    panes: Vec<PaneSnapshot>,
}

impl ServerSnapshot {
    fn parse(pid: &str, start_time: &str, socket_id: &str) -> Result<Self, TmuxError> {
        let pid = pid
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)
            .ok_or_else(|| TmuxError::Rejected("tmux returned an invalid server PID".into()))?;
        let start_time = start_time
            .parse::<u64>()
            .ok()
            .filter(|start_time| *start_time > 0)
            .ok_or_else(|| {
                TmuxError::Rejected("tmux returned an invalid server start time".into())
            })?;
        if !Path::new(socket_id).is_absolute()
            || socket_id.len() > 512
            || socket_id.chars().any(char::is_control)
        {
            return Err(TmuxError::Rejected(
                "tmux returned an invalid socket path".into(),
            ));
        }
        Ok(Self {
            pid,
            start_time,
            socket_id: socket_id.to_string(),
        })
    }
}

#[derive(Debug)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Tmux {
    /// Build a tmux client from explicit paths using the same validation as the
    /// production environment constructor. This also supports controlled local
    /// sockets for integration tests without a test-only dispatch path.
    pub(super) fn from_config(
        socket: Option<PathBuf>,
        binary: Option<PathBuf>,
    ) -> Result<Self, TmuxError> {
        let uid = nix::unistd::geteuid().as_raw();
        let socket = match socket {
            Some(path)
                if path.is_absolute()
                    && path.as_os_str().as_encoded_bytes().len() <= 512
                    && path
                        .to_str()
                        .is_some_and(|value| !value.chars().any(char::is_control)) =>
            {
                path
            }
            Some(_) => {
                return Err(TmuxError::Rejected(
                    "ZMUX_SCHEDULER_TMUX_SOCKET must be an absolute path of at most 512 bytes"
                        .into(),
                ));
            }
            None => default_socket(uid)?,
        };
        let binary = match binary {
            Some(path) if path.is_absolute() => OsStringOrPath::Path(path),
            Some(_) => {
                return Err(TmuxError::Rejected(
                    "ZMUX_SCHEDULER_TMUX_BIN must be an absolute executable path".into(),
                ));
            }
            None => OsStringOrPath::SearchPath,
        };
        Ok(Self { socket, binary })
    }

    pub(super) fn status(&self) -> Result<Option<TmuxStatus>, TmuxError> {
        let Some(snapshot) = self.server_snapshot()? else {
            return Ok(None);
        };
        let process = process_identity(snapshot.pid).map_err(|_| TmuxError::Changed)?;
        let server_incarnation = process_incarnation(&snapshot, &process);
        Ok(Some(TmuxStatus {
            socket_id: snapshot.socket_id,
            server_incarnation: Some(server_incarnation),
        }))
    }

    pub(super) fn targets(
        &self,
        helper_instance_id: Uuid,
        remote_uid: u32,
    ) -> Result<Vec<super::SchedulerTarget>, TmuxError> {
        let Some(batch) = self.list_panes()? else {
            return Ok(Vec::new());
        };
        if batch.process.uid != remote_uid {
            return Err(TmuxError::Changed);
        }
        let server_incarnation = process_incarnation(&batch.server, &batch.process);
        let mut targets = Vec::with_capacity(batch.panes.len().min(MAX_TARGETS));
        for (
            server,
            session_id,
            pane_id,
            window_id,
            window_index,
            pane_index,
            session_name,
            window_name,
            pane_title,
        ) in batch.panes.into_iter().take(MAX_TARGETS).map(|pane| {
            (
                pane.server,
                pane.session_id,
                pane.pane_id,
                pane.window_id,
                pane.window_index,
                pane.pane_index,
                pane.display.session_name,
                pane.display.window_name,
                pane.display.pane_title,
            )
        }) {
            debug_assert_eq!(server, batch.server);
            let identity = TargetIdentity {
                helper_instance_id,
                remote_uid,
                tmux_socket_id: batch.server.socket_id.clone(),
                server_incarnation,
                server_pid: batch.server.pid,
                server_start_time: batch.server.start_time,
                session_id,
                pane_id,
            };
            let Ok(id) = identity.target_id() else {
                continue;
            };
            targets.push(super::SchedulerTarget {
                id,
                identity,
                display: TargetDisplay {
                    session_name,
                    window_id,
                    window_name,
                    window_index,
                    pane_index,
                    pane_title,
                },
            });
        }
        targets.sort_by(|a, b| {
            a.identity
                .session_id
                .cmp(&b.identity.session_id)
                .then(a.identity.pane_id.cmp(&b.identity.pane_id))
        });
        Ok(targets)
    }

    /// Refresh display data for a list in one bounded tmux snapshot and one
    /// server process identity sample. Jobs that no longer match retain their
    /// last stored display metadata; dispatch still performs strict checking.
    pub(super) fn displays_for(
        &self,
        identities: &[TargetIdentity],
    ) -> Result<Vec<Option<TargetDisplay>>, TmuxError> {
        if identities.is_empty() {
            return Ok(Vec::new());
        }
        let Some(batch) = self.list_panes()? else {
            return Ok(vec![None; identities.len()]);
        };
        let server_incarnation = process_incarnation(&batch.server, &batch.process);
        let panes: HashMap<_, _> = batch
            .panes
            .iter()
            .map(|pane| ((pane.session_id.as_str(), pane.pane_id.as_str()), pane))
            .collect();
        Ok(identities
            .iter()
            .map(|identity| {
                if identity.remote_uid != batch.process.uid
                    || identity.tmux_socket_id != batch.server.socket_id
                    || identity.server_pid != batch.server.pid
                    || identity.server_start_time != batch.server.start_time
                    || identity.server_incarnation != server_incarnation
                {
                    return None;
                }
                panes
                    .get(&(identity.session_id.as_str(), identity.pane_id.as_str()))
                    .map(|pane| pane.display.clone())
            })
            .collect())
    }

    pub(super) fn verify_target(
        &self,
        identity: &TargetIdentity,
    ) -> Result<TargetDisplay, TmuxError> {
        self.display_for(identity)
    }

    pub(super) fn dispatch(
        &self,
        job: &ScheduledJob,
        mut persist_text_checkpoint: impl FnMut(&DispatchReceipt) -> bool,
        refreshed_display: &mut Option<TargetDisplay>,
    ) -> DispatchResult {
        let mut receipt = DispatchReceipt {
            text: DeliveryPhase::NotStarted,
            submit: DeliveryPhase::NotStarted,
            last_durable_phase: DispatchCheckpoint::Claimed,
        };
        match self.verify_target(&job.target) {
            Ok(display) => *refreshed_display = Some(display),
            Err(TmuxError::Missing) => {
                return DispatchResult::Failed(receipt, JobFailureCode::TargetMissing);
            }
            Err(TmuxError::Changed) => {
                return DispatchResult::Failed(receipt, JobFailureCode::TargetChanged);
            }
            Err(TmuxError::Unavailable) => {
                return DispatchResult::Failed(receipt, JobFailureCode::TmuxUnavailable);
            }
            Err(TmuxError::Rejected(_)) | Err(TmuxError::Timeout) => {
                return DispatchResult::Failed(receipt, JobFailureCode::TmuxRejected);
            }
        };
        if past_dispatch_grace(job) {
            return DispatchResult::Missed(receipt);
        }
        let buffer = format!("{BUFFER_PREFIX}{}", job.id.simple());
        if self
            .run(
                &["load-buffer", "-b", &buffer, "-"],
                Some(job.message.as_bytes()),
            )
            .is_err()
        {
            let _ = self.run(&["delete-buffer", "-b", &buffer], None);
            return DispatchResult::Failed(receipt, JobFailureCode::TmuxRejected);
        }
        if past_dispatch_grace(job) {
            let _ = self.run(&["delete-buffer", "-b", &buffer], None);
            return DispatchResult::Missed(receipt);
        }

        // The pane ID is immutable and has already been rechecked against the
        // original session on the same server identity. -r preserves LF bytes;
        // -p adds bracket delimiters only when the target application requested them.
        match self.run_guarded(&job.target, &buffer, "paste-buffer", "") {
            Ok(_) => {}
            Err(TmuxError::Changed) => {
                let _ = self.run(&["delete-buffer", "-b", &buffer], None);
                return DispatchResult::Failed(receipt, JobFailureCode::TargetChanged);
            }
            Err(TmuxError::Missing) => {
                let _ = self.run(&["delete-buffer", "-b", &buffer], None);
                return DispatchResult::Failed(receipt, JobFailureCode::TargetMissing);
            }
            Err(TmuxError::Unavailable) => {
                let _ = self.run(&["delete-buffer", "-b", &buffer], None);
                return DispatchResult::Failed(receipt, JobFailureCode::TmuxUnavailable);
            }
            Err(_) => {
                receipt.text = DeliveryPhase::Unknown;
                let _ = self.run(&["delete-buffer", "-b", &buffer], None);
                return DispatchResult::Unknown(receipt);
            }
        }
        receipt.text = DeliveryPhase::Accepted;
        let mut durable = receipt.clone();
        durable.last_durable_phase = DispatchCheckpoint::TextAccepted;
        if !persist_text_checkpoint(&durable) {
            let _ = self.run(&["delete-buffer", "-b", &buffer], None);
            return DispatchResult::Unknown(receipt);
        }
        receipt.last_durable_phase = DispatchCheckpoint::TextAccepted;
        let _ = self.run(&["delete-buffer", "-b", &buffer], None);
        if job.submit {
            if past_dispatch_grace(job) {
                return DispatchResult::Unknown(receipt);
            }
            match self.run_guarded(&job.target, &buffer, "send-keys -t", "Enter") {
                Ok(_) => {
                    receipt.submit = DeliveryPhase::Accepted;
                    receipt.last_durable_phase = DispatchCheckpoint::SubmitAccepted;
                }
                Err(TmuxError::Changed) => {
                    receipt.submit = DeliveryPhase::Failed;
                    return DispatchResult::Failed(receipt, JobFailureCode::TargetChanged);
                }
                Err(TmuxError::Missing) => {
                    receipt.submit = DeliveryPhase::Failed;
                    return DispatchResult::Failed(receipt, JobFailureCode::TargetMissing);
                }
                Err(TmuxError::Unavailable) => {
                    receipt.submit = DeliveryPhase::Failed;
                    return DispatchResult::Failed(receipt, JobFailureCode::TmuxUnavailable);
                }
                Err(_) => {
                    receipt.submit = DeliveryPhase::Unknown;
                    return DispatchResult::Unknown(receipt);
                }
            }
        }
        DispatchResult::Sent(receipt)
    }

    fn server_snapshot(&self) -> Result<Option<ServerSnapshot>, TmuxError> {
        let format = ["#{pid}", "#{start_time}", "#{socket_path}"].join("\t");
        match self.run(&["display-message", "-p", "-F", &format], None) {
            Ok(output) => {
                let fields: Vec<_> = output
                    .stdout
                    .trim_end_matches(['\r', '\n'])
                    .splitn(3, '\t')
                    .collect();
                if fields.len() != 3 {
                    return Err(TmuxError::Rejected(
                        "tmux returned an invalid server snapshot".into(),
                    ));
                }
                Ok(Some(ServerSnapshot::parse(
                    fields[0], fields[1], fields[2],
                )?))
            }
            Err(TmuxError::Unavailable) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn list_panes(&self) -> Result<Option<PaneSnapshotBatch>, TmuxError> {
        let output = self.run(&["list-panes", "-a", "-F", &pane_format()], None)?;
        let panes: Vec<_> = output
            .stdout
            .lines()
            .filter_map(parse_pane_snapshot)
            .collect();
        let Some(server) = panes.first().map(|pane| pane.server.clone()) else {
            return Ok(None);
        };
        if panes.iter().any(|pane| pane.server != server) {
            return Err(TmuxError::Changed);
        }
        let process = process_identity(server.pid).map_err(|_| TmuxError::Changed)?;
        Ok(Some(PaneSnapshotBatch {
            server,
            process,
            panes,
        }))
    }

    fn display_for(&self, identity: &TargetIdentity) -> Result<TargetDisplay, TmuxError> {
        let format = pane_format();
        let output = self.run(
            &[
                "display-message",
                "-p",
                "-t",
                &identity.pane_id,
                "-F",
                &format,
            ],
            None,
        )?;
        let line = output.stdout.trim_end_matches(['\r', '\n']);
        let fields: Vec<_> = line.splitn(11, '\t').collect();
        if fields.len() != 11 {
            return Err(TmuxError::Missing);
        }
        let snapshot = ServerSnapshot::parse(fields[8], fields[9], fields[10])?;
        if snapshot.socket_id != identity.tmux_socket_id
            || snapshot.pid != identity.server_pid
            || snapshot.start_time != identity.server_start_time
        {
            return Err(TmuxError::Changed);
        }
        let process = process_identity(snapshot.pid).map_err(|_| TmuxError::Changed)?;
        if process.uid != identity.remote_uid
            || process_incarnation(&snapshot, &process) != identity.server_incarnation
        {
            return Err(TmuxError::Changed);
        }
        if fields[0] != identity.session_id || fields[1] != identity.pane_id {
            return Err(TmuxError::Changed);
        }
        let window_id = fields[2].to_string();
        if !valid_id(&window_id, b'@') {
            return Err(TmuxError::Rejected(
                "tmux returned invalid window metadata".into(),
            ));
        }
        let window_index = fields[3]
            .parse::<u32>()
            .map_err(|_| TmuxError::Rejected("tmux returned invalid window index".into()))?;
        let pane_index = fields[4]
            .parse::<u32>()
            .map_err(|_| TmuxError::Rejected("tmux returned invalid pane index".into()))?;
        Ok(TargetDisplay {
            session_name: display_text(fields[5]),
            window_id,
            window_name: display_text(fields[6]),
            window_index,
            pane_index,
            pane_title: display_text(fields[7]),
        })
    }

    fn run_guarded(
        &self,
        identity: &TargetIdentity,
        buffer: &str,
        command: &str,
        argument: &str,
    ) -> Result<CommandOutput, TmuxError> {
        let condition = guard_format(identity);
        let then_command = match command {
            "paste-buffer" => format!("paste-buffer -p -r -b {buffer} -t {}", identity.pane_id),
            "send-keys -t" => format!("send-keys -t {} {argument}", identity.pane_id),
            _ => {
                return Err(TmuxError::Rejected(
                    "unsupported guarded tmux command".into(),
                ));
            }
        };
        let output = self.run(
            &[
                "if-shell",
                "-F",
                "-t",
                &identity.pane_id,
                &condition,
                &then_command,
                "display-message -p __ZMUX_SCHEDULER_TARGET_CHANGED__",
            ],
            None,
        )?;
        if output.stdout.contains("__ZMUX_SCHEDULER_TARGET_CHANGED__") {
            return Err(TmuxError::Changed);
        }
        Ok(output)
    }

    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<CommandOutput, TmuxError> {
        self.run_with_limit(args, input, MAX_OUTPUT_BYTES)
    }

    fn run_with_limit(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
        output_limit: usize,
    ) -> Result<CommandOutput, TmuxError> {
        let mut command = match &self.binary {
            OsStringOrPath::SearchPath => Command::new("tmux"),
            OsStringOrPath::Path(path) => Command::new(path),
        };
        command
            .arg("-f")
            .arg("/dev/null")
            .arg("-S")
            .arg(&self.socket)
            .arg("-u")
            .args(args)
            .process_group(0)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("LC_ALL", "C.UTF-8")
            .env("LANG", "C.UTF-8")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                TmuxError::Unavailable
            } else {
                TmuxError::Rejected(safe_error(&error.to_string()))
            }
        })?;
        let mut stdout = FileDescriptor::dup(&child.stdout.take().expect("piped stdout"))
            .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?;
        let mut stderr = FileDescriptor::dup(&child.stderr.take().expect("piped stderr"))
            .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?;
        stdout
            .set_non_blocking(true)
            .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?;
        stderr
            .set_non_blocking(true)
            .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?;
        if let Some(bytes) = input {
            // Scheduler messages are capped at 4096 bytes, well below a pipe's
            // capacity. No message bytes enter argv, environment, or diagnostics.
            let write_result = child.stdin.take().expect("piped stdin").write_all(bytes);
            if let Err(error) = write_result {
                kill_child_group(child.id());
                let _ = child.wait();
                return Err(TmuxError::Rejected(safe_error(&error.to_string())));
            }
        }
        let deadline = Instant::now() + OPERATION_TIMEOUT;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (mut out_eof, mut err_eof) = (false, false);
        let mut status = None;
        loop {
            for (pipe, buffer, eof) in [
                (&mut stdout, &mut out, &mut out_eof),
                (&mut stderr, &mut err, &mut err_eof),
            ] {
                if *eof {
                    continue;
                }
                for _ in 0..4 {
                    let mut bytes = [0u8; 4_096];
                    match pipe.read(&mut bytes) {
                        Ok(0) => {
                            *eof = true;
                            break;
                        }
                        Ok(count) => {
                            if buffer.len() + count > output_limit {
                                kill_child_group(child.id());
                                let _ = child.wait();
                                return Err(TmuxError::Rejected(
                                    "tmux output exceeded its configured bound".into(),
                                ));
                            }
                            buffer.extend_from_slice(&bytes[..count]);
                        }
                        Err(error)
                            if error.kind() == io::ErrorKind::WouldBlock
                                || error.kind() == io::ErrorKind::Interrupted =>
                        {
                            break;
                        }
                        Err(error) => {
                            kill_child_group(child.id());
                            let _ = child.wait();
                            return Err(TmuxError::Rejected(safe_error(&error.to_string())));
                        }
                    }
                }
            }
            if status.is_none() {
                status = child
                    .try_wait()
                    .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?;
            }
            if status.is_some() && out_eof && err_eof {
                let output = CommandOutput {
                    success: status.is_some_and(|status| status.success()),
                    stdout: String::from_utf8(out)
                        .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?,
                    stderr: String::from_utf8(err)
                        .map_err(|error| TmuxError::Rejected(safe_error(&error.to_string())))?,
                };
                if output.success {
                    return Ok(output);
                }
                let diagnostic = safe_error(output.stderr.trim());
                if diagnostic.contains("no server running")
                    || diagnostic.contains("error connecting")
                    || diagnostic.contains("No such file or directory")
                    || diagnostic.contains("Connection refused")
                {
                    return Err(TmuxError::Unavailable);
                }
                if diagnostic.contains("can't find pane") {
                    return Err(TmuxError::Missing);
                }
                return Err(TmuxError::Rejected(diagnostic));
            }
            if Instant::now() >= deadline {
                kill_child_group(child.id());
                let _ = child.wait();
                return Err(TmuxError::Timeout);
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
            if let Err(error) = poll(&mut fds, Some(Duration::from_millis(10)))
                && !matches!(&error, filedescriptor::Error::Poll(error) if error.kind() == io::ErrorKind::Interrupted)
            {
                kill_child_group(child.id());
                let _ = child.wait();
                return Err(TmuxError::Rejected(safe_error(&error.to_string())));
            }
        }
    }
}

#[derive(Debug)]
pub(super) enum DispatchResult {
    Sent(DispatchReceipt),
    Failed(DispatchReceipt, JobFailureCode),
    Unknown(DispatchReceipt),
    Missed(DispatchReceipt),
}

fn past_dispatch_grace(job: &ScheduledJob) -> bool {
    let Ok(deadline) = OffsetDateTime::parse(&job.deadline_utc, &Rfc3339) else {
        return true;
    };
    OffsetDateTime::now_utc()
        .unix_timestamp_nanos()
        .saturating_sub(deadline.unix_timestamp_nanos())
        > i128::from(MAX_DISPATCH_LATENESS_SECONDS) * 1_000_000_000
}

fn default_socket(uid: u32) -> Result<PathBuf, TmuxError> {
    let base = env::var_os("TMUX_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    if !base.is_absolute() {
        return Err(TmuxError::Rejected("TMUX_TMPDIR must be absolute".into()));
    }
    Ok(base.join(format!("tmux-{uid}")).join("default"))
}

fn process_incarnation(snapshot: &ServerSnapshot, process: &ProcessIdentity) -> Uuid {
    let name = format!(
        "zmux-scheduler-server-v1\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        snapshot.socket_id,
        process.pid,
        process.uid,
        process.start[0],
        process.start[1],
        process.boot,
        snapshot.start_time
    );
    Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes())
}

fn guard_format(identity: &TargetIdentity) -> String {
    let mut condition = String::from("#{&&:#{==:#{pid},");
    condition.push_str(&identity.server_pid.to_string());
    condition.push_str("},#{&&:#{==:#{start_time},");
    condition.push_str(&identity.server_start_time.to_string());
    condition.push_str("},#{&&:#{==:#{session_id},");
    condition.push_str(&identity.session_id);
    condition.push_str("},#{==:#{pane_id},");
    condition.push_str(&identity.pane_id);
    condition.push_str("}}}}");
    condition
}

fn valid_id(value: &str, prefix: u8) -> bool {
    value.as_bytes().first() == Some(&prefix)
        && value.len() > 1
        && value.as_bytes()[1..].iter().all(u8::is_ascii_digit)
}

fn pane_format() -> String {
    [
        "#{session_id}",
        "#{pane_id}",
        "#{window_id}",
        "#{window_index}",
        "#{pane_index}",
        "#{q:#{=128:session_name}}",
        "#{q:#{=128:window_name}}",
        "#{q:#{=128:pane_title}}",
        "#{pid}",
        "#{start_time}",
        "#{socket_path}",
    ]
    .join("\t")
}

fn parse_pane_snapshot(line: &str) -> Option<PaneSnapshot> {
    let fields: Vec<_> = line.splitn(11, '\t').collect();
    if fields.len() != 11 || !valid_id(fields[0], b'$') || !valid_id(fields[1], b'%') {
        return None;
    }
    let window_id = fields[2].to_string();
    if !valid_id(&window_id, b'@') {
        return None;
    }
    let window_index = fields[3].parse::<u32>().ok()?;
    let pane_index = fields[4].parse::<u32>().ok()?;
    let server = ServerSnapshot::parse(fields[8], fields[9], fields[10]).ok()?;
    Some(PaneSnapshot {
        server,
        session_id: fields[0].to_string(),
        pane_id: fields[1].to_string(),
        window_id,
        window_index,
        pane_index,
        display: TargetDisplay {
            session_name: display_text(fields[5]),
            window_id: fields[2].to_string(),
            window_name: display_text(fields[6]),
            window_index,
            pane_index,
            pane_title: display_text(fields[7]),
        },
    })
}

fn display_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(MAX_DISPLAY_LABEL_BYTES));
    for character in value.chars().filter(|character| !character.is_control()) {
        if output.len() + character.len_utf8() > MAX_DISPLAY_LABEL_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

fn safe_error(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

fn kill_child_group(pid: u32) {
    if let Err(error) = killpg(Pid::from_raw(pid as i32), Signal::SIGKILL)
        && error != nix::errno::Errno::ESRCH
    {
        eprintln!("zmux scheduler tmux process cleanup failed: {error}");
    }
}
