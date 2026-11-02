//! Bounded provider lifecycle evidence for native presentation.
//!
//! This owner records the latest typed hook-derived state for an exact terminal
//! session. Local hook claims use the desktop's same-UID policy; remote hook
//! claims arrive through an exact managed SSH channel. Neither path authenticates
//! the provider process itself or promises provider replay semantics.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;
pub use zmux_remote_runtime::agent_lifecycle::HookDisposition;
use zmux_remote_runtime::agent_lifecycle::{
    ClaudeHook, MAX_PROVIDER_ID_BYTES, ProviderKind, ProviderLifecycleObservation, ProviderMarker,
    ProviderState, ProviderTransition,
};
use zmux_session_runtime::State as SessionState;
use zmux_workspace_core::{
    PaneId, SessionId, Snapshot as Model, SurfaceId, WindowId, WorkspaceId, targets::TargetKind,
};

pub const MAX_RECORDS: usize = 64;
pub const MAX_ALERT_EVENTS: usize = 128;
const MAX_HOOK_RECEIPTS: usize = 256;
const MAX_PROVIDER_TURN_MARKS: usize = 128;
const MAX_PROVIDER_RETIRED_IDENTITIES: usize = 128;
/// Hook events have no provider replay cursor or heartbeat. After this interval,
/// the published state becomes Unknown instead of assuming it is still current.
pub const EVIDENCE_FRESHNESS: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    ClaudeLocal,
    ClaudeRemote,
    CodexLocal,
    CodexRemote,
    #[serde(rename = "opencode_local")]
    OpenCodeLocal,
    #[serde(rename = "opencode_remote")]
    OpenCodeRemote,
}

impl Provider {
    fn is_remote(self) -> bool {
        matches!(
            self,
            Self::ClaudeRemote | Self::CodexRemote | Self::OpenCodeRemote
        )
    }

    fn is_normalized(self) -> bool {
        !matches!(self, Self::ClaudeLocal | Self::ClaudeRemote)
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeLocal | Self::ClaudeRemote => "Claude Code",
            Self::CodexLocal | Self::CodexRemote => "Codex",
            Self::OpenCodeLocal | Self::OpenCodeRemote => "OpenCode",
        }
    }

    pub(crate) fn for_transport(provider: ProviderKind, remote: bool) -> Self {
        match (provider, remote) {
            (ProviderKind::Codex, false) => Self::CodexLocal,
            (ProviderKind::Codex, true) => Self::CodexRemote,
            (ProviderKind::OpenCode, false) => Self::OpenCodeLocal,
            (ProviderKind::OpenCode, true) => Self::OpenCodeRemote,
        }
    }
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

/// One provider-normalized observation bound to an exact Zmux terminal runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderObservationInput {
    pub target: Target,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub sequence: u64,
    pub observation: ProviderLifecycleObservation,
}

/// Exact local provider session identity for an evidence-loss or exit action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSessionEnd {
    pub target: Target,
    pub provider: ProviderKind,
    pub session_id: String,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub sequence: u64,
}

/// Adapter-local sequence and Zmux-generated ID for one admitted remote hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteHookInvocation {
    pub sequence: u64,
    pub hook_id: Uuid,
}

/// Full lifecycle-row identity required by acknowledged native selection.
/// Activity and connection are deliberately excluded so state refreshes do not
/// stale a still-current terminal target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionTarget {
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
    pub provider: Provider,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub provider_session_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    NeedsUser,
    Completed,
}

/// One admitted live event. Cursor and sequence are adapter-local; they are not
/// Claude replay cursors or provider event IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AlertEvent {
    pub cursor: u64,
    pub sequence: u64,
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
    pub provider_session_id: Uuid,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub prompt_id: Uuid,
    pub kind: AlertKind,
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
        /// Zmux-generated correlation ID for this hook invocation. It is not a
        /// provider event ID and does not imply provider replay guarantees.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hook_id: Option<Uuid>,
        event: Event,
    },
    /// Normalized Codex/OpenCode evidence from the exact current local owner.
    /// This action has no Claude capability and cannot cross to SSH admission.
    ProviderObservation {
        target: Target,
        runtime_id: Uuid,
        generation: Uuid,
        sequence: u64,
        observation: ProviderLifecycleObservation,
    },
    /// Invalidate local provider evidence after adapter/daemon loss.
    ProviderEvidenceLost {
        target: Target,
        provider: ProviderKind,
        session_id: String,
        runtime_id: Uuid,
        generation: Uuid,
        sequence: u64,
    },
    /// Retire local provider state after the exact provider session exits.
    ProviderSessionEnded {
        target: Target,
        provider: ProviderKind,
        session_id: String,
        runtime_id: Uuid,
        generation: Uuid,
        sequence: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    PromptSubmitted { prompt_id: Uuid },
    WaitingUnconfirmed,
    NeedsUser,
    Resumed,
    StopContinued,
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
        #[serde(default)]
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
            } => Some(Event::StopContinued),
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
    pub hook_disposition: HookDisposition,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_epoch: Option<Uuid>,
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
    transport_epoch: Option<Uuid>,
    capability: Uuid,
    prompt_id: Option<Uuid>,
    activity: Activity,
    connection: Connection,
    last_sequence: u64,
    last_event: Option<Event>,
    working_prompt_id: Option<Uuid>,
    completed_prompt_id: Option<Uuid>,
    provider_live_turn_id: Option<Uuid>,
    provider_eligible_turn_id: Option<Uuid>,
    provider_waiting_turn_ids: std::collections::VecDeque<Uuid>,
    provider_completed_turn_ids: std::collections::VecDeque<Uuid>,
    provider_noncompletable_turn_ids: std::collections::VecDeque<Uuid>,
    retired_provider_identities: std::collections::VecDeque<ProviderRuntimeGeneration>,
    activity_observed_at: Instant,
    connection_observed_at: Instant,
    terminal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProviderRuntimeGeneration {
    runtime_id: Uuid,
    generation: Uuid,
}

pub struct Runtime {
    records: Vec<Record>,
    alert_epoch: Uuid,
    alert_cursor: u64,
    alert_dropped_through_cursor: u64,
    alert_events: std::collections::VecDeque<StoredAlertEvent>,
    alert_coverage: std::collections::VecDeque<AlertCoverage>,
    hook_receipts: std::collections::VecDeque<StoredHookReceipt>,
}

struct StoredAlertEvent {
    cursor: u64,
    sequence: u64,
    target: Target,
    provider: Provider,
    runtime_id: Uuid,
    generation: Uuid,
    provider_session_id: Uuid,
    prompt_id: Uuid,
    kind: AlertKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AlertCoverage {
    target: Target,
    provider: Provider,
    runtime_id: Uuid,
    generation: Uuid,
    provider_session_id: Uuid,
    prompt_id: Uuid,
    kind: AlertKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredHookReceipt {
    hook_id: Uuid,
    capability: Uuid,
    runtime_id: Uuid,
    generation: Uuid,
    sequence: u64,
    event: Event,
    disposition: HookDisposition,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            alert_epoch: Uuid::new_v4(),
            alert_cursor: 0,
            alert_dropped_through_cursor: 0,
            alert_events: std::collections::VecDeque::new(),
            alert_coverage: std::collections::VecDeque::new(),
            hook_receipts: std::collections::VecDeque::new(),
        }
    }
}

impl Runtime {
    pub fn target_for_capability(&self, capability: Uuid) -> Option<Target> {
        self.records
            .iter()
            .find(|record| record.capability == capability)
            .map(|record| record.target)
    }

    pub fn provider_for_capability(&self, capability: Uuid) -> Option<Provider> {
        self.records
            .iter()
            .find(|record| record.capability == capability)
            .map(|record| record.provider)
    }

    pub fn matches_selection(&self, target: SelectionTarget, model: &Model) -> bool {
        let Some(record) = self.records.iter().find(|record| {
            record.target.identity_namespace == target.identity_namespace
                && record.target.surface_id == target.surface_id
                && record.target.surface_uuid == target.surface_uuid
                && record.target.session_id == target.session_id
                && record.provider == target.provider
                && record.runtime_id == target.runtime_id
                && record.generation == target.generation
                && record.provider_session_id == target.provider_session_id
        }) else {
            return false;
        };
        owner(model, record.target, record.provider).is_some_and(|owner| {
            owner.window == target.window_id
                && owner.workspace == target.workspace_id
                && owner.pane == target.pane_id
                && owner.surface == target.surface_id
                && model.object_uuid(TargetKind::Window, owner.window.0) == target.window_uuid
                && model.object_uuid(TargetKind::Workspace, owner.workspace.0)
                    == target.workspace_uuid
                && model.object_uuid(TargetKind::Pane, owner.pane.0) == target.pane_uuid
                && model.object_uuid(TargetKind::Surface, owner.surface.0) == target.surface_uuid
        })
    }

    pub fn alert_epoch(&self) -> Uuid {
        self.alert_epoch
    }

    pub fn alert_cursor(&self) -> u64 {
        self.alert_cursor
    }

    pub fn alert_dropped_through_cursor(&self) -> u64 {
        self.alert_dropped_through_cursor
    }

    pub fn alert_events(&self, model: &Model) -> Vec<AlertEvent> {
        self.alert_events
            .iter()
            .filter_map(|event| {
                let owner = owner(model, event.target, event.provider)?;
                Some(AlertEvent {
                    cursor: event.cursor,
                    sequence: event.sequence,
                    provider: event.provider,
                    identity_namespace: event.target.identity_namespace,
                    window_id: owner.window,
                    window_uuid: model.object_uuid(TargetKind::Window, owner.window.0),
                    workspace_id: owner.workspace,
                    workspace_uuid: model.object_uuid(TargetKind::Workspace, owner.workspace.0),
                    pane_id: owner.pane,
                    pane_uuid: model.object_uuid(TargetKind::Pane, owner.pane.0),
                    surface_id: owner.surface,
                    surface_uuid: event.target.surface_uuid,
                    session_id: event.target.session_id,
                    provider_session_id: event.provider_session_id,
                    runtime_id: event.runtime_id,
                    generation: event.generation,
                    prompt_id: event.prompt_id,
                    kind: event.kind,
                })
            })
            .collect()
    }

    /// A heartbeat only proves that the managed SSH lifecycle channel is
    /// responding. It refreshes connection state, never provider activity.
    pub fn remote_transport_heartbeat(
        &mut self,
        target: Target,
        runtime_id: Uuid,
        generation: Uuid,
        now: Instant,
    ) -> bool {
        let mut changed = false;
        for record in &mut self.records {
            if record.target == target
                && record.provider.is_remote()
                && record.runtime_id == runtime_id
                && record.generation == generation
                && !record.terminal
                && record.connection != Connection::Disconnected
            {
                changed |= record.connection != Connection::Connected;
                record.connection = Connection::Connected;
                record.connection_observed_at = now;
            }
        }
        changed
    }

    /// Lose the remote channel without turning transport loss into a provider
    /// event or an Idle/Disconnected activity claim.
    pub fn remote_transport_lost(
        &mut self,
        target: Target,
        runtime_id: Uuid,
        generation: Uuid,
    ) -> bool {
        let mut changed = false;
        for record in &mut self.records {
            if record.target == target
                && record.provider.is_remote()
                && record.runtime_id == runtime_id
                && record.generation == generation
                && !record.terminal
            {
                record.activity = Activity::Unknown;
                record.connection = Connection::Unknown;
                record.terminal = true;
                record.provider_eligible_turn_id = None;
                changed = true;
            }
        }
        changed
    }

    /// Admit a new nonce/version handshake over the existing managed SSH
    /// ControlMaster. It fences the old stream generation and clears the old
    /// prompt/run identity so fresh channel traffic cannot replay a prior alert.
    pub fn remote_transport_reconnected(
        &mut self,
        target: Target,
        runtime_id: Uuid,
        previous_generation: Uuid,
        generation: Uuid,
        now: Instant,
    ) -> bool {
        if generation.is_nil() || generation == previous_generation {
            return false;
        }
        let mut changed = false;
        for record in &mut self.records {
            if record.target == target
                && record.provider.is_remote()
                && record.runtime_id == runtime_id
                && record.generation == previous_generation
                && record.terminal
                && record.connection == Connection::Unknown
            {
                record.generation = generation;
                record.transport_epoch = Some(generation);
                record.prompt_id = None;
                record.working_prompt_id = None;
                record.completed_prompt_id = None;
                record.provider_live_turn_id = None;
                record.provider_eligible_turn_id = None;
                record.activity = Activity::Unknown;
                record.connection = Connection::Connected;
                record.last_sequence = 0;
                record.last_event = None;
                record.activity_observed_at = now;
                record.connection_observed_at = now;
                record.terminal = false;
                changed = true;
            }
        }
        changed
    }

    /// Normalize one provider observation for the exact current local terminal.
    /// The provider's opaque session and turn IDs are deterministically mapped
    /// to UUIDv5 values before they enter the public UUID-shaped snapshot.
    pub fn apply_local_provider_observation(
        &mut self,
        model: &Model,
        input: ProviderObservationInput,
        now: Instant,
    ) -> Result<(), String> {
        let provider = Provider::for_transport(input.observation.provider, false);
        if owner(model, input.target, provider).is_none() {
            return Err("provider lifecycle target is not the exact current local terminal".into());
        }
        self.apply_provider_observation(input, provider, now)
    }

    /// Admit one sanitized provider frame from the exact managed SSH stream.
    /// Callers must first validate the current binding, launch, nonce, and SSH
    /// session; this method then uses the same owner state machine as local input.
    pub fn apply_remote_provider_observation(
        &mut self,
        model: &Model,
        input: ProviderObservationInput,
        now: Instant,
    ) -> Result<(), String> {
        let provider = Provider::for_transport(input.observation.provider, true);
        if owner(model, input.target, provider).is_none() {
            return Err(
                "provider lifecycle target is not the exact current remote terminal".into(),
            );
        }
        let target = input.target;
        let runtime_id = input.runtime_id;
        let generation = input.generation;
        self.apply_provider_observation(input, provider, now)?;
        self.remote_transport_heartbeat(target, runtime_id, generation, now);
        Ok(())
    }

    /// Fence one exact local provider session after its adapter or background
    /// evidence source fails. Activity and connection become `Unknown`.
    pub fn apply_local_provider_evidence_lost(
        &mut self,
        model: &Model,
        end: ProviderSessionEnd,
        now: Instant,
    ) -> Result<(), String> {
        self.apply_local_provider_end(model, end, Connection::Unknown, now)
    }

    /// Retire one exact local provider session after the provider reports its
    /// exit. Activity becomes `Unknown` and connection becomes `Disconnected`.
    pub fn apply_local_provider_session_ended(
        &mut self,
        model: &Model,
        end: ProviderSessionEnd,
        now: Instant,
    ) -> Result<(), String> {
        self.apply_local_provider_end(model, end, Connection::Disconnected, now)
    }

    fn apply_local_provider_end(
        &mut self,
        model: &Model,
        end: ProviderSessionEnd,
        connection: Connection,
        now: Instant,
    ) -> Result<(), String> {
        validate_provider_id(&end.session_id)?;
        let provider = Provider::for_transport(end.provider, false);
        if owner(model, end.target, provider).is_none() {
            return Err("provider lifecycle target is not the exact current local terminal".into());
        }
        if end.runtime_id.is_nil() || end.generation.is_nil() || end.sequence == 0 {
            return Err("provider lifecycle end identity is incomplete".into());
        }
        let provider_session_id = provider_session_uuid(end.provider, &end.session_id);
        let Some(record) = self.records.iter_mut().find(|record| {
            record.target == end.target
                && record.provider == provider
                && record.runtime_id == end.runtime_id
                && record.generation == end.generation
                && record.provider_session_id == provider_session_id
                && !record.terminal
        }) else {
            return Err("provider lifecycle session is stale or unknown".into());
        };
        if end.sequence <= record.last_sequence
            || end.sequence != record.last_sequence.saturating_add(1)
        {
            record.activity = Activity::Unknown;
            record.connection = Connection::Unknown;
            record.provider_eligible_turn_id = None;
            record.terminal = true;
            record.activity_observed_at = now;
            record.connection_observed_at = now;
            return Err("provider lifecycle end sequence is stale or gapped".into());
        }
        record.last_sequence = end.sequence;
        record.last_event = None;
        record.activity = Activity::Unknown;
        record.connection = connection;
        record.provider_eligible_turn_id = None;
        record.terminal = true;
        record.activity_observed_at = now;
        record.connection_observed_at = now;
        Ok(())
    }

    fn apply_provider_observation(
        &mut self,
        input: ProviderObservationInput,
        provider: Provider,
        now: Instant,
    ) -> Result<(), String> {
        let ProviderObservationInput {
            target,
            runtime_id,
            generation,
            sequence,
            observation,
        } = input;
        observation.validate().map_err(|error| error.to_owned())?;
        if target.identity_namespace.is_nil()
            || target.surface_uuid.is_nil()
            || target.surface_id.0 == 0
            || target.session_id.0 == 0
            || runtime_id.is_nil()
            || generation.is_nil()
            || sequence == 0
        {
            return Err("provider lifecycle observation identity is incomplete".into());
        }

        let provider_session_id =
            provider_session_uuid(observation.provider, &observation.session_id);
        let provider_turn_id = observation.turn_id.as_deref().map(|turn_id| {
            provider_turn_uuid(observation.provider, &observation.session_id, turn_id)
        });
        let exact_index = self.records.iter().position(|record| {
            record.target == target
                && record.provider == provider
                && record.runtime_id == runtime_id
                && record.generation == generation
                && record.provider_session_id == provider_session_id
        });

        let matching_session_indices: Vec<usize> = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                (record.target == target
                    && record.provider == provider
                    && record.provider_session_id == provider_session_id)
                    .then_some(index)
            })
            .collect();
        let record_index = if let Some(index) = exact_index {
            index
        } else if !matching_session_indices.is_empty() {
            let requested_identity = ProviderRuntimeGeneration {
                runtime_id,
                generation,
            };
            if matching_session_indices.iter().any(|index| {
                self.records[*index]
                    .retired_provider_identities
                    .contains(&requested_identity)
            }) {
                return Err("provider lifecycle runtime or generation was retired".into());
            }
            let replaceable_local_loss = !provider.is_remote()
                && matching_session_indices.iter().all(|index| {
                    let record = &self.records[*index];
                    record.terminal
                        && matches!(
                            record.connection,
                            Connection::Unknown | Connection::Disconnected
                        )
                });
            if !replaceable_local_loss {
                return Err("provider lifecycle session has a stale runtime or generation".into());
            }
            if matching_session_indices.len() != 1 {
                return Err("provider lifecycle session has duplicate retained records".into());
            }

            // Reuse the session record so its bounded outcome history and
            // retired-generation fence survive recoverable adapter loss.
            let index = matching_session_indices[0];
            let record = &mut self.records[index];
            push_bounded_provider_identity(
                &mut record.retired_provider_identities,
                ProviderRuntimeGeneration {
                    runtime_id: record.runtime_id,
                    generation: record.generation,
                },
            );
            record.runtime_id = runtime_id;
            record.generation = generation;
            record.transport_epoch = provider.is_remote().then_some(generation);
            record.capability = Uuid::new_v4();
            record.prompt_id = None;
            record.activity = Activity::Unknown;
            record.connection = Connection::Connected;
            record.last_sequence = 0;
            record.last_event = None;
            record.working_prompt_id = None;
            record.completed_prompt_id = None;
            record.provider_live_turn_id = None;
            record.provider_eligible_turn_id = None;
            record.activity_observed_at = now;
            record.connection_observed_at = now;
            record.terminal = false;
            index
        } else {
            if self.records.len() >= MAX_RECORDS {
                return Err("live agent lifecycle capacity is full".into());
            }
            let index = self.records.len();
            self.records.push(Record {
                target,
                provider,
                runtime_id,
                generation,
                provider_session_id,
                transport_epoch: provider.is_remote().then_some(generation),
                capability: Uuid::new_v4(),
                prompt_id: None,
                activity: Activity::Unknown,
                connection: Connection::Connected,
                last_sequence: 0,
                last_event: None,
                working_prompt_id: None,
                completed_prompt_id: None,
                provider_live_turn_id: None,
                provider_eligible_turn_id: None,
                provider_waiting_turn_ids: std::collections::VecDeque::new(),
                provider_completed_turn_ids: std::collections::VecDeque::new(),
                provider_noncompletable_turn_ids: std::collections::VecDeque::new(),
                retired_provider_identities: std::collections::VecDeque::new(),
                activity_observed_at: now,
                connection_observed_at: now,
                terminal: false,
            });
            index
        };

        if self.records[record_index].terminal
            || self.records[record_index].connection == Connection::Disconnected
        {
            return Err("provider lifecycle record is terminal".into());
        }
        let prior_sequence = self.records[record_index].last_sequence;
        let sequence_gap = !provider.is_remote() && sequence != prior_sequence.saturating_add(1);
        let sequence_stale = sequence <= prior_sequence;
        if sequence_stale || sequence_gap {
            let record = &mut self.records[record_index];
            record.activity = Activity::Unknown;
            record.connection = Connection::Unknown;
            record.provider_eligible_turn_id = None;
            record.terminal = true;
            record.activity_observed_at = now;
            record.connection_observed_at = now;
            return Err(if sequence_stale {
                "provider lifecycle sequence is stale".into()
            } else {
                "provider lifecycle sequence has a gap".into()
            });
        }

        let mut alert = None;
        {
            let record = &mut self.records[record_index];
            record.connection = Connection::Connected;
            record.connection_observed_at = now;
            record.last_sequence = sequence;
            record.last_event = None;

            let live = observation.marker == ProviderMarker::Live;
            let superseded_snapshot = !live && record.provider_live_turn_id.is_some();
            if !superseded_snapshot {
                record.activity_observed_at = now;
                match observation.state {
                    ProviderState::Working | ProviderState::Waiting => {
                        let turn_id =
                            provider_turn_id.expect("validated active state has a turn ID");
                        let newly_waiting = if observation.state == ProviderState::Waiting {
                            let unseen = !record.provider_waiting_turn_ids.contains(&turn_id);
                            push_bounded(&mut record.provider_waiting_turn_ids, turn_id);
                            unseen
                        } else {
                            false
                        };
                        if record.prompt_id != Some(turn_id) {
                            record.prompt_id = Some(turn_id);
                            record.provider_eligible_turn_id = None;
                            record.working_prompt_id = None;
                            record.completed_prompt_id = None;
                        }
                        record.activity = match observation.state {
                            ProviderState::Working => Activity::Working,
                            ProviderState::Waiting => Activity::Waiting,
                            ProviderState::Idle => unreachable!(),
                        };
                        if live && observation.transition == ProviderTransition::StateChanged {
                            record.provider_live_turn_id = Some(turn_id);
                        }
                        match (observation.transition, observation.state, live) {
                            (ProviderTransition::StateChanged, ProviderState::Working, true)
                                if !record.provider_noncompletable_turn_ids.contains(&turn_id)
                                    && !record.provider_completed_turn_ids.contains(&turn_id) =>
                            {
                                record.provider_eligible_turn_id = Some(turn_id);
                            }
                            (ProviderTransition::StateChanged, ProviderState::Waiting, true)
                                if newly_waiting =>
                            {
                                alert = Some((turn_id, AlertKind::NeedsUser));
                            }
                            _ => {}
                        }
                    }
                    ProviderState::Idle => {
                        let turn_id = provider_turn_id;
                        if let Some(turn_id) = turn_id
                            && record.prompt_id.is_none_or(|current| current == turn_id)
                        {
                            record.prompt_id = Some(turn_id);
                        }
                        let same_current_turn =
                            turn_id.is_some_and(|turn_id| record.prompt_id == Some(turn_id));
                        let was_current = turn_id.is_none() || same_current_turn;
                        if was_current {
                            record.activity = Activity::Idle;
                        }
                        match observation.transition {
                            ProviderTransition::TurnCompleted => {
                                if let Some(turn_id) = turn_id {
                                    let newly_completed =
                                        !record.provider_completed_turn_ids.contains(&turn_id);
                                    let confirmed_new_run = record.provider_eligible_turn_id
                                        == Some(turn_id)
                                        && !record
                                            .provider_noncompletable_turn_ids
                                            .contains(&turn_id);
                                    if newly_completed {
                                        push_bounded(
                                            &mut record.provider_completed_turn_ids,
                                            turn_id,
                                        );
                                        if live && confirmed_new_run {
                                            record.completed_prompt_id = Some(turn_id);
                                            alert = Some((turn_id, AlertKind::Completed));
                                        }
                                    }
                                    if record.provider_eligible_turn_id == Some(turn_id) {
                                        record.provider_eligible_turn_id = None;
                                    }
                                }
                            }
                            ProviderTransition::TurnFailed
                            | ProviderTransition::TurnInterrupted => {
                                if let Some(turn_id) = turn_id {
                                    push_bounded(
                                        &mut record.provider_noncompletable_turn_ids,
                                        turn_id,
                                    );
                                    if record.provider_eligible_turn_id == Some(turn_id) {
                                        record.provider_eligible_turn_id = None;
                                    }
                                }
                            }
                            _ => {}
                        }
                        if was_current {
                            record.activity = Activity::Idle;
                        }
                    }
                }
            }
        }

        if let Some((prompt_id, kind)) = alert {
            self.append_alert(StoredAlertEvent {
                cursor: 0,
                sequence,
                target,
                provider,
                runtime_id,
                generation,
                provider_session_id,
                prompt_id,
                kind,
            });
        }
        Ok(())
    }

    /// Admit a sanitized hook from the private managed-SSH stream. Local control
    /// requests cannot call this path, and the caller must first revalidate the
    /// exact remote terminal owner and launch generation.
    pub fn apply_remote_hook(
        &mut self,
        target: Target,
        runtime_id: Uuid,
        generation: Uuid,
        invocation: RemoteHookInvocation,
        hook: ClaudeHook,
        now: Instant,
    ) -> Result<HookDisposition, String> {
        let RemoteHookInvocation { sequence, hook_id } = invocation;
        if !hook.main_session {
            return Err("remote lifecycle hook is not for the main provider session".into());
        }
        if target.identity_namespace.is_nil()
            || target.surface_uuid.is_nil()
            || target.surface_id.0 == 0
            || target.session_id.0 == 0
            || runtime_id.is_nil()
            || generation.is_nil()
            || sequence == 0
            || hook_id.is_nil()
            || hook.session_id.is_nil()
        {
            return Err("remote lifecycle event identity is incomplete".into());
        }

        let event = remote_hook_lifecycle_event(&hook);
        if hook.name != "SessionStart" && event.is_none() {
            return Err("remote lifecycle hook has no supported event".into());
        }
        let provider_session_id = hook.session_id;
        let request_id = Uuid::new_v4();
        let existing = self.records.iter().find(|record| {
            record.target == target
                && record.provider == Provider::ClaudeRemote
                && record.runtime_id == runtime_id
                && record.generation == generation
                && record.provider_session_id == provider_session_id
        });
        let prior_hook_receipt = self
            .hook_receipts
            .iter()
            .any(|receipt| receipt.hook_id == hook_id);
        let tracked_stop = hook.name == "Stop"
            && existing.is_some_and(|record| {
                record.prompt_id.is_some_and(|prompt_id| {
                    record.working_prompt_id == Some(prompt_id)
                        && record.completed_prompt_id != Some(prompt_id)
                        && hook
                            .prompt_id
                            .is_none_or(|hook_prompt_id| hook_prompt_id == prompt_id)
                })
            });
        let completed_stop = hook.name == "Stop"
            && existing.is_some_and(|record| {
                record.prompt_id.is_some_and(|prompt_id| {
                    record.completed_prompt_id == Some(prompt_id)
                        && hook
                            .prompt_id
                            .is_none_or(|hook_prompt_id| hook_prompt_id == prompt_id)
                })
            });
        let late_completed_stop = completed_stop
            && matches!(
                event.as_ref(),
                Some(Event::TurnStopped {
                    state: TurnStopState::Unknown
                })
            );
        let completed_stop_with_activity = completed_stop
            && matches!(
                event.as_ref(),
                Some(
                    Event::StopContinued
                        | Event::TurnStopped {
                            state: TurnStopState::Working
                        }
                )
            );
        if existing.is_some_and(|record| record.terminal) {
            return Err("remote lifecycle record is terminal".into());
        }
        if matches!(
            event.as_ref(),
            Some(
                Event::WaitingUnconfirmed
                    | Event::NeedsUser
                    | Event::Resumed
                    | Event::StopContinued
                    | Event::TurnStopped { .. }
            )
        ) && existing.is_none_or(|record| record.prompt_id.is_none())
        {
            // A session notification without the current prompt/run identity
            // remains Unknown and cannot produce an alert.
            return Err("remote lifecycle event has no current provider run".into());
        }
        let capability = match existing.map(|record| record.capability) {
            Some(capability) => {
                self.remote_transport_heartbeat(target, runtime_id, generation, now);
                capability
            }
            None => self
                .apply(
                    Request {
                        request_id,
                        action: Action::Register {
                            target,
                            provider: Provider::ClaudeRemote,
                            runtime_id,
                            generation,
                            provider_session_id,
                        },
                    },
                    now,
                )?
                .capability
                .ok_or_else(|| "remote lifecycle registration returned no capability".to_owned())?,
        };
        if let Some(record) = self.records.iter_mut().find(|record| {
            record.capability == capability
                && record.runtime_id == runtime_id
                && record.generation == generation
                && record.provider == Provider::ClaudeRemote
        }) {
            // This method is reached only after the stream reader has accepted
            // its first nonce/version handshake. Heartbeats never change epoch.
            record.transport_epoch.get_or_insert(generation);
        }
        let Some(event) = event else {
            return Ok(HookDisposition::NotHandled);
        };
        if late_completed_stop && !prior_hook_receipt {
            // A delayed Stop for the just-completed provider run cannot move
            // it back to Working/Unknown. The invocation is accepted without
            // an alert; if its receipt is lost, the provider hook's visual
            // fallback remains at-least-once and silent.
            let record = self
                .records
                .iter_mut()
                .find(|record| record.capability == capability)
                .ok_or_else(|| "completed remote Stop owner disappeared".to_owned())?;
            if sequence <= record.last_sequence {
                return Err("remote lifecycle event sequence is stale".into());
            }
            record.last_sequence = sequence;
            record.last_event = Some(event.clone());
            self.remember_hook_receipt(StoredHookReceipt {
                hook_id,
                capability,
                runtime_id,
                generation,
                sequence,
                event,
                disposition: HookDisposition::HandledNoAlert,
            });
            return Ok(HookDisposition::HandledNoAlert);
        }
        let receipt = self.apply(
            Request {
                request_id: Uuid::new_v4(),
                action: Action::Event {
                    capability,
                    runtime_id,
                    generation,
                    sequence,
                    hook_id: Some(hook_id),
                    event,
                },
            },
            now,
        )?;
        let disposition = if hook.name == "Stop" && !prior_hook_receipt {
            let disposition = if tracked_stop {
                if receipt.hook_disposition == HookDisposition::AlertCovered {
                    HookDisposition::AlertCovered
                } else {
                    // Stop is not proof of completion. A current tracked run
                    // has accepted this exact Stop event, so defer completion
                    // notification to a later idle_prompt without emitting a
                    // duplicate fallback.
                    HookDisposition::HandledNoAlert
                }
            } else {
                if completed_stop_with_activity {
                    // Preserve explicit resumed/background-work evidence while
                    // suppressing the generic "finished" fallback for a run
                    // whose completion alert was already admitted.
                    HookDisposition::HandledNoAlert
                } else {
                    // An untracked run cannot claim that a later idle_prompt
                    // will cover this Stop invocation.
                    HookDisposition::NotHandled
                }
            };
            if disposition != receipt.hook_disposition {
                let stored = self
                    .hook_receipts
                    .iter_mut()
                    .find(|receipt| receipt.hook_id == hook_id)
                    .ok_or_else(|| "accepted Stop hook receipt was not retained".to_owned())?;
                stored.disposition = disposition;
            }
            disposition
        } else {
            receipt.hook_disposition
        };
        Ok(disposition)
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
                if provider.is_normalized() {
                    return Err(
                        "Codex and OpenCode require provider-tagged lifecycle observations".into(),
                    );
                }
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
                    existing.connection_observed_at = now;
                    existing.connection = Connection::Connected;
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        hook_disposition: HookDisposition::NotHandled,
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
                    transport_epoch: None,
                    capability,
                    prompt_id: None,
                    activity: Activity::Unknown,
                    connection: Connection::Connected,
                    last_sequence: 0,
                    last_event: None,
                    working_prompt_id: None,
                    completed_prompt_id: None,
                    provider_live_turn_id: None,
                    provider_eligible_turn_id: None,
                    provider_waiting_turn_ids: std::collections::VecDeque::new(),
                    provider_completed_turn_ids: std::collections::VecDeque::new(),
                    provider_noncompletable_turn_ids: std::collections::VecDeque::new(),
                    retired_provider_identities: std::collections::VecDeque::new(),
                    activity_observed_at: now,
                    connection_observed_at: now,
                    terminal: false,
                });
                Ok(Receipt {
                    request_id: request.request_id,
                    accepted: true,
                    hook_disposition: HookDisposition::NotHandled,
                    capability: Some(capability),
                })
            }
            Action::Event {
                capability,
                runtime_id,
                generation,
                sequence,
                hook_id,
                event,
            } => {
                if capability.is_nil()
                    || runtime_id.is_nil()
                    || generation.is_nil()
                    || sequence == 0
                    || hook_id.is_some_and(|hook_id| hook_id.is_nil())
                {
                    return Err("lifecycle event identity is incomplete".into());
                }
                let Some(record_index) = self
                    .records
                    .iter()
                    .position(|record| record.capability == capability)
                else {
                    return Err("lifecycle capability is stale or unknown".into());
                };
                let record = &self.records[record_index];
                if record.runtime_id != runtime_id || record.generation != generation {
                    return Err("lifecycle generation is stale".into());
                }
                if record.terminal || record.connection == Connection::Disconnected {
                    return Err("lifecycle capability is terminal or disconnected".into());
                }
                if let Some(hook_id) = hook_id
                    && let Some(receipt) = self
                        .hook_receipts
                        .iter()
                        .find(|receipt| receipt.hook_id == hook_id)
                {
                    if receipt.capability != capability
                        || receipt.runtime_id != runtime_id
                        || receipt.generation != generation
                        || receipt.sequence != sequence
                        || receipt.event != event
                    {
                        return Err("lifecycle hook ID was reused for a different event".into());
                    }
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        hook_disposition: receipt.disposition,
                        capability: Some(capability),
                    });
                }
                let covered_before = alert_coverage_for(record, &event)
                    .is_some_and(|coverage| self.alert_coverage.contains(&coverage));
                // Stop can be delivered after idle_prompt even though it
                // belongs to the same completed provider turn. The hook ID
                // marks this as provider evidence; preserve the completed
                // state and acknowledge it without another visual fallback.
                let late_completed_stop = hook_id.is_some()
                    && record
                        .prompt_id
                        .is_some_and(|prompt_id| record.completed_prompt_id == Some(prompt_id))
                    && matches!(
                        &event,
                        Event::TurnStopped {
                            state: TurnStopState::Unknown
                        }
                    );
                let late_completed_activity = hook_id.is_some()
                    && record
                        .prompt_id
                        .is_some_and(|prompt_id| record.completed_prompt_id == Some(prompt_id))
                    && matches!(
                        &event,
                        Event::StopContinued
                            | Event::TurnStopped {
                                state: TurnStopState::Working
                            }
                    );
                let duplicate_sequence =
                    sequence == record.last_sequence && record.last_event.as_ref() == Some(&event);
                if duplicate_sequence {
                    let tracked_run = record.prompt_id.is_some_and(|prompt_id| {
                        record.working_prompt_id == Some(prompt_id)
                            && record.completed_prompt_id != Some(prompt_id)
                    });
                    let handled_no_alert = late_completed_stop
                        || late_completed_activity
                        || (hook_id.is_some() && tracked_run && defers_completion_alert(&event));
                    let hook_disposition = if covered_before {
                        HookDisposition::AlertCovered
                    } else if handled_no_alert {
                        HookDisposition::HandledNoAlert
                    } else {
                        HookDisposition::NotHandled
                    };
                    if let Some(hook_id) = hook_id {
                        self.remember_hook_receipt(StoredHookReceipt {
                            hook_id,
                            capability,
                            runtime_id,
                            generation,
                            sequence,
                            event: event.clone(),
                            disposition: hook_disposition,
                        });
                    }
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        hook_disposition,
                        capability: Some(capability),
                    });
                }
                if late_completed_stop {
                    if sequence <= record.last_sequence {
                        return Err("lifecycle event sequence is stale".into());
                    }
                    {
                        let record = &mut self.records[record_index];
                        record.connection = Connection::Connected;
                        record.connection_observed_at = now;
                        record.last_sequence = sequence;
                        record.last_event = Some(event.clone());
                    }
                    let disposition = HookDisposition::HandledNoAlert;
                    if let Some(hook_id) = hook_id {
                        self.remember_hook_receipt(StoredHookReceipt {
                            hook_id,
                            capability,
                            runtime_id,
                            generation,
                            sequence,
                            event,
                            disposition,
                        });
                    }
                    return Ok(Receipt {
                        request_id: request.request_id,
                        accepted: true,
                        hook_disposition: disposition,
                        capability: Some(capability),
                    });
                }
                let alert = {
                    let record = &mut self.records[record_index];
                    if sequence <= record.last_sequence {
                        return Err("lifecycle event sequence is stale".into());
                    }

                    let previous_activity = record.activity;
                    match &event {
                        Event::PromptSubmitted { prompt_id } => {
                            if prompt_id.is_nil() {
                                return Err("provider prompt id cannot be nil".into());
                            }
                            record.prompt_id = Some(*prompt_id);
                            record.completed_prompt_id = None;
                            record.working_prompt_id = Some(*prompt_id);
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
                                return Err(
                                    "resumption evidence has no current provider run".into()
                                );
                            }
                            record.working_prompt_id = record.prompt_id;
                            record.activity = Activity::Working;
                        }
                        Event::StopContinued => {
                            if record.prompt_id.is_none() {
                                return Err(
                                    "stop continuation evidence has no current provider run".into(),
                                );
                            }
                            record.working_prompt_id = record.prompt_id;
                            record.activity = Activity::Working;
                        }
                        Event::TurnStopped { state } => {
                            if record.prompt_id.is_none() {
                                return Err(
                                    "stopped-turn evidence has no current provider run".into()
                                );
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
                    record.last_event = Some(event.clone());
                    record.activity_observed_at = now;
                    record.connection_observed_at = now;
                    match (&event, previous_activity, record.prompt_id) {
                        (Event::NeedsUser, prior, Some(prompt_id))
                            if prior != Activity::Waiting =>
                        {
                            Some((prompt_id, AlertKind::NeedsUser))
                        }
                        (
                            Event::TurnStopped {
                                state: TurnStopState::Idle,
                            },
                            _,
                            Some(prompt_id),
                        ) if record.working_prompt_id == Some(prompt_id)
                            && record.completed_prompt_id != Some(prompt_id) =>
                        {
                            record.completed_prompt_id = Some(prompt_id);
                            Some((prompt_id, AlertKind::Completed))
                        }
                        _ => None,
                    }
                    .map(|(prompt_id, kind)| StoredAlertEvent {
                        cursor: 0,
                        sequence,
                        target: record.target,
                        provider: record.provider,
                        runtime_id: record.runtime_id,
                        generation: record.generation,
                        provider_session_id: record.provider_session_id,
                        prompt_id,
                        kind,
                    })
                };
                let record = &self.records[record_index];
                let tracked_run = record.prompt_id.is_some_and(|prompt_id| {
                    record.working_prompt_id == Some(prompt_id)
                        && record.completed_prompt_id != Some(prompt_id)
                });
                let handled_no_alert = late_completed_activity
                    || (hook_id.is_some() && tracked_run && defers_completion_alert(&event));
                let hook_disposition = if alert.is_some() || covered_before {
                    HookDisposition::AlertCovered
                } else if handled_no_alert {
                    HookDisposition::HandledNoAlert
                } else {
                    HookDisposition::NotHandled
                };
                if let Some(alert) = alert {
                    self.remember_alert_coverage(AlertCoverage {
                        target: alert.target,
                        provider: alert.provider,
                        runtime_id: alert.runtime_id,
                        generation: alert.generation,
                        provider_session_id: alert.provider_session_id,
                        prompt_id: alert.prompt_id,
                        kind: alert.kind,
                    });
                    self.append_alert(alert);
                }
                if let Some(hook_id) = hook_id {
                    self.remember_hook_receipt(StoredHookReceipt {
                        hook_id,
                        capability,
                        runtime_id,
                        generation,
                        sequence,
                        event,
                        disposition: hook_disposition,
                    });
                }
                Ok(Receipt {
                    request_id: request.request_id,
                    accepted: true,
                    hook_disposition,
                    capability: Some(capability),
                })
            }
            Action::ProviderObservation { .. }
            | Action::ProviderEvidenceLost { .. }
            | Action::ProviderSessionEnded { .. } => Err(
                "provider actions require exact local admission through the desktop owner".into(),
            ),
        }
    }

    fn remember_alert_coverage(&mut self, coverage: AlertCoverage) {
        if self.alert_coverage.contains(&coverage) {
            return;
        }
        self.alert_coverage.push_back(coverage);
        while self.alert_coverage.len() > MAX_ALERT_EVENTS {
            self.alert_coverage.pop_front();
        }
    }

    fn remember_hook_receipt(&mut self, receipt: StoredHookReceipt) {
        if self
            .hook_receipts
            .iter()
            .any(|existing| existing.hook_id == receipt.hook_id)
        {
            return;
        }
        self.hook_receipts.push_back(receipt);
        while self.hook_receipts.len() > MAX_HOOK_RECEIPTS {
            self.hook_receipts.pop_front();
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
            if owner(model, record.target, record.provider).is_none() {
                return false;
            }
            match session_states
                .get(&record.target.session_id)
                .copied()
                .flatten()
            {
                Some(SessionState::Running) => {
                    if record.activity != Activity::Unknown
                        && now.saturating_duration_since(record.activity_observed_at)
                            >= EVIDENCE_FRESHNESS
                    {
                        record.activity = Activity::Unknown;
                    }
                    if record.connection == Connection::Connected
                        && now.saturating_duration_since(record.connection_observed_at)
                            >= EVIDENCE_FRESHNESS
                    {
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
                && now.saturating_duration_since(record.connection_observed_at)
                    >= EVIDENCE_FRESHNESS
            {
                record.connection = Connection::Unknown;
                changed = true;
            }
            if record.activity != Activity::Unknown
                && now.saturating_duration_since(record.activity_observed_at) >= EVIDENCE_FRESHNESS
            {
                record.activity = Activity::Unknown;
                changed = true;
            }
        }
        changed
    }

    pub fn snapshot(&self, model: &Model) -> Vec<Agent> {
        self.records
            .iter()
            .filter_map(|record| {
                let owner = owner(model, record.target, record.provider)?;
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
                    transport_epoch: record.transport_epoch,
                    prompt_id: record.prompt_id,
                    activity: record.activity,
                    connection: record.connection,
                })
            })
            .collect()
    }

    fn append_alert(&mut self, mut event: StoredAlertEvent) {
        self.alert_cursor = self.alert_cursor.saturating_add(1);
        event.cursor = self.alert_cursor;
        self.alert_events.push_back(event);
        if self.alert_events.len() > MAX_ALERT_EVENTS
            && let Some(dropped) = self.alert_events.pop_front()
        {
            self.alert_dropped_through_cursor = dropped.cursor;
        }
    }
}

fn alert_coverage_for(record: &Record, event: &Event) -> Option<AlertCoverage> {
    let (prompt_id, kind) = match event {
        Event::NeedsUser => (record.prompt_id?, AlertKind::NeedsUser),
        Event::TurnStopped {
            state: TurnStopState::Idle,
        } => {
            let prompt_id = record.prompt_id?;
            (record.working_prompt_id == Some(prompt_id))
                .then_some((prompt_id, AlertKind::Completed))?
        }
        _ => return None,
    };
    Some(AlertCoverage {
        target: record.target,
        provider: record.provider,
        runtime_id: record.runtime_id,
        generation: record.generation,
        provider_session_id: record.provider_session_id,
        prompt_id,
        kind,
    })
}

/// Stable UUIDv5 namespace for opaque provider identity projection. The exact
/// provider ID bytes are embedded after NUL separators; IDs are validated to
/// exclude controls and are never interpreted as UUIDs or paths.
fn provider_session_uuid(provider: ProviderKind, session_id: &str) -> Uuid {
    let mut name = Vec::with_capacity(session_id.len() + 40);
    name.extend_from_slice(b"zmux-agent-provider-v1\0");
    name.extend_from_slice(provider.wire_name().as_bytes());
    name.extend_from_slice(b"\0session\0");
    name.extend_from_slice(session_id.as_bytes());
    Uuid::new_v5(&Uuid::NAMESPACE_URL, &name)
}

fn validate_provider_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_PROVIDER_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err("provider session ID is invalid".into());
    }
    Ok(())
}

fn provider_turn_uuid(provider: ProviderKind, session_id: &str, turn_id: &str) -> Uuid {
    let mut name = Vec::with_capacity(session_id.len() + turn_id.len() + 40);
    name.extend_from_slice(b"zmux-agent-provider-v1\0");
    name.extend_from_slice(provider.wire_name().as_bytes());
    name.extend_from_slice(b"\0session\0");
    name.extend_from_slice(session_id.as_bytes());
    name.extend_from_slice(b"\0turn\0");
    name.extend_from_slice(turn_id.as_bytes());
    Uuid::new_v5(&Uuid::NAMESPACE_URL, &name)
}

fn push_bounded(queue: &mut std::collections::VecDeque<Uuid>, value: Uuid) {
    if queue.contains(&value) {
        return;
    }
    queue.push_back(value);
    while queue.len() > MAX_PROVIDER_TURN_MARKS {
        queue.pop_front();
    }
}

fn push_bounded_provider_identity(
    queue: &mut std::collections::VecDeque<ProviderRuntimeGeneration>,
    value: ProviderRuntimeGeneration,
) {
    if queue.contains(&value) {
        return;
    }
    queue.push_back(value);
    while queue.len() > MAX_PROVIDER_RETIRED_IDENTITIES {
        queue.pop_front();
    }
}

fn defers_completion_alert(event: &Event) -> bool {
    matches!(
        event,
        Event::Resumed
            | Event::StopContinued
            | Event::TurnStopped {
                state: TurnStopState::Unknown | TurnStopState::Working,
            }
    )
}

fn remote_hook_lifecycle_event(hook: &ClaudeHook) -> Option<Event> {
    match hook.name.as_str() {
        "UserPromptSubmit" => hook
            .prompt_id
            .map(|prompt_id| Event::PromptSubmitted { prompt_id }),
        "PermissionRequest" | "Elicitation" => Some(Event::WaitingUnconfirmed),
        "Notification" => match hook.notification_type.as_deref()? {
            "permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog" => {
                Some(Event::NeedsUser)
            }
            "idle_prompt" => Some(Event::TurnStopped {
                state: TurnStopState::Idle,
            }),
            "elicitation_complete" | "elicitation_response" => Some(Event::Resumed),
            "agent_needs_input" => Some(Event::WaitingUnconfirmed),
            // A background session notification does not identify this main run.
            "agent_completed" => None,
            _ => None,
        },
        "ElicitationResult" | "PostToolUse" | "PostToolUseFailure" => Some(Event::Resumed),
        "PreToolUse" if hook.tool_name.as_deref() == Some("AskUserQuestion") => {
            Some(Event::NeedsUser)
        }
        "PreToolUse" => Some(Event::Resumed),
        "Stop" if hook.stop_hook_active == Some(true) => Some(Event::StopContinued),
        "Stop" if hook.has_background_tasks => Some(Event::TurnStopped {
            state: TurnStopState::Working,
        }),
        "Stop" | "StopFailure" => Some(Event::TurnStopped {
            state: TurnStopState::Unknown,
        }),
        "SessionEnd" if matches!(hook.reason.as_deref(), Some("logout" | "prompt_input_exit")) => {
            Some(Event::SessionEnded)
        }
        "SessionEnd" => Some(Event::EvidenceLost),
        "SessionStart" => None,
        _ => None,
    }
}

struct Owner {
    window: WindowId,
    workspace: WorkspaceId,
    pane: PaneId,
    surface: SurfaceId,
}

fn owner(model: &Model, target: Target, provider: Provider) -> Option<Owner> {
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
                        || match provider {
                            Provider::ClaudeLocal
                            | Provider::CodexLocal
                            | Provider::OpenCodeLocal => surface.remote.is_some(),
                            Provider::ClaudeRemote
                            | Provider::CodexRemote
                            | Provider::OpenCodeRemote => surface.remote.is_none(),
                        }
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
    owner(model, target, Provider::ClaudeLocal).is_some()
}

pub fn has_remote_owner(model: &Model, target: Target) -> bool {
    owner(model, target, Provider::ClaudeRemote).is_some()
}

pub fn has_local_provider_owner(model: &Model, target: Target, provider: Provider) -> bool {
    !provider.is_remote() && provider.is_normalized() && owner(model, target, provider).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zmux_workspace_core::{Action as WorkspaceAction, WorkspaceStore};

    fn provider_fixture() -> (Model, Target, Target) {
        let namespace = Uuid::new_v4();
        let mut store = WorkspaceStore::with_identity_namespace(namespace);
        store
            .apply(WorkspaceAction::CreateWindow { focus: true })
            .unwrap();
        store
            .apply(WorkspaceAction::CreateSshWorkspace {
                name: "managed SSH provider".into(),
                connection: zmux_remote_runtime::SshConnectionConfig {
                    destination: "fixture@example.test".into(),
                    config_file: None,
                    port: None,
                    identity: None,
                    forward_agent: None,
                    ssh_options: Vec::new(),
                    jump_hosts: Vec::new(),
                    command: None,
                },
                focus: false,
            })
            .unwrap();
        let model = store.snapshot();
        let target = |surface: &zmux_workspace_core::Surface| Target {
            identity_namespace: namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id: surface.session_id.unwrap(),
        };
        let local = model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| &pane.surfaces)
            .find(|surface| surface.remote.is_none())
            .unwrap();
        let remote = model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| &pane.surfaces)
            .find(|surface| surface.remote.is_some())
            .unwrap();
        let local_target = target(local);
        let remote_target = target(remote);
        (model, local_target, remote_target)
    }

    fn provider_observation(
        provider: ProviderKind,
        session_id: &str,
        turn_id: Option<&str>,
        state: ProviderState,
        transition: ProviderTransition,
        marker: ProviderMarker,
    ) -> ProviderLifecycleObservation {
        ProviderLifecycleObservation {
            provider,
            scope: zmux_remote_runtime::agent_lifecycle::ProviderScope::Root,
            session_id: session_id.into(),
            turn_id: turn_id.map(str::to_owned),
            state,
            transition,
            marker,
        }
    }

    #[test]
    fn provider_ids_are_opaque_namespaced_and_transport_owners_are_exact() {
        let (model, local_target, remote_target) = provider_fixture();
        let raw_session = "sess_root_d41d8cd98f00";
        let codex_session = provider_session_uuid(ProviderKind::Codex, raw_session);
        let opencode_session = provider_session_uuid(ProviderKind::OpenCode, raw_session);
        assert_eq!(
            codex_session,
            Uuid::parse_str("56da7807-16be-565d-926b-316ca9782f87").unwrap()
        );
        assert_ne!(codex_session, opencode_session);
        assert_ne!(
            codex_session,
            provider_session_uuid(ProviderKind::Codex, "sess_root_d41d8cd98f00:other")
        );
        assert_ne!(
            provider_turn_uuid(ProviderKind::OpenCode, "sess-a", "turn-a"),
            provider_turn_uuid(ProviderKind::OpenCode, "sess-b", "turn-a")
        );
        assert_eq!(
            provider_turn_uuid(ProviderKind::Codex, raw_session, "turn_1"),
            Uuid::parse_str("98a4ca54-2915-5a99-b867-ba6dfb1d5401").unwrap()
        );
        assert!(Uuid::parse_str(raw_session).is_err());

        assert!(has_local_provider_owner(
            &model,
            local_target,
            Provider::CodexLocal
        ));
        assert!(!has_local_provider_owner(
            &model,
            remote_target,
            Provider::CodexLocal
        ));
        assert!(owner(&model, remote_target, Provider::OpenCodeRemote).is_some());
        assert!(owner(&model, local_target, Provider::OpenCodeRemote).is_none());

        let mut runtime = Runtime::default();
        let now = Instant::now();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        assert!(
            runtime
                .apply_local_provider_observation(
                    &model,
                    ProviderObservationInput {
                        target: remote_target,
                        runtime_id,
                        generation,
                        sequence: 1,
                        observation: provider_observation(
                            ProviderKind::Codex,
                            raw_session,
                            Some("turn_1"),
                            ProviderState::Working,
                            ProviderTransition::StateChanged,
                            ProviderMarker::Live,
                        ),
                    },
                    now,
                )
                .is_err()
        );
        assert!(
            runtime
                .apply_remote_provider_observation(
                    &model,
                    ProviderObservationInput {
                        target: local_target,
                        runtime_id,
                        generation,
                        sequence: 1,
                        observation: provider_observation(
                            ProviderKind::Codex,
                            raw_session,
                            Some("turn_1"),
                            ProviderState::Working,
                            ProviderTransition::StateChanged,
                            ProviderMarker::Live,
                        ),
                    },
                    now,
                )
                .is_err()
        );
        assert!(
            runtime
                .apply(
                    Request {
                        request_id: Uuid::new_v4(),
                        action: Action::Register {
                            target: local_target,
                            provider: Provider::CodexLocal,
                            runtime_id,
                            generation,
                            provider_session_id: codex_session,
                        },
                    },
                    now,
                )
                .is_err()
        );
    }

    #[test]
    fn generic_provider_baselines_are_silent_and_confirmed_run_alerts_are_exact_once() {
        let (model, target, _) = provider_fixture();
        let provider = ProviderKind::OpenCode;
        let session_id = "sess/root:7";
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let mut runtime = Runtime::default();
        let mut sequence = 0;
        let mut apply = |observation| {
            sequence += 1;
            runtime
                .apply_local_provider_observation(
                    &model,
                    ProviderObservationInput {
                        target,
                        runtime_id,
                        generation,
                        sequence,
                        observation,
                    },
                    Instant::now(),
                )
                .unwrap();
        };

        apply(provider_observation(
            provider,
            session_id,
            Some("turn/old"),
            ProviderState::Working,
            ProviderTransition::Snapshot,
            ProviderMarker::Baseline,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn/old"),
            ProviderState::Idle,
            ProviderTransition::TurnCompleted,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:new:1"),
            ProviderState::Working,
            ProviderTransition::StateChanged,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:new:1"),
            ProviderState::Idle,
            ProviderTransition::TurnCompleted,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:new:1"),
            ProviderState::Idle,
            ProviderTransition::TurnCompleted,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:needs-user"),
            ProviderState::Waiting,
            ProviderTransition::StateChanged,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:needs-user"),
            ProviderState::Working,
            ProviderTransition::StateChanged,
            ProviderMarker::Live,
        ));
        apply(provider_observation(
            provider,
            session_id,
            Some("turn:needs-user"),
            ProviderState::Waiting,
            ProviderTransition::StateChanged,
            ProviderMarker::Live,
        ));

        for (turn_id, outcome) in [
            ("turn:failed", ProviderTransition::TurnFailed),
            ("turn:interrupted", ProviderTransition::TurnInterrupted),
        ] {
            apply(provider_observation(
                provider,
                session_id,
                Some(turn_id),
                ProviderState::Working,
                ProviderTransition::StateChanged,
                ProviderMarker::Live,
            ));
            apply(provider_observation(
                provider,
                session_id,
                Some(turn_id),
                ProviderState::Idle,
                outcome,
                ProviderMarker::Live,
            ));
            apply(provider_observation(
                provider,
                session_id,
                Some(turn_id),
                ProviderState::Idle,
                ProviderTransition::TurnCompleted,
                ProviderMarker::Live,
            ));
        }

        let alerts = runtime.alert_events(&model);
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].kind, AlertKind::Completed);
        assert_eq!(
            alerts[0].prompt_id,
            provider_turn_uuid(provider, session_id, "turn:new:1")
        );
        assert_eq!(alerts[1].kind, AlertKind::NeedsUser);
        assert_eq!(
            alerts[1].prompt_id,
            provider_turn_uuid(provider, session_id, "turn:needs-user")
        );
    }

    #[test]
    fn stale_local_provider_sequence_fences_activity_without_alerting() {
        let (model, target, _) = provider_fixture();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let observation = provider_observation(
            ProviderKind::Codex,
            "session-current",
            Some("turn-current"),
            ProviderState::Working,
            ProviderTransition::StateChanged,
            ProviderMarker::Live,
        );
        let now = Instant::now();
        for invalid_sequence in [1, 3] {
            let mut runtime = Runtime::default();
            runtime
                .apply_local_provider_observation(
                    &model,
                    ProviderObservationInput {
                        target,
                        runtime_id,
                        generation,
                        sequence: 1,
                        observation: observation.clone(),
                    },
                    now,
                )
                .unwrap();
            assert!(
                runtime
                    .apply_local_provider_observation(
                        &model,
                        ProviderObservationInput {
                            target,
                            runtime_id,
                            generation,
                            sequence: invalid_sequence,
                            observation: observation.clone(),
                        },
                        now,
                    )
                    .is_err()
            );
            let agent = runtime.snapshot(&model).remove(0);
            assert_eq!(agent.activity, Activity::Unknown);
            assert_eq!(agent.connection, Connection::Unknown);
            assert!(runtime.alert_events(&model).is_empty());
        }
    }

    #[test]
    fn remote_loss_and_reconnect_update_every_provider_session_and_recover_silently() {
        let (model, _, target) = provider_fixture();
        let mut runtime = Runtime::default();
        let runtime_id = Uuid::new_v4();
        let old_generation = Uuid::new_v4();
        let new_generation = Uuid::new_v4();
        let now = Instant::now();
        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Register {
                        target,
                        provider: Provider::ClaudeRemote,
                        runtime_id,
                        generation: old_generation,
                        provider_session_id: Uuid::new_v4(),
                    },
                },
                now,
            )
            .unwrap();
        for (provider, session) in [
            (ProviderKind::Codex, "codex/session/1"),
            (ProviderKind::Codex, "codex/session/2"),
            (ProviderKind::OpenCode, "opencode/session/3"),
        ] {
            runtime
                .apply_remote_provider_observation(
                    &model,
                    ProviderObservationInput {
                        target,
                        runtime_id,
                        generation: old_generation,
                        sequence: 1,
                        observation: provider_observation(
                            provider,
                            session,
                            Some("turn/baseline"),
                            ProviderState::Working,
                            ProviderTransition::Snapshot,
                            ProviderMarker::Baseline,
                        ),
                    },
                    now,
                )
                .unwrap();
        }
        assert_eq!(runtime.snapshot(&model).len(), 4);
        assert!(runtime.remote_transport_lost(target, runtime_id, old_generation));
        assert!(runtime.snapshot(&model).iter().all(|agent| {
            agent.activity == Activity::Unknown && agent.connection == Connection::Unknown
        }));
        assert!(runtime.remote_transport_reconnected(
            target,
            runtime_id,
            old_generation,
            new_generation,
            now + Duration::from_millis(1),
        ));
        let recovered = runtime.snapshot(&model);
        assert_eq!(recovered.len(), 4);
        assert!(recovered.iter().all(|agent| {
            agent.activity == Activity::Unknown
                && agent.connection == Connection::Connected
                && agent.generation == new_generation
        }));
        assert!(!runtime.remote_transport_lost(target, runtime_id, old_generation));
        let freshness_at = now + EVIDENCE_FRESHNESS + Duration::from_secs(1);
        assert!(runtime.poll_freshness(freshness_at));
        assert!(runtime.remote_transport_heartbeat(
            target,
            runtime_id,
            new_generation,
            freshness_at + Duration::from_millis(1),
        ));
        assert!(runtime.snapshot(&model).iter().all(|agent| {
            agent.activity == Activity::Unknown && agent.connection == Connection::Connected
        }));

        let codex_session = "codex/session/1";
        runtime
            .apply_remote_provider_observation(
                &model,
                ProviderObservationInput {
                    target,
                    runtime_id,
                    generation: new_generation,
                    sequence: 2,
                    observation: provider_observation(
                        ProviderKind::Codex,
                        codex_session,
                        Some("turn/baseline"),
                        ProviderState::Working,
                        ProviderTransition::Snapshot,
                        ProviderMarker::Recovery,
                    ),
                },
                freshness_at + Duration::from_secs(1),
            )
            .unwrap();
        runtime
            .apply_remote_provider_observation(
                &model,
                ProviderObservationInput {
                    target,
                    runtime_id,
                    generation: new_generation,
                    sequence: 3,
                    observation: provider_observation(
                        ProviderKind::Codex,
                        codex_session,
                        Some("turn/baseline"),
                        ProviderState::Idle,
                        ProviderTransition::TurnCompleted,
                        ProviderMarker::Live,
                    ),
                },
                freshness_at + Duration::from_secs(2),
            )
            .unwrap();
        assert!(runtime.alert_events(&model).is_empty());
        runtime
            .apply_remote_provider_observation(
                &model,
                ProviderObservationInput {
                    target,
                    runtime_id,
                    generation: new_generation,
                    sequence: 4,
                    observation: provider_observation(
                        ProviderKind::Codex,
                        codex_session,
                        Some("turn/live-after-heartbeat"),
                        ProviderState::Working,
                        ProviderTransition::StateChanged,
                        ProviderMarker::Live,
                    ),
                },
                freshness_at + Duration::from_secs(3),
            )
            .unwrap();
        let _ = runtime.remote_transport_heartbeat(
            target,
            runtime_id,
            new_generation,
            freshness_at + Duration::from_secs(4),
        );
        // A delayed provider baseline can refresh the same turn's state, but
        // cannot clear completion eligibility established by live evidence.
        runtime
            .apply_remote_provider_observation(
                &model,
                ProviderObservationInput {
                    target,
                    runtime_id,
                    generation: new_generation,
                    sequence: 6,
                    observation: provider_observation(
                        ProviderKind::Codex,
                        codex_session,
                        Some("turn/live-after-heartbeat"),
                        ProviderState::Working,
                        ProviderTransition::Snapshot,
                        ProviderMarker::Baseline,
                    ),
                },
                freshness_at + Duration::from_millis(4500),
            )
            .unwrap();
        let _ = runtime.remote_transport_heartbeat(
            target,
            runtime_id,
            new_generation,
            freshness_at + Duration::from_millis(4750),
        );
        runtime
            .apply_remote_provider_observation(
                &model,
                ProviderObservationInput {
                    target,
                    runtime_id,
                    generation: new_generation,
                    sequence: 8,
                    observation: provider_observation(
                        ProviderKind::Codex,
                        codex_session,
                        Some("turn/live-after-heartbeat"),
                        ProviderState::Idle,
                        ProviderTransition::TurnCompleted,
                        ProviderMarker::Live,
                    ),
                },
                freshness_at + Duration::from_secs(5),
            )
            .unwrap();
        let alerts = runtime.alert_events(&model);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Completed);
        assert_eq!(
            alerts[0].prompt_id,
            provider_turn_uuid(
                ProviderKind::Codex,
                codex_session,
                "turn/live-after-heartbeat"
            )
        );
    }

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
                Some(Event::StopContinued),
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
                        hook_id: None,
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
                        hook_id: None,
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
                        hook_id: None,
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

    #[test]
    fn explicit_idle_completes_a_tracked_prompt_after_unknown_and_not_on_startup() {
        struct PromptSubmission {
            sequence: u64,
            prompt_id: Uuid,
            now: Instant,
        }

        fn setup() -> (Runtime, Model, Target, Uuid, Uuid, Uuid, Instant) {
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
            let started_at = Instant::now();
            let mut runtime = Runtime::default();
            runtime
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
                    started_at,
                )
                .unwrap();
            (
                runtime,
                model,
                target,
                provider_session_id,
                runtime_id,
                generation,
                started_at,
            )
        }

        fn submit(
            runtime: &mut Runtime,
            target: Target,
            provider_session_id: Uuid,
            runtime_id: Uuid,
            generation: Uuid,
            submission: PromptSubmission,
        ) {
            let capability = runtime
                .records
                .iter()
                .find(|record| {
                    record.target == target
                        && record.provider_session_id == provider_session_id
                        && record.runtime_id == runtime_id
                        && record.generation == generation
                })
                .unwrap()
                .capability;
            runtime
                .apply(
                    Request {
                        request_id: Uuid::new_v4(),
                        action: Action::Event {
                            capability,
                            runtime_id,
                            generation,
                            sequence: submission.sequence,
                            hook_id: None,
                            event: Event::PromptSubmitted {
                                prompt_id: submission.prompt_id,
                            },
                        },
                    },
                    submission.now,
                )
                .unwrap();
        }

        fn idle(
            runtime: &mut Runtime,
            target: Target,
            provider_session_id: Uuid,
            runtime_id: Uuid,
            generation: Uuid,
            sequence: u64,
            now: Instant,
        ) {
            let capability = runtime
                .records
                .iter()
                .find(|record| {
                    record.target == target
                        && record.provider_session_id == provider_session_id
                        && record.runtime_id == runtime_id
                        && record.generation == generation
                })
                .unwrap()
                .capability;
            runtime
                .apply(
                    Request {
                        request_id: Uuid::new_v4(),
                        action: Action::Event {
                            capability,
                            runtime_id,
                            generation,
                            sequence,
                            hook_id: None,
                            event: Event::TurnStopped {
                                state: TurnStopState::Idle,
                            },
                        },
                    },
                    now,
                )
                .unwrap();
        }

        // An explicit idle signal with no admitted user prompt cannot create a
        // completion alert. Once a prompt is accepted, that same signal can.
        let (mut runtime, model, target, provider_session_id, runtime_id, generation, t0) = setup();
        let capability = runtime.records[0].capability;
        assert!(
            runtime
                .apply(
                    Request {
                        request_id: Uuid::new_v4(),
                        action: Action::Event {
                            capability,
                            runtime_id,
                            generation,
                            sequence: 1,
                            hook_id: None,
                            event: Event::TurnStopped {
                                state: TurnStopState::Idle,
                            },
                        },
                    },
                    t0,
                )
                .is_err()
        );
        assert_eq!(runtime.alert_cursor(), 0);
        let prompt_id = Uuid::new_v4();
        submit(
            &mut runtime,
            target,
            provider_session_id,
            runtime_id,
            generation,
            PromptSubmission {
                sequence: 1,
                prompt_id,
                now: t0,
            },
        );
        idle(
            &mut runtime,
            target,
            provider_session_id,
            runtime_id,
            generation,
            2,
            t0 + Duration::from_millis(1),
        );
        assert_eq!(runtime.alert_events(&model).len(), 1);

        // Stop does not establish completion. A later idle_prompt does, even
        // though the Stop transition deliberately made activity Unknown.
        let (mut after_stop, model, target, provider_session_id, runtime_id, generation, t0) =
            setup();
        let prompt_id = Uuid::new_v4();
        submit(
            &mut after_stop,
            target,
            provider_session_id,
            runtime_id,
            generation,
            PromptSubmission {
                sequence: 1,
                prompt_id,
                now: t0,
            },
        );
        let capability = after_stop.records[0].capability;
        after_stop
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation,
                        sequence: 2,
                        hook_id: None,
                        event: Event::TurnStopped {
                            state: TurnStopState::Unknown,
                        },
                    },
                },
                t0 + Duration::from_millis(1),
            )
            .unwrap();
        idle(
            &mut after_stop,
            target,
            provider_session_id,
            runtime_id,
            generation,
            3,
            t0 + Duration::from_millis(2),
        );
        idle(
            &mut after_stop,
            target,
            provider_session_id,
            runtime_id,
            generation,
            4,
            t0 + Duration::from_millis(3),
        );
        let alerts = after_stop.alert_events(&model);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Completed);

        // Freshness expiry also changes only presentation to Unknown; it does
        // not erase the admitted working run needed to accept explicit idle.
        let (mut after_expiry, model, target, provider_session_id, runtime_id, generation, t0) =
            setup();
        let prompt_id = Uuid::new_v4();
        submit(
            &mut after_expiry,
            target,
            provider_session_id,
            runtime_id,
            generation,
            PromptSubmission {
                sequence: 1,
                prompt_id,
                now: t0,
            },
        );
        let expired_at = t0 + EVIDENCE_FRESHNESS + Duration::from_secs(1);
        assert!(after_expiry.poll_freshness(expired_at));
        assert_eq!(after_expiry.snapshot(&model)[0].activity, Activity::Unknown);
        idle(
            &mut after_expiry,
            target,
            provider_session_id,
            runtime_id,
            generation,
            2,
            expired_at + Duration::from_millis(1),
        );
        assert_eq!(after_expiry.alert_events(&model).len(), 1);
    }

    #[test]
    fn remote_reconnect_changes_transport_epoch_and_requires_a_fresh_prompt() {
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
        let mut runtime = Runtime::default();
        let runtime_id = Uuid::new_v4();
        let old_generation = Uuid::new_v4();
        let provider_session_id = Uuid::new_v4();
        let capability = runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Register {
                        target,
                        provider: Provider::ClaudeRemote,
                        runtime_id,
                        generation: old_generation,
                        provider_session_id,
                    },
                },
                Instant::now(),
            )
            .unwrap()
            .capability
            .unwrap();
        let prompt_id = Uuid::new_v4();
        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation: old_generation,
                        sequence: 1,
                        hook_id: None,
                        event: Event::PromptSubmitted { prompt_id },
                    },
                },
                Instant::now(),
            )
            .unwrap();
        let new_generation = Uuid::new_v4();
        let third_generation = Uuid::new_v4();
        let nonce = Uuid::new_v4();
        let old_binding = crate::remote_agent_lifecycle::Binding {
            target,
            launch_id: 7,
            runtime_id,
            generation: old_generation,
            nonce,
        };
        let new_binding = crate::remote_agent_lifecycle::Binding {
            generation: new_generation,
            ..old_binding
        };
        let third_binding = crate::remote_agent_lifecycle::Binding {
            generation: third_generation,
            ..old_binding
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(6);
        let heartbeat = |binding, sequence| crate::remote_agent_lifecycle::Message::Frame {
            binding,
            frame: zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat {
                version: zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION,
                nonce,
                generation: binding.generation,
                sequence,
            },
        };
        sender
            .try_send(crate::remote_agent_lifecycle::Message::Lost {
                binding: old_binding,
            })
            .unwrap();
        sender
            .try_send(crate::remote_agent_lifecycle::Message::Connected {
                binding: new_binding,
                previous_generation: Some(old_generation),
            })
            .unwrap();
        sender.try_send(heartbeat(new_binding, 1)).unwrap();
        sender
            .try_send(crate::remote_agent_lifecycle::Message::Lost {
                binding: new_binding,
            })
            .unwrap();
        sender
            .try_send(crate::remote_agent_lifecycle::Message::Connected {
                binding: third_binding,
                previous_generation: Some(new_generation),
            })
            .unwrap();
        sender.try_send(heartbeat(third_binding, 1)).unwrap();

        // The supervisor has already advanced its shared binding to G3, but
        // the worker has not drained any of the six ordered messages yet.
        let latest_binding = third_binding;
        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 6);
        for message in messages {
            match message {
                crate::remote_agent_lifecycle::Message::Lost { binding } => {
                    assert!(latest_binding.same_owner(binding));
                    assert!(runtime.remote_transport_lost(
                        binding.target,
                        binding.runtime_id,
                        binding.generation
                    ))
                }
                crate::remote_agent_lifecycle::Message::Connected {
                    binding,
                    previous_generation: Some(previous_generation),
                } => {
                    assert!(latest_binding.same_owner(binding));
                    assert!(
                        binding.generation == new_generation
                            || binding.generation == latest_binding.generation
                    );
                    assert!(runtime.remote_transport_reconnected(
                        binding.target,
                        binding.runtime_id,
                        previous_generation,
                        binding.generation,
                        Instant::now(),
                    ));
                }
                crate::remote_agent_lifecycle::Message::Frame { binding, frame } => {
                    if !latest_binding.is_current_generation(binding) {
                        assert_eq!(binding, new_binding);
                        assert!(matches!(
                            frame,
                            zmux_remote_runtime::agent_lifecycle::Frame::Heartbeat { .. }
                        ));
                        continue;
                    }
                    assert_eq!(binding, third_binding);
                    runtime.remote_transport_heartbeat(
                        binding.target,
                        binding.runtime_id,
                        binding.generation,
                        Instant::now(),
                    );
                }
                _ => panic!("unexpected lifecycle transition in reconnect interleaving"),
            }
        }
        let record = &runtime.records[0];
        assert_eq!(record.generation, third_generation);
        assert_eq!(record.transport_epoch, Some(third_generation));
        assert_eq!(record.activity, Activity::Unknown);
        assert_eq!(record.connection, Connection::Connected);
        assert_eq!(record.prompt_id, None);
        assert_eq!(record.working_prompt_id, None);
        assert_eq!(record.last_sequence, 0);
        assert_eq!(runtime.alert_cursor(), 0);

        let idle = runtime.apply(
            Request {
                request_id: Uuid::new_v4(),
                action: Action::Event {
                    capability,
                    runtime_id,
                    generation: new_generation,
                    sequence: 1,
                    hook_id: None,
                    event: Event::TurnStopped {
                        state: TurnStopState::Idle,
                    },
                },
            },
            Instant::now(),
        );
        assert!(idle.is_err());
        assert_eq!(runtime.alert_cursor(), 0);

        let fresh_prompt_id = Uuid::new_v4();
        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation: third_generation,
                        sequence: 1,
                        hook_id: None,
                        event: Event::PromptSubmitted {
                            prompt_id: fresh_prompt_id,
                        },
                    },
                },
                Instant::now(),
            )
            .unwrap();
        runtime
            .apply(
                Request {
                    request_id: Uuid::new_v4(),
                    action: Action::Event {
                        capability,
                        runtime_id,
                        generation: third_generation,
                        sequence: 2,
                        hook_id: None,
                        event: Event::TurnStopped {
                            state: TurnStopState::Idle,
                        },
                    },
                },
                Instant::now(),
            )
            .unwrap();
        assert_eq!(runtime.alert_cursor(), 1);
    }
}
