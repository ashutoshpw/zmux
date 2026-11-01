//! Bounded Claude lifecycle frames carried over a managed SSH operation.
//!
//! The SSH channel authenticates the remote host/account route selected by the
//! existing OpenSSH ControlMaster. It does not authenticate the Claude process
//! or provide a provider replay cursor.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const AGENT_LIFECYCLE_PROTOCOL_VERSION: u8 = 3;
pub const MAX_AGENT_LIFECYCLE_FRAME_BYTES: usize = 4_096;
pub const MAX_PENDING_HOOK_RECEIPTS: usize = 64;

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
            | Self::Gap { version, .. } => *version,
        }
    }

    pub fn nonce(&self) -> Uuid {
        match self {
            Self::Hello { nonce, .. }
            | Self::Active { nonce, .. }
            | Self::Heartbeat { nonce, .. }
            | Self::Hook { nonce, .. }
            | Self::Gap { nonce, .. } => *nonce,
        }
    }

    pub fn sequence(&self) -> Option<u64> {
        match self {
            Self::Hello { .. } | Self::Active { .. } => None,
            Self::Heartbeat { sequence, .. }
            | Self::Hook { sequence, .. }
            | Self::Gap { sequence, .. } => Some(*sequence),
        }
    }

    pub fn generation(&self) -> Option<Uuid> {
        match self {
            Self::Hello { .. } => None,
            Self::Active { generation, .. }
            | Self::Heartbeat { generation, .. }
            | Self::Hook { generation, .. }
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
