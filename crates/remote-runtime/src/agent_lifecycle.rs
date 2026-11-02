//! Bounded provider lifecycle frames carried over a managed SSH operation.
//!
//! The SSH channel authenticates the remote host/account route selected by the
//! existing OpenSSH ControlMaster. It does not authenticate the Claude process
//! or provide a provider replay cursor.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const AGENT_LIFECYCLE_PROTOCOL_VERSION: u8 = 3;
pub const MAX_AGENT_LIFECYCLE_FRAME_BYTES: usize = 4_096;
pub const MAX_PENDING_HOOK_RECEIPTS: usize = 64;
/// Maximum UTF-8 bytes accepted for an opaque provider session or turn ID.
pub const MAX_PROVIDER_ID_BYTES: usize = 256;

/// Providers with lifecycle adapters that report their own opaque IDs.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl ProviderKind {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

/// Provider-reported activity. This is not inferred from terminal output.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Working,
    Waiting,
    Idle,
}

/// Root-session boundary carried from the provider adapter.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderScope {
    Root,
    Subagent,
}

/// Meaning of this observation relative to previously admitted provider state.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTransition {
    Snapshot,
    StateChanged,
    TurnCompleted,
    TurnFailed,
    TurnInterrupted,
}

/// Snapshot/reconnect markers suppress alerts and cannot establish a new run.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMarker {
    Live,
    Baseline,
    Recovery,
}

/// Sanitized, provider-tagged state shared by local normalization and the
/// managed SSH lifecycle protocol. IDs are opaque bounded UTF-8 strings; the
/// desktop maps them into provider-namespaced UUIDv5 values for its public API.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderLifecycleObservation {
    pub provider: ProviderKind,
    pub scope: ProviderScope,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub state: ProviderState,
    pub transition: ProviderTransition,
    pub marker: ProviderMarker,
}

impl ProviderLifecycleObservation {
    /// Validate the bounded, provider-neutral wire contract before admission.
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_provider_id(&self.session_id).map_err(|_| "provider session ID is invalid")?;
        if let Some(turn_id) = self.turn_id.as_deref() {
            validate_provider_id(turn_id).map_err(|_| "provider turn ID is invalid")?;
        }
        if self.scope != ProviderScope::Root {
            return Err("provider lifecycle observation is not for the root session");
        }
        if matches!(self.state, ProviderState::Working | ProviderState::Waiting)
            && self.turn_id.is_none()
        {
            return Err("working or waiting provider state has no turn ID");
        }
        if matches!(
            self.transition,
            ProviderTransition::TurnCompleted
                | ProviderTransition::TurnFailed
                | ProviderTransition::TurnInterrupted
        ) && (self.state != ProviderState::Idle || self.turn_id.is_none())
        {
            return Err("provider turn outcome requires an idle state and turn ID");
        }
        Ok(())
    }
}

fn validate_provider_id(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > MAX_PROVIDER_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_state_frame_round_trips_with_bounded_root_identity() {
        let nonce = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let decoded: Frame = serde_json::from_value(serde_json::json!({
            "kind": "provider_state",
            "version": AGENT_LIFECYCLE_PROTOCOL_VERSION,
            "nonce": nonce,
            "generation": generation,
            "sequence": 7,
            "observation": {
                "provider": "opencode",
                "scope": "root",
                "session_id": "sess_root_7",
                "turn_id": "turn:8",
                "state": "idle",
                "transition": "turn_completed",
                "marker": "live"
            }
        }))
        .unwrap();
        assert!(matches!(
            &decoded,
            Frame::ProviderState {
                version: AGENT_LIFECYCLE_PROTOCOL_VERSION,
                sequence: 7,
                ..
            }
        ));
        if let Frame::ProviderState { observation, .. } = decoded {
            assert_eq!(observation.session_id, "sess_root_7");
            assert_eq!(observation.turn_id.as_deref(), Some("turn:8"));
            assert_eq!(observation.validate(), Ok(()));
        }
    }

    #[test]
    fn provider_state_rejects_subagents_missing_turns_and_unbounded_ids() {
        let mut observation = ProviderLifecycleObservation {
            provider: ProviderKind::Codex,
            scope: ProviderScope::Root,
            session_id: "codex-session".into(),
            turn_id: Some("turn-1".into()),
            state: ProviderState::Working,
            transition: ProviderTransition::StateChanged,
            marker: ProviderMarker::Live,
        };
        assert_eq!(observation.validate(), Ok(()));

        observation.scope = ProviderScope::Subagent;
        assert!(observation.validate().is_err());
        observation.scope = ProviderScope::Root;
        observation.turn_id = None;
        assert!(observation.validate().is_err());
        observation.turn_id = Some("t".repeat(MAX_PROVIDER_ID_BYTES));
        observation.session_id = "s".repeat(MAX_PROVIDER_ID_BYTES);
        assert_eq!(observation.validate(), Ok(()));

        for invalid_id in [
            String::new(),
            "control\ncharacter".into(),
            "x".repeat(MAX_PROVIDER_ID_BYTES + 1),
        ] {
            observation.session_id = invalid_id.clone();
            assert!(observation.validate().is_err());
            observation.session_id = "codex-session".into();
            observation.turn_id = Some(invalid_id);
            assert!(observation.validate().is_err());
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDisposition {
    /// The local owner created or already has the exact semantic alert.
    AlertCovered,
    /// The local owner admitted this exact hook and intentionally deferred its
    /// alert decision to a later provider event, such as `idle_prompt` after
    /// `Stop`.
    HandledNoAlert,
    /// No matching owner alert covers this hook; render its silent fallback.
    NotHandled,
}

/// Frames sent by the local owner back to the remote lifecycle stream.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    Activate {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
    },
    Receipt {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
        /// Zmux-generated per-hook correlation UUID, not a provider event ID.
        event_id: Uuid,
        disposition: HookDisposition,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    Hello {
        version: u8,
        nonce: Uuid,
    },
    Active {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
    },
    Heartbeat {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
        sequence: u64,
    },
    Hook {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
        sequence: u64,
        /// Zmux-generated per-hook correlation UUID, not a provider replay cursor.
        event_id: Uuid,
        receipt_requested: bool,
        event: Box<ClaudeHook>,
    },
    ProviderState {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
        sequence: u64,
        observation: Box<ProviderLifecycleObservation>,
    },
    Gap {
        version: u8,
        nonce: Uuid,
        generation: Uuid,
        sequence: u64,
    },
}

impl Frame {
    pub fn version(&self) -> u8 {
        match self {
            Self::Hello { version, .. }
            | Self::Active { version, .. }
            | Self::Heartbeat { version, .. }
            | Self::Hook { version, .. }
            | Self::ProviderState { version, .. }
            | Self::Gap { version, .. } => *version,
        }
    }

    pub fn nonce(&self) -> Uuid {
        match self {
            Self::Hello { nonce, .. }
            | Self::Active { nonce, .. }
            | Self::Heartbeat { nonce, .. }
            | Self::Hook { nonce, .. }
            | Self::ProviderState { nonce, .. }
            | Self::Gap { nonce, .. } => *nonce,
        }
    }

    pub fn sequence(&self) -> Option<u64> {
        match self {
            Self::Hello { .. } | Self::Active { .. } => None,
            Self::Heartbeat { sequence, .. }
            | Self::Hook { sequence, .. }
            | Self::ProviderState { sequence, .. }
            | Self::Gap { sequence, .. } => Some(*sequence),
        }
    }

    pub fn generation(&self) -> Option<Uuid> {
        match self {
            Self::Hello { .. } => None,
            Self::Active { generation, .. }
            | Self::Heartbeat { generation, .. }
            | Self::Hook { generation, .. }
            | Self::ProviderState { generation, .. }
            | Self::Gap { generation, .. } => Some(*generation),
        }
    }
}

/// Sanitized fields only. The remote shell helper drops prompts, tool input,
/// transcript text, and arbitrary provider payload fields before transport.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaudeHook {
    pub name: String,
    pub session_id: Uuid,
    pub main_session: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub prompt_id: Option<Uuid>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub notification_type: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub stop_hook_active: Option<bool>,
    #[serde(default)]
    pub has_background_tasks: bool,
}
