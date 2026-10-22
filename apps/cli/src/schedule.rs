//! CLI parser and private reconciliation journal for the remote scheduler.
use super::*;
use nix::{
    fcntl::{Flock, FlockArg},
    libc,
    unistd::geteuid,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use zmux_remote_runtime::scheduler::{
    DeadlineInput, EditPatch, MAX_AHEAD_SECONDS, MAX_HISTORY_LIMIT, MAX_LIST_LIMIT,
    MAX_MESSAGE_BYTES, ReceiptState, SCHEDULER_PROTOCOL_VERSION, SchedulerAction, SchedulerReply,
    SchedulerRequest, SchedulerResponse, SchedulerResult,
    validate_message as validate_scheduler_message, validate_rfc3339 as validate_scheduler_rfc3339,
};

const JOURNAL_DIRECTORY: &str = "schedule-requests";
const JOURNAL_FILE: &str = "requests.json";
const LOCK_FILE: &str = "lock";
const JOURNAL_VERSION: u32 = 1;
const JOURNAL_LIMIT: usize = 256;
const JOURNAL_BYTES: usize = 128 * 1024;

#[derive(Clone, Serialize)]
enum Deadline {
    At(String),
    InSeconds(u64),
}

#[derive(Clone, Serialize)]
enum MessageSource {
    Text(String),
    Stdin,
}

#[derive(Serialize)]
enum Operation {
    SetupCheck,
    SetupInstall,
    Status {
        receipt_request_id: Option<Uuid>,
    },
    Targets,
    Create {
        target_id: String,
        deadline: Deadline,
        message: MessageSource,
        submit: bool,
    },
    List {
        limit: Option<u16>,
    },
    Show {
        job_id: Uuid,
    },
    Edit {
        job_id: Uuid,
        expected_version: u64,
        deadline: Option<Deadline>,
        message: Option<MessageSource>,
    },
    Cancel {
        job_id: Uuid,
        expected_version: u64,
    },
    History {
        limit: Option<u16>,
    },
}

pub(super) struct Invocation {
    surface: Uuid,
    pub(super) relative: bool,
    request_id: Option<Uuid>,
    operation: Operation,
}

impl Invocation {
    fn is_mutation(&self) -> bool {
        matches!(
            &self.operation,
            Operation::SetupInstall
                | Operation::Create { .. }
                | Operation::Edit { .. }
                | Operation::Cancel { .. }
        )
    }

    fn has_durable_receipt(&self) -> bool {
        matches!(
            &self.operation,
            Operation::Create { .. } | Operation::Edit { .. } | Operation::Cancel { .. }
        )
    }

    fn action(&self) -> SchedulerAction {
        match &self.operation {
            Operation::SetupCheck => SchedulerAction::SetupCheck,
            Operation::SetupInstall => SchedulerAction::SetupInstall,
            Operation::Status { receipt_request_id } => SchedulerAction::Status {
                receipt_request_id: *receipt_request_id,
            },
            Operation::Targets => SchedulerAction::Targets,
            Operation::Create {
                target_id,
                deadline,
                message,
                submit,
            } => SchedulerAction::Create {
                target_id: target_id.clone(),
                deadline: deadline.input(),
                message: message
                    .text()
                    .expect("stdin is resolved before dispatch")
                    .to_owned(),
                submit: *submit,
            },
            Operation::List { limit } => SchedulerAction::List { limit: *limit },
            Operation::Show { job_id } => SchedulerAction::Show { job_id: *job_id },
            Operation::Edit {
                job_id,
                expected_version,
                deadline,
                message,
            } => SchedulerAction::Edit {
                job_id: *job_id,
                expected_version: *expected_version,
                patch: EditPatch {
                    deadline: deadline.as_ref().map(Deadline::input),
                    message: message.as_ref().map(|source| {
                        source
                            .text()
                            .expect("stdin is resolved before dispatch")
                            .to_owned()
                    }),
                },
            },
            Operation::Cancel {
                job_id,
                expected_version,
            } => SchedulerAction::Cancel {
                job_id: *job_id,
                expected_version: *expected_version,
            },
            Operation::History { limit } => SchedulerAction::History { limit: *limit },
        }
    }

    pub(super) fn discovery(&self) -> Value {
        json!({
            "method": "schedule",
            "surface": self.surface,
            "request": SchedulerRequest {
                protocol_version: SCHEDULER_PROTOCOL_VERSION,
                // Parser probes are public catalog data. Keep their mutation ID
                // deterministic and visibly synthetic instead of exposing the
                // freshly generated ID for this parse.
                request_id: self.is_mutation().then_some(Uuid::from_u128(1)),
                action: self.action_for_discovery(),
            }
        })
    }

    fn action_for_discovery(&self) -> SchedulerAction {
        match &self.operation {
            Operation::Create {
                message: MessageSource::Stdin,
                ..
            } => {
                let mut operation = self.clone_for_discovery();
                if let Operation::Create { message, .. } = &mut operation.operation {
                    *message = MessageSource::Text("schedule message from stdin".into());
                }
                operation.action()
            }
            Operation::Edit {
                message: Some(MessageSource::Stdin),
                ..
            } => {
                let mut operation = self.clone_for_discovery();
                if let Operation::Edit { message, .. } = &mut operation.operation {
                    *message = Some(MessageSource::Text("schedule message from stdin".into()));
                }
                operation.action()
            }
            _ => self.action(),
        }
    }

    fn clone_for_discovery(&self) -> Self {
        Self {
            surface: self.surface,
            relative: self.relative,
            request_id: self.request_id,
            operation: self.operation.clone(),
        }
    }

    fn validate(&self) -> Result<(), CliError> {
        SchedulerRequest {
            protocol_version: SCHEDULER_PROTOCOL_VERSION,
            request_id: self.request_id,
            action: self.action_for_discovery(),
        }
        .validate()
        .map_err(|error| CliError(error.to_string()))
    }

    fn resolve_stdin(&mut self) -> Result<(), CliError> {
        let needs_stdin = match &self.operation {
            Operation::Create { message, .. } => matches!(message, MessageSource::Stdin),
            Operation::Edit { message, .. } => {
                matches!(message, Some(MessageSource::Stdin))
            }
            _ => false,
        };
        if !needs_stdin {
            return Ok(());
        }

        let mut bytes = Vec::with_capacity(MAX_MESSAGE_BYTES + 1);
        io::stdin()
            .lock()
            .take((MAX_MESSAGE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(CliError(format!(
                "schedule stdin message exceeds the {MAX_MESSAGE_BYTES} UTF-8 byte limit"
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| CliError("schedule stdin message must be valid UTF-8".into()))?;
        validate_message(&text)?;
        match &mut self.operation {
            Operation::Create { message, .. } => *message = MessageSource::Text(text),
            Operation::Edit { message, .. } => *message = Some(MessageSource::Text(text)),
            _ => unreachable!("stdin source only exists on create/edit"),
        }
        Ok(())
    }
}

impl Clone for Operation {
    fn clone(&self) -> Self {
        match self {
            Self::SetupCheck => Self::SetupCheck,
            Self::SetupInstall => Self::SetupInstall,
            Self::Status { receipt_request_id } => Self::Status {
                receipt_request_id: *receipt_request_id,
            },
            Self::Targets => Self::Targets,
            Self::Create {
                target_id,
                deadline,
                message,
                submit,
            } => Self::Create {
                target_id: target_id.clone(),
                deadline: deadline.clone(),
                message: message.clone(),
                submit: *submit,
            },
            Self::List { limit } => Self::List { limit: *limit },
            Self::Show { job_id } => Self::Show { job_id: *job_id },
            Self::Edit {
                job_id,
                expected_version,
                deadline,
                message,
            } => Self::Edit {
                job_id: *job_id,
                expected_version: *expected_version,
                deadline: deadline.clone(),
                message: message.clone(),
            },
            Self::Cancel {
                job_id,
                expected_version,
            } => Self::Cancel {
                job_id: *job_id,
                expected_version: *expected_version,
            },
            Self::History { limit } => Self::History { limit: *limit },
        }
    }
}

impl Deadline {
    fn input(&self) -> DeadlineInput {
        match self {
            Self::At(rfc3339) => DeadlineInput::At {
                rfc3339: rfc3339.clone(),
            },
            Self::InSeconds(seconds) => DeadlineInput::InSeconds { seconds: *seconds },
        }
    }
}

impl MessageSource {
    fn text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Stdin => None,
        }
    }
}

fn option_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, CliError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| CliError(format!("{flag} requires a value")))
}

fn once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError(format!("repeated {flag}")));
    }
    *slot = Some(value);
    Ok(())
}

fn uuid(value: &str, label: &str) -> Result<Uuid, CliError> {
    let id = value
        .parse::<Uuid>()
        .map_err(|_| CliError(format!("{label} must be a UUID")))?;
    if id.is_nil() {
        return Err(CliError(format!("{label} cannot be nil")));
    }
    Ok(id)
}

fn deadline(at: Option<String>, after: Option<u64>) -> Result<Deadline, CliError> {
    match (at, after) {
        (Some(value), None) => {
            validate_rfc3339(&value)?;
            Ok(Deadline::At(value))
        }
        (None, Some(seconds)) => Ok(Deadline::InSeconds(seconds)),
        _ => Err(CliError("choose exactly one of --at or --in".into())),
    }
}

fn validate_message(value: &str) -> Result<(), CliError> {
    validate_scheduler_message(value).map_err(|error| CliError(error.to_string()))
}

fn parse_duration(value: &str) -> Result<u64, CliError> {
    if value.is_empty() {
        return Err(CliError(
            "--in requires a duration such as 30s, 5m, 2h or 1d".into(),
        ));
    }
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut previous_rank = 4;
    let mut seconds = 0u64;
    while index < bytes.len() {
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if start == index || index == bytes.len() {
            return Err(CliError(
                "--in requires integer units in descending order: d, h, m, s".into(),
            ));
        }
        let count = value[start..index]
            .parse::<u64>()
            .map_err(|_| CliError("--in duration is out of range".into()))?;
        let (rank, scale) = match bytes[index] {
            b'd' => (3, 86_400),
            b'h' => (2, 3_600),
            b'm' => (1, 60),
            b's' => (0, 1),
            _ => {
                return Err(CliError(
                    "--in units must be d, h, m or s (for example 1h30m)".into(),
                ));
            }
        };
        if rank >= previous_rank {
            return Err(CliError(
                "--in units must appear once in descending order: d, h, m, s".into(),
            ));
        }
        previous_rank = rank;
        seconds = seconds
            .checked_add(
                count
                    .checked_mul(scale)
                    .ok_or_else(|| CliError("--in duration is out of range".into()))?,
            )
            .ok_or_else(|| CliError("--in duration is out of range".into()))?;
        index += 1;
    }
    if !(1..=MAX_AHEAD_SECONDS).contains(&seconds) {
        return Err(CliError(
            "--in must be between 1 second and 365 days".into(),
        ));
    }
    Ok(seconds)
}

fn validate_rfc3339(value: &str) -> Result<(), CliError> {
    validate_scheduler_rfc3339(value)
        .map_err(|error| CliError(format!("--at must be valid RFC3339: {error}")))
}

fn target_id(value: &str) -> Result<String, CliError> {
    let raw_pane = value.strip_prefix('%').is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    });
    if value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control) || raw_pane {
        return Err(CliError(
            "--remote-pane requires an identity-fenced target token returned by `schedule targets`; raw tmux pane IDs are not accepted".into(),
        ));
    }
    Ok(value.to_owned())
}

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if tail.is_empty() {
        return Ok(Parsed::Offline(Offline::Help(Some("schedule".into()))));
    }
    let args = utf8_args(tail)?;
    if args[0] == "help" {
        return match args.as_slice() {
            [_] => Ok(Parsed::Offline(Offline::Help(Some("schedule".into())))),
            [_, verb] => Ok(Parsed::Offline(Offline::Help(Some(format!(
                "schedule {verb}"
            ))))),
            _ => Err(CliError("schedule help accepts at most one verb".into())),
        };
    }
    let verb = args[0].as_str();
    if matches!(verb, "-h" | "--help") {
        return Ok(Parsed::Offline(Offline::Help(Some("schedule".into()))));
    }
    if args.len() == 2 && matches!(args[1].as_str(), "-h" | "--help") {
        return Ok(Parsed::Offline(Offline::Help(Some(format!(
            "schedule {verb}"
        )))));
    }
    if !matches!(
        verb,
        "setup" | "status" | "targets" | "create" | "list" | "show" | "edit" | "cancel" | "history"
    ) {
        return Err(CliError(
            "unknown schedule verb; run `zmux help schedule`".into(),
        ));
    }

    let mut surface = None;
    let mut request_id = None;
    let mut receipt_request_id = None;
    let mut remote_pane = None;
    let mut at = None;
    let mut after = None;
    let mut message = None;
    let mut submit = false;
    let mut check = false;
    let mut expected_version = None;
    let mut limit = None;
    let mut relative = false;
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        if matches!(flag, "--help" | "-h") {
            return Ok(Parsed::Offline(Offline::Help(Some(format!(
                "schedule {verb}"
            )))));
        }
        match flag {
            "--surface" => {
                let value = uuid(&option_value(&args, &mut index, flag)?, "--surface")?;
                once(&mut surface, value, flag)?;
            }
            "--request-id" => {
                let value = uuid(&option_value(&args, &mut index, flag)?, "--request-id")?;
                if verb == "status" {
                    once(&mut receipt_request_id, value, flag)?;
                } else {
                    once(&mut request_id, value, flag)?;
                }
            }
            "--remote-pane" => {
                let value = target_id(&option_value(&args, &mut index, flag)?)?;
                once(&mut remote_pane, value, flag)?;
            }
            "--at" => {
                let value = option_value(&args, &mut index, flag)?;
                validate_rfc3339(&value)?;
                once(&mut at, value, flag)?;
            }
            "--in" => {
                let value = parse_duration(&option_value(&args, &mut index, flag)?)?;
                once(&mut after, value, flag)?;
            }
            "--message" => {
                let value = option_value(&args, &mut index, flag)?;
                validate_message(&value)?;
                once(&mut message, MessageSource::Text(value), flag)?;
            }
            "--stdin" if message.is_none() => message = Some(MessageSource::Stdin),
            "--relative" if !relative && verb == "list" => relative = true,
            "--submit" if !submit => submit = true,
            "--check" if !check => check = true,
            "--expect-version" => {
                let value = option_value(&args, &mut index, flag)?
                    .parse::<u64>()
                    .ok()
                    .filter(|version| *version > 0)
                    .ok_or_else(|| {
                        CliError("--expect-version must be a positive integer".into())
                    })?;
                once(&mut expected_version, value, flag)?;
            }
            "--limit" => {
                let value = option_value(&args, &mut index, flag)?
                    .parse::<u16>()
                    .map_err(|_| CliError("--limit must be an integer".into()))?;
                let maximum = if verb == "history" {
                    MAX_HISTORY_LIMIT
                } else {
                    MAX_LIST_LIMIT
                };
                if !(1..=maximum).contains(&value) {
                    return Err(CliError(format!("--limit must be 1–{maximum}")));
                }
                once(&mut limit, value, flag)?;
            }
            "--" => {
                return Err(CliError(
                    "schedule does not accept trailing positional arguments".into(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError(format!(
                    "unknown or repeated schedule {verb} option"
                )));
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }

    let surface =
        surface.ok_or_else(|| CliError(format!("schedule {verb} requires --surface UUID")))?;
    let unused_options = UnusedScheduleOptions {
        positionals: &positionals,
        remote_pane: remote_pane.is_some(),
        at: at.is_some(),
        after: after.is_some(),
        message: message.is_some(),
        submit,
        check,
        expected_version: expected_version.is_some(),
        limit: limit.is_some(),
        request_id: request_id.is_some(),
    };
    let (operation, mutation) = match verb {
        "setup" => {
            if !positionals.is_empty()
                || remote_pane.is_some()
                || at.is_some()
                || after.is_some()
                || message.is_some()
                || submit
                || expected_version.is_some()
                || limit.is_some()
                || receipt_request_id.is_some()
            {
                return Err(CliError(
                    "schedule setup accepts only --surface and optional --check".into(),
                ));
            }
            (
                if check {
                    Operation::SetupCheck
                } else {
                    Operation::SetupInstall
                },
                !check,
            )
        }
        "status" => {
            reject_unused(verb, unused_options)?;
            (Operation::Status { receipt_request_id }, false)
        }
        "targets" => {
            reject_unused(verb, unused_options)?;
            (Operation::Targets, false)
        }
        "create" => {
            if !positionals.is_empty()
                || expected_version.is_some()
                || check
                || limit.is_some()
                || receipt_request_id.is_some()
            {
                return Err(CliError("schedule create requires options only and does not accept --check, --limit or --expect-version".into()));
            }
            let target_id = remote_pane.ok_or_else(|| {
                CliError(
                    "schedule create requires --remote-pane TARGET_TOKEN from `schedule targets`"
                        .into(),
                )
            })?;
            let deadline = deadline(at, after)?;
            let message = message.ok_or_else(|| {
                CliError("schedule create requires exactly one of --message TEXT or --stdin".into())
            })?;
            (
                Operation::Create {
                    target_id,
                    deadline,
                    message,
                    submit,
                },
                true,
            )
        }
        "list" => {
            reject_unused(
                verb,
                UnusedScheduleOptions {
                    limit: false,
                    ..unused_options
                },
            )?;
            (Operation::List { limit }, false)
        }
        "show" => {
            reject_unused(
                verb,
                UnusedScheduleOptions {
                    positionals: &[],
                    ..unused_options
                },
            )?;
            (
                Operation::Show {
                    job_id: one_task(&positionals)?,
                },
                false,
            )
        }
        "edit" => {
            if remote_pane.is_some()
                || check
                || submit
                || limit.is_some()
                || receipt_request_id.is_some()
            {
                return Err(CliError(
                    "schedule edit does not accept target selection, --check, --submit or --limit"
                        .into(),
                ));
            }
            let job_id = one_task(&positionals)?;
            let expected_version = expected_version
                .ok_or_else(|| CliError("schedule edit requires --expect-version N".into()))?;
            if at.is_none() && after.is_none() && message.is_none() {
                return Err(CliError(
                    "schedule edit requires a nonempty --message/--stdin or --at/--in patch".into(),
                ));
            }
            let deadline = if at.is_some() || after.is_some() {
                Some(deadline(at, after)?)
            } else {
                None
            };
            (
                Operation::Edit {
                    job_id,
                    expected_version,
                    deadline,
                    message,
                },
                true,
            )
        }
        "cancel" => {
            if remote_pane.is_some()
                || at.is_some()
                || after.is_some()
                || message.is_some()
                || submit
                || check
                || limit.is_some()
                || receipt_request_id.is_some()
            {
                return Err(CliError("schedule cancel accepts only TASK_UUID, --surface, --expect-version and --request-id".into()));
            }
            let job_id = one_task(&positionals)?;
            let expected_version = expected_version
                .ok_or_else(|| CliError("schedule cancel requires --expect-version N".into()))?;
            (
                Operation::Cancel {
                    job_id,
                    expected_version,
                },
                true,
            )
        }
        "history" => {
            reject_unused(
                verb,
                UnusedScheduleOptions {
                    limit: false,
                    ..unused_options
                },
            )?;
            (Operation::History { limit }, false)
        }
        _ => unreachable!("verb validated above"),
    };
    if !mutation && request_id.is_some() {
        return Err(CliError(
            "--request-id is only available on schedule mutations; status uses --request-id to select a receipt"
                .into(),
        ));
    }
    let request_id = if mutation {
        Some(request_id.unwrap_or_else(Uuid::new_v4))
    } else {
        None
    };
    let invocation = Invocation {
        surface,
        relative,
        request_id,
        operation,
    };
    invocation.validate()?;
    Ok(Parsed::Schedule(invocation))
}

fn reject_unused(verb: &str, options: UnusedScheduleOptions<'_>) -> Result<(), CliError> {
    if !options.positionals.is_empty()
        || options.remote_pane
        || options.at
        || options.after
        || options.message
        || options.submit
        || options.check
        || options.expected_version
        || options.limit
        || options.request_id
    {
        return Err(CliError(format!(
            "schedule {verb} received an option that does not apply to it"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct UnusedScheduleOptions<'a> {
    positionals: &'a [String],
    remote_pane: bool,
    at: bool,
    after: bool,
    message: bool,
    submit: bool,
    check: bool,
    expected_version: bool,
    limit: bool,
    request_id: bool,
}

fn one_task(positionals: &[String]) -> Result<Uuid, CliError> {
    if positionals.len() != 1 {
        return Err(CliError(
            "schedule show/edit/cancel requires one TASK_UUID".into(),
        ));
    }
    uuid(&positionals[0], "task ID")
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RequestState {
    Pending,
    OutcomeUnknown,
    Resolved,
    Rejected,
}

#[derive(Clone, Deserialize, Serialize)]
struct LocalRequest {
    request_id: Uuid,
    payload_sha256: String,
    surface: Uuid,
    operation: String,
    state: RequestState,
    updated_at_unix_ms: u128,
}

#[derive(Deserialize, Serialize)]
struct RequestJournal {
    version: u32,
    requests: Vec<LocalRequest>,
}

impl Default for RequestJournal {
    fn default() -> Self {
        Self {
            version: JOURNAL_VERSION,
            requests: Vec::new(),
        }
    }
}

struct JournalStore {
    directory: PathBuf,
    _lock: Flock<File>,
    journal: RequestJournal,
}

impl JournalStore {
    fn open(timeout: Duration) -> Result<Self, CliError> {
        let directory = request_directory(true)?;
        let lock_path = directory.join(LOCK_FILE);
        let mut file = open_private_file(&lock_path, true, 0)?;
        let deadline = Instant::now() + timeout;
        let lock = loop {
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(lock) => break lock,
                Err((returned, nix::errno::Errno::EWOULDBLOCK)) if Instant::now() < deadline => {
                    file = returned;
                    thread::sleep(Duration::from_millis(5));
                }
                Err((_, error)) => {
                    return Err(CliError(format!(
                        "schedule request journal lock failed: {error}"
                    )));
                }
            }
        };
        let named = fs::symlink_metadata(&lock_path)?;
        let opened = lock.metadata()?;
        if !named.is_file()
            || named.uid() != geteuid().as_raw()
            || named.nlink() != 1
            || named.dev() != opened.dev()
            || named.ino() != opened.ino()
        {
            return Err(CliError(
                "schedule request journal lock identity changed".into(),
            ));
        }
        let journal_path = directory.join(JOURNAL_FILE);
        let journal = match fs::symlink_metadata(&journal_path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => RequestJournal::default(),
            Err(error) => return Err(error.into()),
            Ok(_) => {
                let file = open_private_file(&journal_path, false, JOURNAL_BYTES)?;
                let mut bytes = Vec::new();
                file.take(JOURNAL_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > JOURNAL_BYTES {
                    return Err(CliError(
                        "schedule request journal exceeds its size limit".into(),
                    ));
                }
                let journal: RequestJournal = serde_json::from_slice(&bytes).map_err(|_| {
                    CliError("invalid schedule request journal; existing file retained".into())
                })?;
                validate_journal(&journal)?;
                journal
            }
        };
        Ok(Self {
            directory,
            _lock: lock,
            journal,
        })
    }

    fn record_pending(&mut self, mut request: LocalRequest) -> Result<(), CliError> {
        if let Some(existing) = self
            .journal
            .requests
            .iter_mut()
            .find(|entry| entry.request_id == request.request_id)
        {
            if existing.payload_sha256 != request.payload_sha256
                || existing.surface != request.surface
                || existing.operation != request.operation
            {
                return Err(CliError(
                    "request UUID is already recorded with a different payload; the message is not shown".into(),
                ));
            }
            existing.state = RequestState::Pending;
            existing.updated_at_unix_ms = now_unix_ms();
        } else {
            request.state = RequestState::Pending;
            request.updated_at_unix_ms = now_unix_ms();
            self.journal.requests.push(request);
        }
        self.prune()?;
        self.save()
    }

    fn mark(&mut self, request_id: Uuid, state: RequestState) -> Result<(), CliError> {
        if let Some(entry) = self
            .journal
            .requests
            .iter_mut()
            .find(|entry| entry.request_id == request_id)
        {
            entry.state = state;
            entry.updated_at_unix_ms = now_unix_ms();
        }
        self.save()
    }

    fn unresolved(&self, surface: Uuid) -> Vec<Value> {
        let mut rows = self
            .journal
            .requests
            .iter()
            .filter(|entry| {
                entry.surface == surface
                    && matches!(
                        entry.state,
                        RequestState::Pending | RequestState::OutcomeUnknown
                    )
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_unix_ms));
        rows.into_iter()
            .map(|entry| {
                json!({
                    "request_id": entry.request_id,
                    "operation": entry.operation,
                    "state": entry.state,
                    "updated_at_unix_ms": entry.updated_at_unix_ms,
                })
            })
            .collect()
    }

    fn prune(&mut self) -> Result<(), CliError> {
        while self.journal.requests.len() > JOURNAL_LIMIT {
            let removable = self
                .journal
                .requests
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    matches!(entry.state, RequestState::Resolved | RequestState::Rejected)
                })
                .min_by_key(|(_, entry)| entry.updated_at_unix_ms)
                .map(|(index, _)| index);
            if let Some(index) = removable {
                self.journal.requests.remove(index);
            } else {
                return Err(CliError(
                    "all local schedule request slots are unresolved; run schedule status before adding another mutation".into(),
                ));
            }
        }
        Ok(())
    }

    fn save(&self) -> Result<(), CliError> {
        validate_journal(&self.journal)?;
        let bytes = serde_json::to_vec(&self.journal)
            .map_err(|_| CliError("encode schedule request journal".into()))?;
        if bytes.len() > JOURNAL_BYTES {
            return Err(CliError(
                "schedule request journal exceeds its size limit".into(),
            ));
        }
        let temporary = self.directory.join(format!(
            ".requests-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&temporary)?;
        let result = (|| -> Result<(), CliError> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, self.directory.join(JOURNAL_FILE))?;
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn validate_journal(journal: &RequestJournal) -> Result<(), CliError> {
    if journal.version != JOURNAL_VERSION || journal.requests.len() > JOURNAL_LIMIT {
        return Err(CliError(
            "unsupported or oversized schedule request journal".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for entry in &journal.requests {
        if entry.request_id.is_nil()
            || entry.surface.is_nil()
            || entry.payload_sha256.len() != 64
            || !entry
                .payload_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || entry.operation.is_empty()
            || !ids.insert(entry.request_id)
        {
            return Err(CliError("invalid schedule request journal entry".into()));
        }
    }
    Ok(())
}

fn request_directory(create: bool) -> Result<PathBuf, CliError> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            CliError("HOME must be absolute for the private schedule request journal".into())
        })?;
    let parent = home.join(".config/zmux");
    if create {
        fs::create_dir_all(&parent)?;
    }
    let directory = parent.join(JOURNAL_DIRECTORY);
    if create {
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid() != geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(CliError(
            "schedule request journal directory must be private and owned by this user".into(),
        ));
    }
    Ok(directory)
}

fn open_private_file(path: &Path, create: bool, max_bytes: usize) -> Result<File, CliError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
        || metadata.len() > max_bytes as u64
    {
        return Err(CliError(
            "schedule request journal file must be bounded, private and owned by this user".into(),
        ));
    }
    Ok(file)
}

fn request_hash(invocation: &Invocation) -> Result<String, CliError> {
    let bytes = serde_json::to_vec(&json!({
        "surface": invocation.surface,
        "operation": invocation.operation,
    }))
    .map_err(|_| CliError("encode schedule request fingerprint".into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportError {
    code: String,
    message: String,
    #[serde(default)]
    request_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleExecution {
    ok: bool,
    #[serde(default)]
    reply: Option<SchedulerResponse>,
    #[serde(default)]
    error: Option<TransportError>,
    #[serde(default)]
    outcome_unknown: bool,
}

pub(super) fn run(options: &ClientOptions, mut invocation: Invocation) -> Result<Value, CliError> {
    invocation.resolve_stdin()?;
    let request = SchedulerRequest {
        protocol_version: SCHEDULER_PROTOCOL_VERSION,
        request_id: invocation.request_id,
        action: invocation.action(),
    };
    request
        .validate()
        .map_err(|error| CliError(error.to_string()))?;

    let mutation_id = invocation.request_id;
    let journaled_id = invocation
        .has_durable_receipt()
        .then_some(mutation_id)
        .flatten();
    let mut journal = if let Some(request_id) = journaled_id {
        let mut journal = JournalStore::open(options.timeout)?;
        journal.record_pending(LocalRequest {
            request_id,
            payload_sha256: request_hash(&invocation)?,
            surface: invocation.surface,
            operation: operation_name(&invocation.operation).to_owned(),
            state: RequestState::Pending,
            updated_at_unix_ms: now_unix_ms(),
        })?;
        Some(journal)
    } else {
        None
    };
    if let Some(request_id) = mutation_id {
        // Task mutation IDs are durable in the private journal before they are
        // shown. Setup install has no receipt and is reconciled with setup --check.
        eprintln!("schedule request_id={request_id}");
    }

    let local_unresolved = if matches!(
        &invocation.operation,
        Operation::Status {
            receipt_request_id: None
        }
    ) {
        let store = match journal.take() {
            Some(store) => store,
            None => JournalStore::open(options.timeout)?,
        };
        let unresolved = store.unresolved(invocation.surface);
        drop(store);
        unresolved
    } else {
        Vec::new()
    };
    drop(journal);

    let control = match super::request_with_phase(
        options,
        ControlCommand::Schedule {
            surface: invocation.surface,
            request,
        },
    ) {
        Ok(response) => response,
        Err(error) => {
            let outcome_unknown = mutation_id.is_some() && error.may_have_sent;
            if let Some(request_id) = journaled_id {
                mark_request(
                    request_id,
                    outcome_unknown_state(outcome_unknown),
                    options.timeout,
                );
            }
            report_uncertain_recovery(&invocation, outcome_unknown);
            return Ok(error_result(
                mutation_id,
                "local_transport",
                sanitize_error_message(&error.to_string(), &invocation),
                outcome_unknown,
                None,
                local_unresolved,
            ));
        }
    };

    if !control.ok {
        let error = control.error;
        let code = error
            .as_ref()
            .map(|error| error.code.as_str())
            .unwrap_or("control_error");
        let message = error
            .as_ref()
            .map(|error| sanitize_error_message(&error.message, &invocation))
            .unwrap_or_else(|| "local control rejected the schedule request".into());
        if let Some(request_id) = journaled_id {
            mark_request(request_id, RequestState::Rejected, options.timeout);
        }
        return Ok(error_result(
            mutation_id,
            code,
            message,
            false,
            None,
            local_unresolved,
        ));
    }

    let Some(result) = control.result else {
        if let Some(request_id) = journaled_id {
            mark_request(request_id, RequestState::OutcomeUnknown, options.timeout);
        }
        report_uncertain_recovery(&invocation, mutation_id.is_some());
        return Ok(error_result(
            mutation_id,
            "invalid_schedule_response",
            "local control returned no schedule execution result".into(),
            mutation_id.is_some(),
            None,
            local_unresolved,
        ));
    };
    let execution: ScheduleExecution = match serde_json::from_value(result) {
        Ok(execution) => execution,
        Err(_) => {
            if let Some(request_id) = journaled_id {
                mark_request(request_id, RequestState::OutcomeUnknown, options.timeout);
            }
            report_uncertain_recovery(&invocation, mutation_id.is_some());
            return Ok(error_result(
                mutation_id,
                "invalid_schedule_response",
                "local control returned an invalid schedule execution result".into(),
                mutation_id.is_some(),
                None,
                local_unresolved,
            ));
        }
    };

    if !execution.ok {
        let code = execution
            .error
            .as_ref()
            .map(|error| error.code.as_str())
            .unwrap_or("schedule_transport_error");
        let message = execution
            .error
            .as_ref()
            .map(|error| sanitize_error_message(&error.message, &invocation))
            .unwrap_or_else(|| "the remote scheduler did not return a confirmed response".into());
        if let Some(request_id) = journaled_id {
            mark_request(
                request_id,
                outcome_unknown_state(execution.outcome_unknown),
                options.timeout,
            );
        }
        report_uncertain_recovery(&invocation, execution.outcome_unknown);
        return Ok(error_result(
            mutation_id,
            code,
            message,
            execution.outcome_unknown,
            execution.error.as_ref().and_then(|error| error.request_id),
            local_unresolved,
        ));
    }

    let Some(response) = execution.reply else {
        if let Some(request_id) = journaled_id {
            mark_request(request_id, RequestState::OutcomeUnknown, options.timeout);
        }
        report_uncertain_recovery(&invocation, mutation_id.is_some());
        return Ok(error_result(
            mutation_id,
            "invalid_schedule_response",
            "remote scheduler returned no protocol response".into(),
            mutation_id.is_some(),
            None,
            local_unresolved,
        ));
    };

    if response.protocol_version != SCHEDULER_PROTOCOL_VERSION || response.request_id != mutation_id
    {
        if let Some(request_id) = journaled_id {
            mark_request(request_id, RequestState::OutcomeUnknown, options.timeout);
        }
        report_uncertain_recovery(&invocation, mutation_id.is_some());
        return Ok(error_result(
            mutation_id,
            "invalid_schedule_response",
            "remote scheduler response did not match the request".into(),
            mutation_id.is_some(),
            None,
            local_unresolved,
        ));
    }

    if let Some(request_id) = journaled_id {
        let state = match &response.result {
            SchedulerResult::Success(reply) => {
                receipt_state(reply).unwrap_or(RequestState::Resolved)
            }
            SchedulerResult::Error(error) if error.outcome_unknown => RequestState::OutcomeUnknown,
            SchedulerResult::Error(_) => RequestState::Rejected,
        };
        mark_request(request_id, state, options.timeout);
    }
    if let Operation::Status {
        receipt_request_id: Some(receipt_request_id),
    } = &invocation.operation
        && let SchedulerResult::Success(SchedulerReply::RequestStatus { receipt }) =
            &response.result
        && receipt.request_id == *receipt_request_id
    {
        let state = match receipt.state {
            ReceiptState::Applied | ReceiptState::Rejected => RequestState::Resolved,
            ReceiptState::OutcomeUnknown => RequestState::OutcomeUnknown,
        };
        mark_request(*receipt_request_id, state, options.timeout);
    }

    if let SchedulerResult::Error(error) = &response.result {
        report_uncertain_recovery(&invocation, error.outcome_unknown);
    }

    let mut response_json =
        serde_json::to_value(&response).map_err(|_| CliError("encode schedule response".into()))?;
    if matches!(&response.result, SchedulerResult::Error(_))
        && let Some(message) = response_json
            .pointer_mut("/result/data/message")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    {
        response_json["result"]["data"]["message"] =
            Value::String(sanitize_error_message(&message, &invocation));
    }
    let success = matches!(&response.result, SchedulerResult::Success(_));
    let mut output = json!({
        "ok": success,
        "request_id": mutation_id,
        "response": response_json,
    });
    if matches!(
        &invocation.operation,
        Operation::Status {
            receipt_request_id: None
        }
    ) {
        output["local_unresolved_requests"] = json!(local_unresolved);
    }
    Ok(output)
}

fn operation_name(operation: &Operation) -> &'static str {
    match operation {
        Operation::SetupInstall => "setup_install",
        Operation::SetupCheck => "setup_check",
        Operation::Status { .. } => "status",
        Operation::Targets => "targets",
        Operation::Create { .. } => "create",
        Operation::List { .. } => "list",
        Operation::Show { .. } => "show",
        Operation::Edit { .. } => "edit",
        Operation::Cancel { .. } => "cancel",
        Operation::History { .. } => "history",
    }
}

fn receipt_state(reply: &SchedulerReply) -> Option<RequestState> {
    match reply {
        SchedulerReply::Mutation { receipt, .. } | SchedulerReply::RequestStatus { receipt } => {
            Some(match receipt.state {
                ReceiptState::Applied => RequestState::Resolved,
                ReceiptState::Rejected => RequestState::Rejected,
                ReceiptState::OutcomeUnknown => RequestState::OutcomeUnknown,
            })
        }
        _ => None,
    }
}

fn outcome_unknown_state(outcome_unknown: bool) -> RequestState {
    if outcome_unknown {
        RequestState::OutcomeUnknown
    } else {
        RequestState::Rejected
    }
}

fn report_uncertain_recovery(invocation: &Invocation, outcome_unknown: bool) {
    if outcome_unknown && matches!(&invocation.operation, Operation::SetupInstall) {
        eprintln!(
            "schedule setup outcome is uncertain; check the installed service with `zmux schedule setup --surface {} --check` (setup has no durable request receipt)",
            invocation.surface
        );
    }
}

fn mark_request(request_id: Uuid, state: RequestState, timeout: Duration) {
    match JournalStore::open(timeout).and_then(|mut journal| journal.mark(request_id, state)) {
        Ok(()) => (),
        Err(error) => eprintln!(
            "zmux: schedule request journal update failed for {request_id}: {error}; remote outcome is unchanged"
        ),
    }
}

fn sanitize_error_message(message: &str, invocation: &Invocation) -> String {
    let mut safe = message.to_owned();
    for body in invocation.message_bodies() {
        if !body.is_empty() && safe.contains(body) {
            safe = safe.replace(body, "[message text omitted]");
        }
    }
    safe.chars().take(512).collect()
}

impl Invocation {
    fn message_bodies(&self) -> impl Iterator<Item = &str> {
        let source = match &self.operation {
            Operation::Create { message, .. } => Some(message),
            Operation::Edit { message, .. } => message.as_ref(),
            _ => None,
        };
        source.into_iter().filter_map(MessageSource::text)
    }
}

fn error_result(
    request_id: Option<Uuid>,
    code: &str,
    message: String,
    outcome_unknown: bool,
    transport_request_id: Option<Uuid>,
    local_unresolved: Vec<Value>,
) -> Value {
    let mut output = json!({
        "ok": false,
        "request_id": request_id,
        "error": {"code": code, "message": message},
        "outcome_unknown": outcome_unknown,
    });
    if let Some(transport_request_id) = transport_request_id {
        output["transport_request_id"] = json!(transport_request_id);
    }
    if !local_unresolved.is_empty() {
        output["local_unresolved_requests"] = json!(local_unresolved);
    }
    output
}

/// Snapshot presentation only; the helper remains authoritative for job state.
pub(super) fn relative_list(result: &Value) -> Result<String, CliError> {
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};
    let jobs = result
        .pointer("/response/result/data/jobs")
        .and_then(Value::as_array)
        .ok_or_else(|| CliError("schedule list response has no jobs".into()))?;
    let now = OffsetDateTime::now_utc();
    let mut output = String::from(
        "TASK UUID                             VERSION  WHEN                  STATE\n",
    );
    if jobs.is_empty() {
        output.push_str("No pending messages.\n");
    }
    for job in jobs {
        let deadline = job["deadline_utc"]
            .as_str()
            .ok_or_else(|| CliError("job has no deadline".into()))?;
        let date = OffsetDateTime::parse(deadline, &Rfc3339)
            .map_err(|_| CliError("job has an invalid deadline".into()))?;
        let delta = (date - now).whole_seconds();
        let seconds = delta.unsigned_abs();
        let clock = format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        );
        let when = match delta.cmp(&0) {
            std::cmp::Ordering::Greater => format!("in {clock}"),
            std::cmp::Ordering::Equal => "due now".into(),
            std::cmp::Ordering::Less => format!("{clock} overdue"),
        };
        let id = job["id"]
            .as_str()
            .ok_or_else(|| CliError("job has no ID".into()))?;
        let version = job["version"]
            .as_u64()
            .ok_or_else(|| CliError("job has no version".into()))?;
        let state = job["state"]
            .as_str()
            .ok_or_else(|| CliError("job has no state".into()))?;
        output.push_str(&format!("{id}  {version:<7}  {when:<20}  {state}\n"));
    }
    output.push_str(
        "Times are a snapshot based on this computer's clock; remote state is authoritative.\n",
    );
    Ok(output)
}
