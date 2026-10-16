//! Bounded foreground lifecycle wire contract shared by CLI and native host.
//! A reservation acknowledgment asserts a durable revision; its construction and
//! the actual persistence checkpoint belong to the native coordinator.
use super::{Record, broker::TransportReport};
use serde::{Deserialize, Serialize};
use std::{io, os::unix::net::UnixStream, path::PathBuf};
use uuid::Uuid;
use zmux_session_runtime::child::{ExitObservation, ProcessIdentity};

pub const ENV: &str = "ZMUX_LOCAL_ZELLIJ_OBSERVER";
pub const FRAME_LIMIT: usize = 32768;
pub const MESSAGE_LIMIT: u64 = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub installation_epoch: Uuid,
    pub profile_id: Uuid,
    pub operation_id: u64,
    pub identity_namespace: Uuid,
    pub session_id: Uuid,
    pub surface_id: Uuid,
    pub launch_generation: u64,
}
impl Context {
    pub fn validate(&self) -> io::Result<()> {
        if [
            self.installation_epoch,
            self.profile_id,
            self.identity_namespace,
            self.session_id,
            self.surface_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || self.operation_id == 0
            || self.launch_generation == 0
        {
            return Err(super::fail("invalid observer launch correlation"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u8,
    pub socket: PathBuf,
    pub launcher: ProcessIdentity,
    pub context: Context,
    pub nonce: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgment {
    pub version: u8,
    pub context: Context,
    pub nonce: String,
    pub sequence: u64,
    pub event: String,
    pub owner_uuid: Uuid,
    pub document_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_generation: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "receipt",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OwnerOutcome {
    LivePreserved,
    WorkloadCompleted(Box<super::completion::TerminalReceiptV1>),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrontendReceipt {
    pub owned_client_exit: ExitObservation,
    pub requested_cancel: bool,
    pub original_owner_preserved: bool,
    pub workers_joined: bool,
    pub transport: TransportReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_outcome: Option<OwnerOutcome>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    WorkloadFinished {
        owner_uuid: Uuid,
        receipt: super::completion::TerminalReceiptV1,
    },
    OwnerReserved {
        owner_uuid: Uuid,
    },
    OwnerBound {
        record: Record,
    },
    TransportConnected {
        owner_uuid: Uuid,
        frontend: ProcessIdentity,
    },
    FrontendFinished {
        owner_uuid: Uuid,
        #[serde(flatten)]
        receipt: FrontendReceipt,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Frame {
    pub version: u8,
    pub context: Context,
    pub nonce: String,
    pub sequence: u64,
    #[serde(flatten)]
    pub event: Event,
}
impl Frame {
    /// Check the flattened envelope explicitly: serde cannot enforce unknown
    /// fields on an outer structure containing a flattened tagged event.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
            return Err(super::fail("observer frame exceeded bound"));
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        let object = value
            .as_object()
            .ok_or_else(|| super::fail("observer frame is not an object"))?;
        let fields: &[&str] = match object.get("event").and_then(|v| v.as_str()) {
            Some("workload_finished") => &["owner_uuid", "receipt"],
            Some("owner_reserved") => &["owner_uuid"],
            Some("owner_bound") => &["record"],
            Some("transport_connected") => &["owner_uuid", "frontend"],
            Some("frontend_finished") => &[
                "owner_uuid",
                "owned_client_exit",
                "requested_cancel",
                "original_owner_preserved",
                "workers_joined",
                "transport",
            ],
            _ => return Err(super::fail("unknown observer lifecycle event")),
        };
        let common = ["version", "context", "nonce", "sequence", "event"];
        let optional_outcome = object.get("event").and_then(|v| v.as_str())
            == Some("frontend_finished")
            && object.contains_key("owner_outcome");
        if object.len() != common.len() + fields.len() + usize::from(optional_outcome)
            || object.keys().any(|key| {
                !(common.contains(&key.as_str())
                    || fields.contains(&key.as_str())
                    || optional_outcome && key == "owner_outcome")
            })
        {
            return Err(super::fail("unknown or missing observer frame field"));
        }
        #[derive(Deserialize)]
        struct WireFrame {
            version: u8,
            context: Context,
            nonce: String,
            sequence: u64,
            #[serde(flatten)]
            event: Event,
        }
        let wire: WireFrame = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        Ok(Self {
            version: wire.version,
            context: wire.context,
            nonce: wire.nonce,
            sequence: wire.sequence,
            event: wire.event,
        })
    }
}

/// Kernel credentials and full live identity; never grants process signal rights.
pub fn peer_identity(stream: &UnixStream) -> io::Result<ProcessIdentity> {
    super::broker::peer(stream)
}

/// The canonical short, sticky shared base used by the foreground producer.
pub fn runtime_base() -> io::Result<PathBuf> {
    super::files::runtime_base()
}
