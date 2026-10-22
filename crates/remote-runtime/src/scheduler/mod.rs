//! Versioned requests and durable job types for the per-account remote scheduler.
//!
//! This module owns the public JSON contract shared by the CLI, desktop transport,
//! and helper process. The daemon deliberately does not interpret setup operations;
//! those are intercepted by the managed-SSH bootstrap transport.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub mod daemon;
mod service;
mod store;
mod store_state;
mod tmux;

pub use service::SchedulerService;

pub const SCHEDULER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 4_096;
pub const MIN_AHEAD_SECONDS: u64 = 1;
pub const MAX_AHEAD_SECONDS: u64 = 31_536_000;
pub const MAX_PENDING_JOBS: usize = 100;
pub const MAX_LIST_LIMIT: u16 = 100;
pub const MAX_HISTORY_LIMIT: u16 = 100;
pub const MAX_RETAINED_HISTORY: usize = 100;
pub const MAX_REQUEST_BYTES: usize = 16 * 1_024;
pub const MAX_RESPONSE_BYTES: usize = 1_024 * 1_024;
pub const RECEIPT_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const STORE_LOCK_TIMEOUT_MILLIS: u64 = 5_000;
/// A live daemon may send a task up to five seconds after its deadline.
/// Later work is marked missed; this is a best-effort bound, not a real-time guarantee.
pub const MAX_DISPATCH_LATENESS_SECONDS: u64 = 5;

/// One bounded request to the remote account's scheduler.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerRequest {
    pub protocol_version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    pub action: SchedulerAction,
}

impl SchedulerRequest {
    pub fn validate(&self) -> Result<(), SchedulerValidationError> {
        if self.protocol_version != SCHEDULER_PROTOCOL_VERSION {
            return Err(SchedulerValidationError::UnsupportedProtocol);
        }
        if self
            .request_id
            .is_some_and(|request_id| request_id.is_nil())
        {
            return Err(SchedulerValidationError::NilRequestId);
        }
        if self.action.requires_request_id() && self.request_id.is_none() {
            return Err(SchedulerValidationError::RequestIdRequired);
        }
        self.action.validate()
    }
}

/// Setup variants are reserved for the managed-SSH bootstrap transport. A request
/// containing either variant must never be sent to the foreground daemon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SchedulerAction {
    SetupCheck,
    SetupInstall,
    Status {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receipt_request_id: Option<Uuid>,
    },
    Targets,
    Create {
        target_id: String,
        deadline: DeadlineInput,
        message: String,
        submit: bool,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u16>,
    },
    Show {
        job_id: Uuid,
    },
    Edit {
        job_id: Uuid,
        expected_version: u64,
        patch: EditPatch,
    },
    Cancel {
        job_id: Uuid,
        expected_version: u64,
    },
    History {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u16>,
    },
}

impl SchedulerAction {
    /// Whether this operation changes durable state or performs installation.
    pub const fn requires_request_id(&self) -> bool {
        matches!(
            self,
            Self::SetupInstall | Self::Create { .. } | Self::Edit { .. } | Self::Cancel { .. }
        )
    }

    pub const fn is_read(&self) -> bool {
        !self.requires_request_id()
    }

    pub fn validate(&self) -> Result<(), SchedulerValidationError> {
        match self {
            Self::SetupCheck | Self::SetupInstall | Self::Targets => Ok(()),
            Self::Status { receipt_request_id } => {
                if receipt_request_id.is_some_and(|request_id| request_id.is_nil()) {
                    Err(SchedulerValidationError::NilRequestId)
                } else {
                    Ok(())
                }
            }
            Self::Create {
                target_id,
                deadline,
                message,
                ..
            } => {
                TargetIdentity::from_target_id(target_id)?;
                deadline.validate()?;
                validate_message(message)
            }
            Self::List { limit } | Self::History { limit } => {
                if let Some(limit) = limit {
                    let maximum = if matches!(self, Self::History { .. }) {
                        MAX_HISTORY_LIMIT
                    } else {
                        MAX_LIST_LIMIT
                    };
                    if *limit == 0 || *limit > maximum {
                        return Err(SchedulerValidationError::InvalidLimit(maximum));
                    }
                }
                Ok(())
            }
            Self::Show { job_id } => validate_uuid(*job_id),
            Self::Edit {
                job_id,
                expected_version,
                patch,
            } => {
                validate_uuid(*job_id)?;
                if *expected_version == 0 {
                    return Err(SchedulerValidationError::InvalidVersion);
                }
                patch.validate()
            }
            Self::Cancel {
                job_id,
                expected_version,
            } => {
                validate_uuid(*job_id)?;
                if *expected_version == 0 {
                    return Err(SchedulerValidationError::InvalidVersion);
                }
                Ok(())
            }
        }
    }
}

fn validate_uuid(value: Uuid) -> Result<(), SchedulerValidationError> {
    if value.is_nil() {
        Err(SchedulerValidationError::NilIdentifier)
    } else {
        Ok(())
    }
}

/// Relative durations are resolved against the helper's acceptance time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeadlineInput {
    At { rfc3339: String },
    InSeconds { seconds: u64 },
}

impl DeadlineInput {
    pub fn validate(&self) -> Result<(), SchedulerValidationError> {
        match self {
            Self::At { rfc3339 } => validate_rfc3339(rfc3339),
            Self::InSeconds { seconds }
                if !(MIN_AHEAD_SECONDS..=MAX_AHEAD_SECONDS).contains(seconds) =>
            {
                Err(SchedulerValidationError::InvalidDuration)
            }
            Self::InSeconds { .. } => Ok(()),
        }
    }

    fn resolve(
        &self,
        accepted_at: OffsetDateTime,
    ) -> Result<ResolvedDeadline, SchedulerValidationError> {
        self.validate()?;
        let (instant, source_offset_seconds) = match self {
            Self::At { rfc3339 } => {
                let parsed = parse_rfc3339(rfc3339)?;
                let offset = parsed.offset().whole_seconds();
                (parsed.to_offset(time::UtcOffset::UTC), Some(offset))
            }
            Self::InSeconds { seconds } => {
                (accepted_at + time::Duration::seconds(*seconds as i64), None)
            }
        };
        let now_nanos = accepted_at.unix_timestamp_nanos();
        let due_nanos = instant.unix_timestamp_nanos();
        if due_nanos <= now_nanos
            || due_nanos - now_nanos > (MAX_AHEAD_SECONDS as i128) * 1_000_000_000
        {
            return Err(SchedulerValidationError::OutsideScheduleWindow);
        }
        let due_at_unix_nanos = i64::try_from(due_nanos)
            .map_err(|_| SchedulerValidationError::OutsideScheduleWindow)?;
        let deadline_utc = instant
            .format(&Rfc3339)
            .map_err(|_| SchedulerValidationError::InvalidTimestamp)?;
        Ok(ResolvedDeadline {
            due_at_unix_nanos,
            deadline_utc,
            source_offset_seconds,
            input: self.clone(),
        })
    }
}

/// A single-field or combined edit. Empty patches are invalid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DeadlineInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl EditPatch {
    pub fn validate(&self) -> Result<(), SchedulerValidationError> {
        if self.deadline.is_none() && self.message.is_none() {
            return Err(SchedulerValidationError::EmptyPatch);
        }
        if let Some(deadline) = &self.deadline {
            deadline.validate()?;
        }
        if let Some(message) = &self.message {
            validate_message(message)?;
        }
        Ok(())
    }
}

/// Validate RFC3339 timestamps accepted by both CLI and helper.
///
/// The timestamp must include `Z` or a numeric `±HH:MM` offset. `-00:00` and
/// leap seconds are rejected because they do not identify a stable POSIX instant.
pub fn validate_rfc3339(value: &str) -> Result<(), SchedulerValidationError> {
    parse_rfc3339(value).map(|_| ())
}

fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, SchedulerValidationError> {
    if value.len() < 20
        || value.len() > 64
        || value.ends_with("-00:00")
        || value.as_bytes().get(17..19) == Some(b"60")
    {
        return Err(SchedulerValidationError::InvalidTimestamp);
    }
    validate_fraction_digits(value)?;
    let mut canonical = value.to_owned();
    if canonical.as_bytes().get(10) == Some(&b't') {
        canonical.replace_range(10..11, "T");
    }
    if canonical.ends_with('z') {
        canonical.pop();
        canonical.push('Z');
    }
    OffsetDateTime::parse(&canonical, &Rfc3339)
        .map_err(|_| SchedulerValidationError::InvalidTimestamp)
}

fn validate_fraction_digits(value: &str) -> Result<(), SchedulerValidationError> {
    if value.as_bytes().get(19) != Some(&b'.') {
        return Ok(());
    }
    let bytes = value.as_bytes();
    let start = 20;
    let end = bytes[start..]
        .iter()
        .position(|byte| matches!(byte, b'Z' | b'z' | b'+' | b'-'))
        .map(|offset| start + offset)
        .unwrap_or(bytes.len());
    let digits = end.saturating_sub(start);
    if !(1..=9).contains(&digits) || !bytes[start..end].iter().all(u8::is_ascii_digit) {
        return Err(SchedulerValidationError::InvalidTimestamp);
    }
    Ok(())
}

/// Validate message bytes before a request reaches the durable store.
///
/// LF and tab are retained as literal message bytes. NUL, carriage return, escape,
/// and all other control characters are rejected.
pub fn validate_message(message: &str) -> Result<(), SchedulerValidationError> {
    if message.is_empty() {
        return Err(SchedulerValidationError::EmptyMessage);
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(SchedulerValidationError::MessageTooLarge);
    }
    if message
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(SchedulerValidationError::UnsupportedControl);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
// Keep the success payload directly matchable in the shared CLI/transport API.
#[allow(clippy::large_enum_variant)]
pub enum SchedulerResult {
    Success(SchedulerReply),
    Error(SchedulerError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerResponse {
    pub protocol_version: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
    pub result: SchedulerResult,
}

impl SchedulerResponse {
    pub fn success(request: &SchedulerRequest, reply: SchedulerReply) -> Self {
        Self {
            protocol_version: SCHEDULER_PROTOCOL_VERSION,
            request_id: request.request_id,
            result: SchedulerResult::Success(reply),
        }
    }

    pub fn error(request_id: Option<Uuid>, error: SchedulerError) -> Self {
        Self {
            protocol_version: SCHEDULER_PROTOCOL_VERSION,
            request_id,
            result: SchedulerResult::Error(error),
        }
    }
}

/// Stable reply shapes. `kind` and fields are top-level inside `data`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchedulerReply {
    Status {
        status: SchedulerStatus,
    },
    Targets {
        targets: Vec<SchedulerTarget>,
    },
    Setup {
        setup: serde_json::Value,
    },
    Mutation {
        job: ScheduledJob,
        receipt: RequestReceipt,
    },
    Job {
        job: ScheduledJob,
    },
    Jobs {
        jobs: Vec<ScheduledJob>,
    },
    History {
        jobs: Vec<ScheduledJob>,
        truncated: bool,
        pruned_before_utc: Option<String>,
    },
    RequestStatus {
        receipt: RequestReceipt,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerStatus {
    /// Stable for one helper installation and preserved across daemon restarts.
    pub helper_instance_id: Uuid,
    pub remote_uid: u32,
    pub helper_version: String,
    pub protocol_version: u16,
    pub service_mode: ServiceMode,
    pub store_health: StoreHealth,
    pub checked_at_utc: String,
    pub tmux_available: bool,
    pub default_tmux_socket_id: Option<String>,
    pub server_incarnation: Option<Uuid>,
    pub linger_enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    ManualForeground,
    SystemdUser,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreHealth {
    Healthy,
    Degraded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerTarget {
    /// Opaque v1 token returned by target discovery and required by Create.
    pub id: String,
    pub identity: TargetIdentity,
    pub display: TargetDisplay,
}

/// Immutable target identity. Display names and indices live in [`TargetDisplay`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetIdentity {
    pub helper_instance_id: Uuid,
    pub remote_uid: u32,
    pub tmux_socket_id: String,
    pub server_incarnation: Uuid,
    pub server_pid: u32,
    pub server_start_time: u64,
    pub session_id: String,
    pub pane_id: String,
}

impl TargetIdentity {
    pub fn validate(&self) -> Result<(), SchedulerValidationError> {
        if self.helper_instance_id.is_nil()
            || self.server_incarnation.is_nil()
            || self.server_pid == 0
            || self.server_pid > i32::MAX as u32
            || self.server_start_time == 0
        {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        if !self.tmux_socket_id.starts_with('/')
            || self.tmux_socket_id.len() > 512
            || self.tmux_socket_id.chars().any(char::is_control)
        {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        if !valid_tmux_object_id(&self.session_id, b'$')
            || !valid_tmux_object_id(&self.pane_id, b'%')
        {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        Ok(())
    }

    pub fn target_id(&self) -> Result<String, SchedulerValidationError> {
        self.validate()?;
        Ok(format!(
            "v1.{}.{}.{}.{}.{}.{}.{}.{}",
            self.helper_instance_id.simple(),
            self.remote_uid,
            self.server_incarnation.simple(),
            self.server_pid,
            self.server_start_time,
            encode_hex(self.tmux_socket_id.as_bytes()),
            self.session_id,
            self.pane_id
        ))
    }

    pub fn from_target_id(value: &str) -> Result<Self, SchedulerValidationError> {
        if value.len() > 1_024 {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != 9 || parts[0] != "v1" {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        let helper_instance_id =
            Uuid::parse_str(parts[1]).map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let remote_uid = parts[2]
            .parse::<u32>()
            .map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let server_incarnation =
            Uuid::parse_str(parts[3]).map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let server_pid = parts[4]
            .parse::<u32>()
            .map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let server_start_time = parts[5]
            .parse::<u64>()
            .map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let socket_bytes = decode_hex(parts[6])?;
        let tmux_socket_id = String::from_utf8(socket_bytes)
            .map_err(|_| SchedulerValidationError::InvalidTargetId)?;
        let identity = Self {
            helper_instance_id,
            remote_uid,
            tmux_socket_id,
            server_incarnation,
            server_pid,
            server_start_time,
            session_id: parts[7].into(),
            pane_id: parts[8].into(),
        };
        identity.validate()?;
        if identity.target_id()? != value {
            return Err(SchedulerValidationError::InvalidTargetId);
        }
        Ok(identity)
    }
}

fn valid_tmux_object_id(value: &str, prefix: u8) -> bool {
    let bytes = value.as_bytes();
    bytes.first() == Some(&prefix) && bytes.len() > 1 && bytes[1..].iter().all(u8::is_ascii_digit)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn decode_hex(value: &str) -> Result<Vec<u8>, SchedulerValidationError> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(SchedulerValidationError::InvalidTargetId);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Result<u8, SchedulerValidationError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(SchedulerValidationError::InvalidTargetId),
    }
}

/// Mutable names and indices for user display only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDisplay {
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub window_index: u32,
    pub pane_index: u32,
    pub pane_title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledJob {
    pub id: Uuid,
    pub version: u64,
    pub state: JobState,
    pub message: String,
    /// Resolved absolute instant, formatted canonically in UTC with `Z`.
    pub deadline_utc: String,
    /// Last requested input preserves the source offset for native rendering/audit.
    pub deadline_input: DeadlineInput,
    pub submit: bool,
    pub target: TargetIdentity,
    pub display: TargetDisplay,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub dispatch: Option<DispatchReceipt>,
    pub failure_code: Option<JobFailureCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Dispatching,
    Sent,
    Failed,
    Cancelled,
    Missed,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPhase {
    NotStarted,
    Accepted,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchCheckpoint {
    Claimed,
    TextAccepted,
    SubmitAccepted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchReceipt {
    pub text: DeliveryPhase,
    pub submit: DeliveryPhase,
    pub last_durable_phase: DispatchCheckpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobFailureCode {
    TargetMissing,
    TargetChanged,
    TmuxRejected,
    TmuxUnavailable,
    InvalidMessage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    Create,
    Edit,
    Cancel,
    SetupInstall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Applied,
    Rejected,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestReceipt {
    pub request_id: Uuid,
    pub operation: MutationKind,
    pub state: ReceiptState,
    pub job_id: Option<Uuid>,
    pub job_version: Option<u64>,
    pub received_at_utc: String,
    pub completed_at_utc: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerErrorCode {
    InvalidRequest,
    UnsupportedProtocol,
    UnsupportedHost,
    ServiceUnavailable,
    RequestIdRequired,
    RequestIdConflict,
    RequestExpired,
    NotFoundOrExpired,
    NotFound,
    InvalidTarget,
    TargetMissing,
    TargetChanged,
    StaleVersion,
    AlreadyClaimed,
    Capacity,
    Busy,
    StoreUnavailable,
    TmuxUnavailable,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerError {
    pub code: SchedulerErrorCode,
    pub message: String,
    pub retryable: bool,
    pub outcome_unknown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<u64>,
}

impl SchedulerError {
    pub fn new(code: SchedulerErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            outcome_unknown: false,
            current_version: None,
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SchedulerValidationError {
    #[error("unsupported scheduler protocol version")]
    UnsupportedProtocol,
    #[error("a non-nil request ID is required for this operation")]
    RequestIdRequired,
    #[error("request ID cannot be nil")]
    NilRequestId,
    #[error("identifier cannot be nil")]
    NilIdentifier,
    #[error("invalid or unsupported RFC3339 timestamp")]
    InvalidTimestamp,
    #[error("duration must be between 1 second and 365 days")]
    InvalidDuration,
    #[error("deadline must be in the future and no more than 365 days away")]
    OutsideScheduleWindow,
    #[error("message cannot be empty")]
    EmptyMessage,
    #[error("message exceeds the 4096 UTF-8 byte limit")]
    MessageTooLarge,
    #[error("message contains an unsupported control character")]
    UnsupportedControl,
    #[error("edit must change a message, deadline, or both")]
    EmptyPatch,
    #[error("job version must be greater than zero")]
    InvalidVersion,
    #[error("limit must be between 1 and {0}")]
    InvalidLimit(u16),
    #[error("invalid opaque target ID")]
    InvalidTargetId,
}

impl SchedulerValidationError {
    pub fn error_code(&self) -> SchedulerErrorCode {
        match self {
            Self::UnsupportedProtocol => SchedulerErrorCode::UnsupportedProtocol,
            Self::RequestIdRequired => SchedulerErrorCode::RequestIdRequired,
            Self::InvalidTargetId => SchedulerErrorCode::InvalidTarget,
            _ => SchedulerErrorCode::InvalidRequest,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ResolvedDeadline {
    due_at_unix_nanos: i64,
    deadline_utc: String,
    source_offset_seconds: Option<i32>,
    input: DeadlineInput,
}
