//! Explicit foreground CLI consumer. Durable authorization is owned by S01.
mod namespace;
mod worker;

use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{Receiver, SyncSender},
    },
    time::Instant,
};
use uuid::Uuid;
use zmux_remote_runtime::local_zellij::{Record, observer_protocol::FrontendReceipt};
use zmux_session_runtime::{EngineOptions, Session, SessionError, child::ProcessIdentity};
use zmux_workspace_core::LocalZellijIdentity;

pub(crate) use worker::launch_client;
pub use zmux_remote_runtime::local_zellij::observer_protocol::Context as Correlation;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub epoch: Uuid,
    pub cli: PathBuf,
    pub zellij: PathBuf,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: Uuid,
    pub root: PathBuf,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub installation: Installation,
    pub profiles: Vec<Profile>,
}
impl Configuration {
    /// Shape only. Filesystem and executable inspection stay on the launch worker.
    pub fn validate(&self) -> Result<(), String> {
        if self.installation.epoch.is_nil()
            || !self.installation.cli.is_absolute()
            || !self.installation.zellij.is_absolute()
            || self.profiles.is_empty()
            || self.profiles.len() > 8
        {
            return Err("invalid explicit local Zellij installation/profile configuration".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut roots = std::collections::HashSet::new();
        for profile in &self.profiles {
            if profile.id.is_nil()
                || !profile.root.is_absolute()
                || !ids.insert(profile.id)
                || !roots.insert(&profile.root)
            {
                return Err(
                    "local Zellij profiles require distinct nonzero IDs and absolute roots".into(),
                );
            }
        }
        Ok(())
    }
}

pub(crate) enum LaunchOperation {
    Start {
        name: String,
        cwd: PathBuf,
        command: Option<String>,
    },
    Attach {
        identity: LocalZellijIdentity,
    },
}
pub(crate) struct LaunchRequest {
    pub correlation: Correlation,
    pub installation: Installation,
    pub profile: Profile,
    pub operation: LaunchOperation,
    pub startup_deadline: Instant,
    pub cancellation: Arc<AtomicBool>,
    pub engine: Option<EngineOptions>,
    pub environment: Vec<(String, String)>,
}
pub(crate) enum LaunchEvent {
    OwnerReserved {
        correlation: Correlation,
        owner_uuid: Uuid,
    },
}
/// Created only by the coordinator after exact successful durable Saved evidence.
/// This value is neither a public command nor a deserializable protocol request.
pub(crate) struct CheckpointSaved {
    pub correlation: Correlation,
    pub owner_uuid: Uuid,
    pub document_revision: u64,
}
pub(crate) type EventSender = SyncSender<LaunchEvent>;
pub(crate) type AuthorizationReceiver = Receiver<CheckpointSaved>;

#[derive(Clone, Debug, Default)]
pub(crate) struct Observation {
    pub final_receipt: Option<FrontendReceipt>,
    pub error: Option<String>,
    pub monitor_joined: bool,
    pub namespace_cleaned: bool,
}
#[derive(Clone)]
pub(crate) struct ObserverHandle {
    state: Arc<std::sync::Mutex<Observation>>,
}
impl ObserverHandle {
    pub fn snapshot(&self) -> Result<Observation, SessionError> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| SessionError::Failure("local Zellij observation state poisoned".into()))
    }
}
pub(crate) struct ClientReady {
    pub session: Session,
    pub observer: ObserverHandle,
    pub record: Record,
    pub frontend: ProcessIdentity,
}
pub(crate) struct ClientLaunchFailure {
    pub error: SessionError,
    pub cleanup: Option<Session>,
    pub observer: Option<ObserverHandle>,
}
