//! Bounded, local-only provider lifecycle evidence for native presentation.
//!
//! This owner records the latest typed hook-derived state for an exact terminal
//! session. It does not authenticate a remote provider, infer activity from
//! terminal output, or promise provider replay semantics. The control request is
//! authorized by the desktop's same-UID policy; it does not bind provider claims
//! to an OS process identity.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;
use zmux_session_runtime::State as SessionState;
use zmux_workspace_core::{
    PaneId, SessionId, Snapshot as Model, SurfaceId, WindowId, WorkspaceId, targets::TargetKind,
};

pub const MAX_RECORDS: usize = 64;
/// Hook events have no provider replay cursor or heartbeat. After this interval,
/// the published state becomes Unknown instead of assuming it is still current.
pub const EVIDENCE_FRESHNESS: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    ClaudeLocal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Unknown,
    Working,
    Waiting,
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStopState {
    Unknown,
    Working,
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connected,
    Disconnected,
    Unknown,
}

/// The stable desktop owner and current local session to which a provider hook is bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub identity_namespace: Uuid,
    pub surface_id: SurfaceId,
    pub surface_uuid: Uuid,
    pub session_id: SessionId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub request_id: Uuid,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Register {
        target: Target,
        provider: Provider,
        runtime_id: Uuid,
        generation: Uuid,
        provider_session_id: Uuid,
    },
    Event {
        capability: Uuid,
        runtime_id: Uuid,
        generation: Uuid,
        /// Locally assigned adapter order. This is not a provider replay cursor.
        sequence: u64,
        event: Event,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    PromptSubmitted { prompt_id: Uuid },
    WaitingUnconfirmed,
    NeedsUser,
    Resumed,
    TurnStopped { state: TurnStopState },
    SessionEnded,
    EvidenceLost,
}

/// Typed subset of Claude Code's documented local hook payloads used by the
/// explicit Zmux wrapper. Unknown provider fields are ignored for forward
/// compatibility; required lifecycle fields remain typed and validated.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum ClaudeHookEvent {
    #[serde(rename = "SessionStart")]
    SessionStart {
        session_id: String,
        source: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit {
        session_id: String,
        prompt_id: Uuid,
        permission_mode: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "PermissionRequest")]
    PermissionRequest {
        session_id: String,
        #[serde(default)]
        tool_name: Option<String>,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "Elicitation")]
    Elicitation {
        session_id: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "ElicitationResult")]
    ElicitationResult {
        session_id: String,
        action: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "Notification")]
    Notification {
        session_id: String,
        notification_type: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "PreToolUse")]
    PreToolUse {
        session_id: String,
        tool_name: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "PostToolUse")]
    PostToolUse {
        session_id: String,
        tool_name: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure {
        session_id: String,
        tool_name: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "Stop")]
    Stop {
        session_id: String,
        #[serde(default)]
        stop_hook_active: Option<bool>,
        #[serde(default)]
        background_tasks: Option<Vec<serde_json::Value>>,
        #[serde(default)]
        session_crons: Option<Vec<serde_json::Value>>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "StopFailure")]
    StopFailure {
        session_id: String,
        error: String,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
    #[serde(rename = "SessionEnd")]
    SessionEnd {
        session_id: String,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        agent_type: Option<String>,
    },
}

impl ClaudeHookEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::SessionStart { .. } => "SessionStart",
            Self::UserPromptSubmit { .. } => "UserPromptSubmit",
            Self::PermissionRequest { .. } => "PermissionRequest",
            Self::Elicitation { .. } => "Elicitation",
            Self::ElicitationResult { .. } => "ElicitationResult",
            Self::Notification { .. } => "Notification",
            Self::PreToolUse { .. } => "PreToolUse",
            Self::PostToolUse { .. } => "PostToolUse",
            Self::PostToolUseFailure { .. } => "PostToolUseFailure",
            Self::Stop { .. } => "Stop",
            Self::StopFailure { .. } => "StopFailure",
            Self::SessionEnd { .. } => "SessionEnd",
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::SessionStart { session_id, .. }
            | Self::UserPromptSubmit { session_id, .. }
            | Self::PermissionRequest { session_id, .. }
            | Self::Elicitation { session_id, .. }
            | Self::ElicitationResult { session_id, .. }
            | Self::Notification { session_id, .. }
            | Self::PreToolUse { session_id, .. }
            | Self::PostToolUse { session_id, .. }
            | Self::PostToolUseFailure { session_id, .. }
            | Self::Stop { session_id, .. }
            | Self::StopFailure { session_id, .. }
            | Self::SessionEnd { session_id, .. } => session_id,
        }
    }

    pub fn is_main_session(&self) -> bool {
        match self {
            Self::SessionStart {
                agent_id,
                agent_type,
                ..
            }
            | Self::UserPromptSubmit {
                agent_id,
                agent_type,
                ..
            }
            | Self::PermissionRequest {
                agent_id,
                agent_type,
                ..
            }
            | Self::Elicitation {
                agent_id,
                agent_type,
                ..
            }
            | Self::ElicitationResult {
                agent_id,
                agent_type,
                ..
            }
            | Self::Notification {
                agent_id,
                agent_type,
                ..
            }
            | Self::PreToolUse {
                agent_id,
                agent_type,
                ..
            }
            | Self::PostToolUse {
                agent_id,
                agent_type,
                ..
            }
            | Self::PostToolUseFailure {
                agent_id,
                agent_type,
                ..
            }
            | Self::Stop {
                agent_id,
                agent_type,
                ..
            }
            | Self::StopFailure {
                agent_id,
                agent_type,
                ..
            }
            | Self::SessionEnd {
                agent_id,
                agent_type,
                ..
            } => agent_id.is_none() && agent_type.is_none(),
        }
    }

    /// Normalize provider events without treating candidate prompts or a
    /// stopped response turn as user-visible waiting or task completion.
    pub fn lifecycle_event(&self) -> Option<Event> {
        match self {
            Self::UserPromptSubmit { prompt_id, .. } => Some(Event::PromptSubmitted {
                prompt_id: *prompt_id,
            }),
            Self::PermissionRequest { .. } | Self::Elicitation { .. } => {
                Some(Event::WaitingUnconfirmed)
            }
            Self::Notification {
                notification_type, ..
            } if matches!(
                notification_type.as_str(),
                "permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog"
            ) =>
            {
                Some(Event::NeedsUser)
            }
            Self::Notification {
                notification_type, ..
            } if notification_type == "idle_prompt" => Some(Event::TurnStopped {
                state: TurnStopState::Idle,
            }),
            Self::Notification {
                notification_type, ..
            } if matches!(
                notification_type.as_str(),
                "elicitation_complete" | "elicitation_response"
            ) =>
            {
                Some(Event::Resumed)
            }
            Self::Notification {
                notification_type, ..
            } if notification_type == "agent_needs_input" => Some(Event::WaitingUnconfirmed),
            Self::ElicitationResult { .. }
            | Self::PostToolUse { .. }
            | Self::PostToolUseFailure { .. } => Some(Event::Resumed),
            Self::PreToolUse { tool_name, .. } if tool_name == "AskUserQuestion" => {
                Some(Event::NeedsUser)
            }
            Self::PreToolUse { .. } => Some(Event::Resumed),
            Self::Stop {
                stop_hook_active: Some(true),
                ..
            } => Some(Event::Resumed),
            Self::Stop {
                background_tasks: Some(tasks),
                ..
            } if !tasks.is_empty() => Some(Event::TurnStopped {
                state: TurnStopState::Working,
            }),
            Self::Stop { .. } => Some(Event::TurnStopped {
                state: TurnStopState::Unknown,
            }),
            Self::StopFailure { .. } => Some(Event::TurnStopped {
                state: TurnStopState::Unknown,
            }),
            Self::SessionEnd { reason, .. }
                if matches!(reason.as_deref(), Some("logout" | "prompt_input_exit")) =>
            {
                Some(Event::SessionEnded)
            }
            Self::SessionEnd { .. } => Some(Event::EvidenceLost),
            // This notification belongs to a completed background session, not
            // the tracked main session bound to this record.
            Self::Notification {
                notification_type, ..
            } if notification_type == "agent_completed" => None,
            Self::SessionStart { .. } | Self::Notification { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub request_id: Uuid,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<Uuid>,
}

/// Snapshot data safe to expose to native UI. The capability never crosses this boundary.
#[derive(Clone, Debug, Serialize)]
pub struct Agent {
    pub provider: Provider,
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub workspace_id: WorkspaceId,
    pub workspace_uuid: Uuid,
    pub pane_id: PaneId,
    pub pane_uuid: Uuid,
    pub surface_id: SurfaceId,
    pub surface_uuid: Uuid,
    pub session_id: SessionId,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub provider_session_id: Uuid,
    pub prompt_id: Option<Uuid>,
    pub activity: Activity,
    pub connection: Connection,
}

struct Record {
    target: Target,
    provider: Provider,
    runtime_id: Uuid,
    generation: Uuid,
    provider_session_id: Uuid,
    capability: Uuid,
    prompt_id: Option<Uuid>,
    activity: Activity,
    connection: Connection,
    last_sequence: u64,
    last_event: Option<Event>,
    observed_at: Instant,
    terminal: bool,
}

#[derive(Default)]
pub struct Runtime {
    records: Vec<Record>,
}

impl Runtime {
    pub fn target_for_capability(&self, capability: Uuid) -> Option<Target> {
        self.records
            .iter()
            .find(|record| record.capability == capability)
            .map(|record| record.target)
    }

    pub fn apply(&mut self, request: Request, now: Instant) -> Result<Receipt, String> {
        if request.request_id.is_nil() {
            return Err("lifecycle request id cannot be nil".into());
        }
        match request.action {
            Action::Register {
                target,
                provider,
                runtime_id,
                generation,
                provider_session_id,
            } => {
                if target.identity_namespace.is_nil()
                    || target.surface_uuid.is_nil()
                    || target.surface_id.0 == 0
                    || target.session_id.0 == 0
                    || runtime_id.is_nil()
                    || generation.is_nil()
                    || provider_session_id.is_nil()
                {
                    return Err("lifecycle registration identity is incomplete".into());
                }

                if self.records.iter().any(|record| {
                    record.target == target
                        && record.provider == provider
                        && record.provider_session_id == provider_session_id
                        && record.connection == Connection::Disconnected
                }) {
                    return Err("provider session has ended and cannot be re-registered".into());
                }

                if self.records.iter().any(|record| {
                    record.target == target
                        && record.provider == provider
                        && record.provider_session_id == provider_session_id
                        && record.runtime_id == runtime_id
                        && record.generation == generation
                        && record.terminal
                }) {
                    return Err("lifecycle adapter generation is terminal".into());
                }

                if let Some(existing) = self.records.iter_mut().find(|record| {
                    record.target == target
                        && record.provider == provider
                        && record.provider_session_id == provider_session_id
                        && record.runtime_id == runtime_id
                        && record.generation == generation
                }) {
                    existing.observed_at = now;
                    existing.connection = Connection::Connected;
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        capability: Some(existing.capability),
                    });
                }

                // A new session or adapter generation replaces the previous
                // record for this provider on the exact terminal owner.
                self.records
                    .retain(|record| record.target != target || record.provider != provider);
                if self.records.len() >= MAX_RECORDS {
                    return Err("live agent lifecycle capacity is full".into());
                }
                let capability = Uuid::new_v4();
                self.records.push(Record {
                    target,
                    provider,
                    runtime_id,
                    generation,
                    provider_session_id,
                    capability,
                    prompt_id: None,
                    activity: Activity::Unknown,
                    connection: Connection::Connected,
                    last_sequence: 0,
                    last_event: None,
                    observed_at: now,
                    terminal: false,
                });
                Ok(Receipt {
                    request_id: request.request_id,
                    accepted: true,
                    capability: Some(capability),
                })
            }
            Action::Event {
                capability,
                runtime_id,
                generation,
                sequence,
                event,
            } => {
                if capability.is_nil()
                    || runtime_id.is_nil()
                    || generation.is_nil()
                    || sequence == 0
                {
                    return Err("lifecycle event identity is incomplete".into());
                }
                let Some(record) = self
                    .records
                    .iter_mut()
                    .find(|record| record.capability == capability)
                else {
                    return Err("lifecycle capability is stale or unknown".into());
                };
                if record.runtime_id != runtime_id || record.generation != generation {
                    return Err("lifecycle generation is stale".into());
                }
                if sequence == record.last_sequence && record.last_event.as_ref() == Some(&event) {
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        capability: Some(capability),
                    });
                }
                if sequence <= record.last_sequence {
                    return Err("lifecycle event sequence is stale".into());
                }
                if record.terminal || record.connection == Connection::Disconnected {
                    return Err("lifecycle capability is terminal or disconnected".into());
                }

                match &event {
                    Event::PromptSubmitted { prompt_id } => {
                        if prompt_id.is_nil() {
                            return Err("provider prompt id cannot be nil".into());
                        }
                        record.prompt_id = Some(*prompt_id);
                        record.activity = Activity::Working;
                    }
                    Event::WaitingUnconfirmed => {
                        if record.prompt_id.is_none() {
                            return Err("waiting evidence has no current provider run".into());
                        }
                        record.activity = Activity::Unknown;
                    }
                    Event::NeedsUser => {
                        if record.prompt_id.is_none() {
                            return Err("needs-user event has no current provider run".into());
                        }
                        record.activity = Activity::Waiting;
                    }
                    Event::Resumed => {
                        if record.prompt_id.is_none() {
                            return Err("resumption evidence has no current provider run".into());
                        }
                        record.activity = Activity::Working;
                    }
                    Event::TurnStopped { state } => {
                        if record.prompt_id.is_none() {
                            return Err("stopped-turn evidence has no current provider run".into());
                        }
                        record.activity = match state {
                            TurnStopState::Unknown => Activity::Unknown,
                            TurnStopState::Working => Activity::Working,
                            TurnStopState::Idle => Activity::Idle,
                        };
                    }
                    Event::SessionEnded => {
                        record.activity = Activity::Unknown;
                        record.connection = Connection::Disconnected;
                    }
                    Event::EvidenceLost => {
                        record.activity = Activity::Unknown;
                        record.connection = Connection::Unknown;
                    }
                }
                if !matches!(&event, Event::SessionEnded | Event::EvidenceLost) {
                    record.connection = Connection::Connected;
                } else {
                    record.terminal = true;
                }
                record.last_sequence = sequence;
                record.last_event = Some(event);
                record.observed_at = now;
                Ok(Receipt {
                    request_id: request.request_id,
                    accepted: true,
                    capability: Some(capability),
                })
            }
        }
    }

    /// Reconcile records against current exact local surfaces and cached PTY state.
    /// Missing/replaced surfaces are removed; exited or stale owners become explicit.
    pub fn reconcile(
        &mut self,
        model: &Model,
        session_states: &std::collections::HashMap<SessionId, Option<SessionState>>,
        now: Instant,
    ) {
        self.records.retain_mut(|record| {
            if owner(model, record.target).is_none() {
                return false;
            }
            match session_states
                .get(&record.target.session_id)
                .copied()
                .flatten()
            {
                Some(SessionState::Running) => {
                    if record.connection == Connection::Connected
                        && now.saturating_duration_since(record.observed_at) >= EVIDENCE_FRESHNESS
                    {
                        record.activity = Activity::Unknown;
                        record.connection = Connection::Unknown;
                    }
                }
                Some(SessionState::Exited | SessionState::Closed | SessionState::Failed) => {
                    record.activity = Activity::Unknown;
                    record.connection = Connection::Disconnected;
                }
                None => {
                    record.activity = Activity::Unknown;
                    if record.connection != Connection::Disconnected {
                        record.connection = Connection::Unknown;
                    }
                }
            }
            true
        });
    }

    /// Expire connection evidence from the worker loop even when no snapshot input changes.
    /// The caller uses the returned transition to publish one updated snapshot.
    pub fn poll_freshness(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for record in &mut self.records {
            if record.connection == Connection::Connected
                && now.saturating_duration_since(record.observed_at) >= EVIDENCE_FRESHNESS
            {
                record.activity = Activity::Unknown;
                record.connection = Connection::Unknown;
                changed = true;
            }
        }
        changed
    }

    pub fn snapshot(&self, model: &Model) -> Vec<Agent> {
        self.records
            .iter()
            .filter_map(|record| {
                let owner = owner(model, record.target)?;
                Some(Agent {
                    provider: record.provider,
                    identity_namespace: record.target.identity_namespace,
                    window_id: owner.window,
                    window_uuid: model.object_uuid(TargetKind::Window, owner.window.0),
                    workspace_id: owner.workspace,
                    workspace_uuid: model.object_uuid(TargetKind::Workspace, owner.workspace.0),
                    pane_id: owner.pane,
                    pane_uuid: model.object_uuid(TargetKind::Pane, owner.pane.0),
                    surface_id: owner.surface,
                    surface_uuid: record.target.surface_uuid,
                    session_id: record.target.session_id,
                    runtime_id: record.runtime_id,
                    generation: record.generation,
                    provider_session_id: record.provider_session_id,
                    prompt_id: record.prompt_id,
                    activity: record.activity,
                    connection: record.connection,
                })
            })
            .collect()
    }
}

struct Owner {
    window: WindowId,
    workspace: WorkspaceId,
    pane: PaneId,
    surface: SurfaceId,
}

fn owner(model: &Model, target: Target) -> Option<Owner> {
    if model.identity_namespace != target.identity_namespace {
        return None;
    }
    for window in &model.windows {
        for workspace_id in &window.workspace_ids {
            let workspace = model
                .workspaces
                .iter()
                .find(|item| item.id == *workspace_id)?;
            for pane in workspace.root.panes() {
                for surface in &pane.surfaces {
                    if surface.id != target.surface_id
                        || surface.kind != zmux_workspace_core::SurfaceKind::Terminal
                        || surface.remote.is_some()
                        || surface.local_tmux.is_some()
                        || surface.session_id != Some(target.session_id)
                        || model.object_uuid(TargetKind::Surface, surface.id.0)
                            != target.surface_uuid
                    {
                        continue;
                    }
                    return Some(Owner {
                        window: window.id,
                        workspace: workspace.id,
                        pane: pane.id,
                        surface: surface.id,
                    });
                }
            }
        }
    }
    None
}

pub fn has_local_owner(model: &Model, target: Target) -> bool {
    owner(model, target).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zmux_workspace_core::{Action as WorkspaceAction, WorkspaceStore};

    #[test]
    fn claude_hook_payloads_normalize_only_explicit_lifecycle_events() {
        let prompt_id = Uuid::new_v4();
        let fixtures = [
            (
                json!({"hook_event_name":"SessionStart","session_id":"session","source":"startup"}),
                "SessionStart",
                None,
            ),
            (
                json!({"hook_event_name":"UserPromptSubmit","session_id":"session","prompt_id":prompt_id,"permission_mode":"default","prompt":"work"}),
                "UserPromptSubmit",
                Some(Event::PromptSubmitted { prompt_id }),
            ),
            (
                json!({"hook_event_name":"PermissionRequest","session_id":"session","tool_name":"Bash","tool_use_id":"tool-1","tool_input":{}}),
                "PermissionRequest",
                Some(Event::WaitingUnconfirmed),
            ),
            (
                json!({"hook_event_name":"Elicitation","session_id":"session","message":"question"}),
                "Elicitation",
                Some(Event::WaitingUnconfirmed),
            ),
            (
                json!({"hook_event_name":"ElicitationResult","session_id":"session","action":"accept"}),
                "ElicitationResult",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"Notification","session_id":"session","notification_type":"permission_prompt","message":"Claude needs your permission"}),
                "Notification",
                Some(Event::NeedsUser),
            ),
            (
                json!({"hook_event_name":"Notification","session_id":"session","notification_type":"agent_needs_input","message":"A background session needs input"}),
                "Notification",
                Some(Event::WaitingUnconfirmed),
            ),
            (
                json!({"hook_event_name":"Notification","session_id":"session","notification_type":"idle_prompt","message":"Claude is waiting"}),
                "Notification",
                Some(Event::TurnStopped {
                    state: TurnStopState::Idle,
                }),
            ),
            (
                json!({"hook_event_name":"Notification","session_id":"session","notification_type":"elicitation_response","message":"Elicitation response sent"}),
                "Notification",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"Notification","session_id":"session","notification_type":"agent_completed","message":"A background session finished"}),
                "Notification",
                None,
            ),
            (
                json!({"hook_event_name":"PreToolUse","session_id":"session","tool_name":"AskUserQuestion","tool_use_id":"tool-2"}),
                "PreToolUse",
                Some(Event::NeedsUser),
            ),
            (
                json!({"hook_event_name":"PreToolUse","session_id":"session","tool_name":"EnterPlanMode"}),
                "PreToolUse",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"PostToolUse","session_id":"session","tool_name":"Bash","tool_use_id":"tool-3","tool_input":{},"tool_response":{}}),
                "PostToolUse",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"PostToolUseFailure","session_id":"session","tool_name":"Bash","tool_use_id":"tool-4","tool_input":{},"error":"exit code 1"}),
                "PostToolUseFailure",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"Stop","session_id":"session","stop_hook_active":true,"background_tasks":[],"session_crons":[]}),
                "Stop",
                Some(Event::Resumed),
            ),
            (
                json!({"hook_event_name":"Stop","session_id":"session","stop_hook_active":false,"background_tasks":[],"session_crons":[]}),
                "Stop",
                Some(Event::TurnStopped {
                    state: TurnStopState::Unknown,
                }),
            ),
            (
                json!({"hook_event_name":"Stop","session_id":"session","background_tasks":[{"id":"task-1"}],"session_crons":[]}),
                "Stop",
                Some(Event::TurnStopped {
                    state: TurnStopState::Working,
                }),
            ),
            (
                json!({"hook_event_name":"Stop","session_id":"session"}),
                "Stop",
                Some(Event::TurnStopped {
                    state: TurnStopState::Unknown,
                }),
            ),
            (
                json!({"hook_event_name":"StopFailure","session_id":"session","error":"rate_limit"}),
                "StopFailure",
                Some(Event::TurnStopped {
                    state: TurnStopState::Unknown,
                }),
            ),
            (
                json!({"hook_event_name":"SessionEnd","session_id":"session","reason":"prompt_input_exit"}),
                "SessionEnd",
                Some(Event::SessionEnded),
            ),
            (
                json!({"hook_event_name":"SessionEnd","session_id":"session","reason":"clear"}),
                "SessionEnd",
                Some(Event::EvidenceLost),
            ),
        ];

        for (payload, expected_name, expected_event) in fixtures {
            let hook: ClaudeHookEvent = serde_json::from_value(payload).unwrap();
            assert_eq!(hook.name(), expected_name);
            assert_eq!(hook.session_id(), "session");
            assert_eq!(hook.lifecycle_event(), expected_event);
        }
        let subagent: ClaudeHookEvent = serde_json::from_value(json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session",
            "prompt_id":prompt_id,
            "permission_mode":"default",
            "agent_id":"agent-1",
            "agent_type":"subagent"
        }))
        .unwrap();
        assert!(!subagent.is_main_session());
        assert!(
            serde_json::from_value::<ClaudeHookEvent>(json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"session",
                "prompt_id":"not-a-uuid",
                "permission_mode":"default"
            }))
            .is_err()
        );
    }

    #[test]
    fn fresh_events_recover_stale_connection_and_ended_sessions_stay_disconnected() {
        let namespace = Uuid::new_v4();
        let mut store = WorkspaceStore::with_identity_namespace(namespace);
        store
            .apply(WorkspaceAction::CreateWindow { focus: true })
            .unwrap();
        let model = store.snapshot();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = Target {
            identity_namespace: namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id: surface.session_id.unwrap(),
        };
        let provider_session_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let t0 = Instant::now();
        let mut runtime = Runtime::default();
        let registration = runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Register {
                        target,
                        provider: Provider::ClaudeLocal,
                        runtime_id,
                        generation,
                        provider_session_id,
                    },
                },
                t0,
            )
            .unwrap();
        let capability = registration.capability.unwrap();
        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation,
                        sequence: 1,
                        event: Event::PromptSubmitted {
                            prompt_id: Uuid::new_v4(),
                        },
                    },
                },
                t0 + Duration::from_millis(1),
            )
            .unwrap();

        let stale_at = t0 + EVIDENCE_FRESHNESS + Duration::from_secs(1);
        assert!(runtime.poll_freshness(stale_at));
        let stale = runtime.snapshot(&model).remove(0);
        assert_eq!(stale.activity, Activity::Unknown);
        assert_eq!(stale.connection, Connection::Unknown);
        assert!(!runtime.poll_freshness(stale_at + Duration::from_secs(1)));

        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation,
                        sequence: 2,
                        event: Event::Resumed,
                    },
                },
                stale_at + Duration::from_secs(1),
            )
            .unwrap();
        let refreshed = runtime.snapshot(&model).remove(0);
        assert_eq!(refreshed.activity, Activity::Working);
        assert_eq!(refreshed.connection, Connection::Connected);

        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation,
                        sequence: 3,
                        event: Event::SessionEnded,
                    },
                },
                stale_at + Duration::from_secs(2),
            )
            .unwrap();
        let session_states =
            std::collections::HashMap::from([(target.session_id, Some(SessionState::Running))]);
        runtime.reconcile(
            &model,
            &session_states,
            stale_at + EVIDENCE_FRESHNESS + Duration::from_secs(2),
        );
        assert_eq!(
            runtime.snapshot(&model)[0].connection,
            Connection::Disconnected
        );
        let retry = runtime.apply(
            Request {
                request_id: Uuid::new_v4(),
                action: Action::Register {
                    target,
                    provider: Provider::ClaudeLocal,
                    runtime_id: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                    provider_session_id,
                },
            },
            stale_at + EVIDENCE_FRESHNESS + Duration::from_secs(3),
        );
        assert!(retry.is_err());
    }
}
