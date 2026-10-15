//! In-process workspace metadata. All access runs on the desktop action executor.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{Snapshot, WorkspaceId};

pub const MAX_STATUS_ENTRIES: usize = 64;
pub const MAX_LOG_ENTRIES: usize = 256;
pub const MAX_TEXT_BYTES: usize = 2048;
pub const MAX_NAME_BYTES: usize = 128;
/// Leaves room for all status entries in one bounded control response, even
/// when JSON escaping doubles every retained message byte.
pub const MAX_LOG_MESSAGE_BYTES: usize = 1024;
/// Process-wide conservative JSON size budget, including all workspace rows.
pub const MAX_RETAINED_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MetadataAction {
    SetPullRequest {
        pull_request: PullRequest,
    },
    ClearPullRequest {},
    SetStatus {
        key: String,
        value: String,
        #[serde(default)]
        icon: Option<String>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        priority: i32,
    },
    ClearStatus {
        key: String,
    },
    ListStatus,
    SetProgress {
        value: f64,
        #[serde(default)]
        label: Option<String>,
    },
    ClearProgress,
    Log {
        message: String,
        #[serde(default)]
        level: LogLevel,
        #[serde(default)]
        source: Option<String>,
    },
    ClearLog,
    ListLog {
        #[serde(default)]
        limit: Option<usize>,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    #[default]
    Info,
    Progress,
    Success,
    Warning,
    Error,
}
#[derive(Clone, Debug, Serialize)]
pub struct StatusEntry {
    pub key: String,
    pub value: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub priority: i32,
}
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub value: f64,
    pub label: Option<String>,
}
/// A manual link, not automatically fetched PR state. No network or process work
/// runs on this metadata owner. The CLI verifies repository provenance first.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}
impl PullRequest {
    pub fn parse(url: &str) -> Result<Self, MetadataError> {
        let invalid = || {
            MetadataError::Invalid(
                "PR must use https://github.com/OWNER/REPO/pull/NUMBER with a canonical positive number",
            )
        };
        if url.len() > 256 {
            return Err(invalid());
        }
        let path = url
            .strip_prefix("https://github.com/")
            .ok_or_else(invalid)?;
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() != 4
            || parts[2] != "pull"
            || !valid_repository(&format!("{}/{}", parts[0], parts[1]))
        {
            return Err(invalid());
        }
        let number = positive_pr_number(parts[3]).ok_or_else(invalid)?;
        Ok(Self {
            number,
            url: url.into(),
        })
    }
    pub fn repository(&self) -> &str {
        // Only validated records are retained; parsing never normalizes paths.
        self.url
            .strip_prefix("https://github.com/")
            .and_then(|path| path.rsplit_once("/pull/").map(|(repository, _)| repository))
            .unwrap_or("")
    }
    fn validate(&self) -> Result<(), MetadataError> {
        if Self::parse(&self.url)?.number != self.number {
            return Err(MetadataError::Invalid(
                "PR number does not match its canonical URL",
            ));
        }
        Ok(())
    }
}
pub fn positive_pr_number(value: &str) -> Option<u64> {
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}
pub fn valid_repository(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && owner.len() <= 39
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && owner.as_bytes()[0].is_ascii_alphanumeric()
        && owner.as_bytes()[owner.len() - 1].is_ascii_alphanumeric()
        && !repo.is_empty()
        && repo.len() <= 100
        && !matches!(repo, "." | "..")
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub sequence: u64,
    pub message: String,
    pub level: LogLevel,
    pub source: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceMetadata {
    pub workspace_id: WorkspaceId,
    pub workspace_uuid: Uuid,
    pub pull_request: Option<PullRequest>,
    pub statuses: Vec<StatusEntry>,
    pub progress: Option<Progress>,
    /// Oldest to newest. A list limit retains the newest N in this same order.
    pub logs: Vec<LogEntry>,
    pub logs_dropped: u64,
}
impl WorkspaceMetadata {
    // All admitted text requires at most two JSON bytes per UTF-8 byte. The
    // fixed framing allowances include IDs, counters, enum strings and numbers.
    fn encoded_bound(&self) -> usize {
        192 + self
            .pull_request
            .as_ref()
            .map_or(0, |pr| 128 + 2 * pr.url.len())
            + self
                .statuses
                .iter()
                .map(|s| {
                    128 + 2
                        * (s.key.len()
                            + s.value.len()
                            + s.icon.as_ref().map_or(0, String::len)
                            + s.color.as_ref().map_or(0, String::len))
                })
                .sum::<usize>()
            + self
                .progress
                .as_ref()
                .map_or(0, |p| 64 + 2 * p.label.as_ref().map_or(0, String::len))
            + self
                .logs
                .iter()
                .map(|l| 128 + 2 * (l.message.len() + l.source.as_ref().map_or(0, String::len)))
                .sum::<usize>()
    }
    fn is_empty(&self) -> bool {
        self.pull_request.is_none()
            && self.statuses.is_empty()
            && self.progress.is_none()
            && self.logs.is_empty()
    }
    fn empty(workspace_id: WorkspaceId, workspace_uuid: Uuid) -> Self {
        Self {
            workspace_id,
            workspace_uuid,
            pull_request: None,
            statuses: Vec::new(),
            progress: None,
            logs: Vec::new(),
            logs_dropped: 0,
        }
    }
}
#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("workspace status capacity reached; clear an entry before adding a new key")]
    StatusCapacity,
    #[error("process-wide metadata byte capacity reached; clear metadata before adding more data")]
    ByteCapacity,
}
impl MetadataError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_metadata",
            Self::StatusCapacity | Self::ByteCapacity => "metadata_capacity",
        }
    }
}
impl MetadataAction {
    /// The owner validates wire requests too; CLI validation is only early feedback.
    pub fn validate(&self) -> Result<(), MetadataError> {
        match self {
            Self::SetPullRequest { pull_request } => pull_request.validate()?,
            Self::SetStatus {
                key,
                value,
                icon,
                color,
                ..
            } => {
                name(key)?;
                text(value)?;
                if let Some(icon) = icon {
                    name(icon)?;
                }
                if let Some(color) = color
                    && !(color.len() == 7
                        && color.starts_with('#')
                        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
                {
                    return Err(MetadataError::Invalid("color must be #RRGGBB"));
                }
            }
            Self::ClearStatus { key } => name(key)?,
            Self::SetProgress { value, label } => {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    return Err(MetadataError::Invalid(
                        "progress must be a finite number from 0 to 1",
                    ));
                }
                if let Some(label) = label {
                    text(label)?;
                }
            }
            Self::Log {
                message, source, ..
            } => {
                text(message)?;
                if message.len() > MAX_LOG_MESSAGE_BYTES {
                    return Err(MetadataError::Invalid(
                        "log messages must contain at most 1024 bytes",
                    ));
                }
                if message.is_empty() {
                    return Err(MetadataError::Invalid("log message must not be empty"));
                }
                if let Some(source) = source {
                    name(source)?;
                }
            }
            Self::ListLog { limit: Some(limit) } if !(1..=MAX_LOG_ENTRIES).contains(limit) => {
                return Err(MetadataError::Invalid("log limit must be from 1 to 256"));
            }
            _ => (),
        }
        Ok(())
    }
}
fn name(value: &str) -> Result<(), MetadataError> {
    if value.is_empty() || value.len() > MAX_NAME_BYTES || value.chars().any(char::is_control) {
        return Err(MetadataError::Invalid(
            "metadata names must contain 1 to 128 bytes without control characters",
        ));
    }
    Ok(())
}
fn text(value: &str) -> Result<(), MetadataError> {
    if value.len() > MAX_TEXT_BYTES
        || value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(MetadataError::Invalid(
            "metadata text must contain at most 2048 bytes and no controls except newline/tab",
        ));
    }
    Ok(())
}
#[derive(Default)]
pub(crate) struct MetadataStore {
    namespace: Option<Uuid>,
    workspaces: BTreeMap<u64, WorkspaceMetadata>,
    sequence: u64,
    retained_bytes: usize,
}
impl MetadataStore {
    pub fn reconcile(&mut self, model: &Snapshot) {
        if self.namespace != Some(model.identity_namespace) {
            self.workspaces.clear();
            self.sequence = 0;
            self.retained_bytes = 0;
            self.namespace = Some(model.identity_namespace);
        }
        self.workspaces
            .retain(|id, _| model.workspaces.iter().any(|w| w.id.0 == *id));
        self.retained_bytes = self
            .workspaces
            .values()
            .map(WorkspaceMetadata::encoded_bound)
            .sum();
    }
    pub fn snapshot(&self) -> Vec<WorkspaceMetadata> {
        self.workspaces.values().cloned().collect()
    }
    pub fn apply(
        &mut self,
        workspace: WorkspaceId,
        workspace_uuid: Uuid,
        action: MetadataAction,
    ) -> Result<WorkspaceMetadata, MetadataError> {
        action.validate()?;
        let read = matches!(
            action,
            MetadataAction::ListStatus | MetadataAction::ListLog { .. }
        );
        let limit = if let MetadataAction::ListLog { limit } = &action {
            *limit
        } else {
            None
        };
        let mut state = self
            .workspaces
            .get(&workspace.0)
            .cloned()
            .unwrap_or_else(|| WorkspaceMetadata::empty(workspace, workspace_uuid));
        let mut sequence = self.sequence;
        match action {
            MetadataAction::SetPullRequest { pull_request } => {
                state.pull_request = Some(pull_request)
            }
            MetadataAction::ClearPullRequest {} => state.pull_request = None,
            MetadataAction::SetStatus {
                key,
                value,
                icon,
                color,
                priority,
            } => {
                let existing = state.statuses.iter().position(|s| s.key == key);
                if existing.is_none() && state.statuses.len() >= MAX_STATUS_ENTRIES {
                    return Err(MetadataError::StatusCapacity);
                }
                let entry = StatusEntry {
                    key,
                    value,
                    icon,
                    color,
                    priority,
                };
                if let Some(i) = existing {
                    state.statuses[i] = entry;
                } else {
                    state.statuses.push(entry);
                }
                state
                    .statuses
                    .sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.key.cmp(&b.key)));
            }
            MetadataAction::ClearStatus { key } => state.statuses.retain(|s| s.key != key),
            MetadataAction::SetProgress { value, label } => {
                state.progress = Some(Progress { value, label })
            }
            MetadataAction::ClearProgress => state.progress = None,
            MetadataAction::Log {
                message,
                level,
                source,
            } => {
                sequence = sequence.saturating_add(1);
                if state.logs.len() == MAX_LOG_ENTRIES {
                    state.logs.remove(0);
                    state.logs_dropped = state.logs_dropped.saturating_add(1);
                }
                state.logs.push(LogEntry {
                    sequence,
                    message,
                    level,
                    source,
                });
            }
            MetadataAction::ClearLog => {
                state.logs.clear();
                state.logs_dropped = 0;
            }
            MetadataAction::ListStatus | MetadataAction::ListLog { .. } => (),
        }
        if !read {
            let previous = self
                .workspaces
                .get(&workspace.0)
                .map_or(0, WorkspaceMetadata::encoded_bound);
            let candidate = if state.is_empty() {
                0
            } else {
                state.encoded_bound()
            };
            let retained_bytes = self.retained_bytes - previous + candidate;
            // Two bytes for the outer JSON array and one comma per workspace.
            if retained_bytes + self.workspaces.len() + 3 > MAX_RETAINED_BYTES {
                return Err(MetadataError::ByteCapacity);
            }
            self.retained_bytes = retained_bytes;
            self.sequence = sequence;
            if state.is_empty() {
                self.workspaces.remove(&workspace.0);
            } else {
                self.workspaces.insert(workspace.0, state.clone());
            }
        }
        if let Some(limit) = limit {
            let remove = state.logs.len().saturating_sub(limit);
            state.logs.drain(..remove);
        }
        Ok(state)
    }
}
