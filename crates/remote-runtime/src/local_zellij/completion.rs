//! A finite result belongs to one originally bound owner lifetime. Durable bytes
//! alone are storage consistency; authority requires the same live supervisor.
use super::{Record, fail, files::NamespaceIdentity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;
use zmux_session_runtime::child::ProcessIdentity;

pub const TERMINAL_LIMIT: usize = 8192;
pub const CONTROL_LIMIT: usize = 16384;
pub const DOMAIN: &str = "zmux.local-zellij.terminal.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedBinding {
    pub version: u8,
    pub owner_uuid: Uuid,
    pub owner_generation: Uuid,
    pub namespace: NamespaceIdentity,
    pub completion_socket: (u64, u64),
    pub server: ProcessIdentity,
    pub supervisor: ProcessIdentity,
    pub workload: ProcessIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_contract: Option<super::admission::ExecutionContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<Box<super::admission::ExecutionBindingV2>>,
    pub owner_generation: Uuid,
    pub completion_socket: (u64, u64),
    pub binding_digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadExit {
    Exited(i32),
    Signaled(i32),
}
impl WorkloadExit {
    pub fn validate(self) -> io::Result<()> {
        match self {
            Self::Exited(code) if (0..=255).contains(&code) => Ok(()),
            Self::Signaled(signal) if (1..=127).contains(&signal) => Ok(()),
            _ => Err(super::fail("invalid terminal workload exit status")),
        }
    }
    pub fn mapped(self) -> io::Result<i32> {
        self.validate()?;
        Ok(match self {
            Self::Exited(code) => code,
            Self::Signaled(signal) => 128 + signal,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalReceiptV1 {
    pub version: u8,
    pub domain: String,
    pub binding: CommittedBinding,
    pub binding_digest: String,
    pub terminal_sequence: u64,
    pub workload_exit: WorkloadExit,
    pub child_reaped: bool,
}
fn bytes<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > TERMINAL_LIMIT {
        return Err(super::fail("terminal receipt exceeded8KiB"));
    }
    Ok(bytes)
}
fn digest<T: Serialize>(value: &T) -> io::Result<String> {
    Ok(format!("{:x}", Sha256::digest(bytes(value)?)))
}
fn identity_shape(identity: &ProcessIdentity) -> bool {
    identity.pid > 0
        && identity.parent_pid > 0
        && identity.start != [0, 0]
        && Uuid::parse_str(&identity.boot).is_ok_and(|id| !id.is_nil())
        && identity.executable.is_absolute()
        && identity
            .executable
            .to_str()
            .is_some_and(|s| s.len() <= 4096)
}
impl CommittedBinding {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || self.owner_uuid.is_nil()
            || self.owner_generation.is_nil()
            || ![&self.server, &self.supervisor, &self.workload]
                .iter()
                .all(|i| identity_shape(i))
            || self.supervisor.parent_pid != self.server.pid
            || self.workload.parent_pid != self.supervisor.pid
            || self.supervisor.uid != self.server.uid
            || self.workload.uid != self.server.uid
            || self.server.boot != self.supervisor.boot
            || self.workload.boot != self.supervisor.boot
            || [
                self.namespace.root,
                self.namespace.socket,
                self.completion_socket,
            ]
            .iter()
            .any(|(_, inode)| *inode == 0)
        {
            return Err(super::fail("invalid committed terminal owner binding"));
        }
        bytes(self)?;
        Ok(())
    }
    pub fn digest(&self) -> io::Result<String> {
        self.validate()?;
        digest(self)
    }
}
impl TerminalReceiptV1 {
    pub fn validate(&self) -> io::Result<()> {
        self.binding.validate()?;
        self.workload_exit.validate()?;
        if self.version != 1
            || self.domain != DOMAIN
            || self.terminal_sequence != 1
            || !self.child_reaped
            || self.binding_digest != self.binding.digest()?
        {
            return Err(super::fail("invalid terminal completion receipt"));
        }
        bytes(self)?;
        Ok(())
    }
    pub fn digest(&self) -> io::Result<String> {
        self.validate()?;
        digest(self)
    }
    pub fn matches(&self, record: &Record) -> io::Result<()> {
        self.validate()?;
        if self.binding != record.committed_binding()? {
            return Err(super::fail(
                "terminal receipt differs from original owner binding",
            ));
        }
        Ok(())
    }
}

/// Query the original live supervisor; durable bytes alone never grant authority.
pub fn authenticate(
    record: &Record,
    deadline: std::time::Instant,
) -> io::Result<Option<TerminalReceipt>> {
    super::completion_control::authenticate(record, deadline)
}

pub use super::admission::{ExecutionBindingV2, ExecutionObservation, TerminalReceiptV2};

/// Legacy serialization remains raw V1; V2 has one explicit, exclusive envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TerminalReceipt {
    V1(TerminalReceiptV1),
    V2(#[serde(with = "admitted_wire")] TerminalReceiptV2),
}
mod admitted_wire {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire<T> {
        execution_contract: super::super::admission::ExecutionContract,
        receipt: T,
    }
    pub fn serialize<S: serde::Serializer>(
        value: &TerminalReceiptV2,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            execution_contract: super::super::admission::ExecutionContract::AdmittedLifetimeV2,
            receipt: value,
        }
        .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        decoder: D,
    ) -> Result<TerminalReceiptV2, D::Error> {
        Ok(Wire::<TerminalReceiptV2>::deserialize(decoder)?.receipt)
    }
}
impl From<TerminalReceiptV1> for TerminalReceipt {
    fn from(value: TerminalReceiptV1) -> Self {
        Self::V1(value)
    }
}
impl From<TerminalReceiptV2> for TerminalReceipt {
    fn from(value: TerminalReceiptV2) -> Self {
        Self::V2(value)
    }
}
impl TerminalReceipt {
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::V1(v) => v.validate(),
            Self::V2(v) => v.validate(),
        }
    }
    pub fn digest(&self) -> io::Result<String> {
        match self {
            Self::V1(v) => v.digest(),
            Self::V2(v) => v.digest(),
        }
    }
    pub fn execution_binding(&self) -> ExecutionBinding {
        match self {
            Self::V1(v) => ExecutionBinding::V1(v.binding.clone()),
            Self::V2(v) => ExecutionBinding::V2(v.binding.clone()),
        }
    }
    pub fn contract_version(&self) -> u8 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
        }
    }
    pub fn owner_uuid(&self) -> Uuid {
        match self {
            Self::V1(v) => v.binding.owner_uuid,
            Self::V2(v) => v.binding.owner_uuid,
        }
    }
    pub fn owner_generation(&self) -> Uuid {
        match self {
            Self::V1(v) => v.binding.owner_generation,
            Self::V2(v) => v.binding.owner_generation,
        }
    }
    pub fn binding_digest(&self) -> &str {
        match self {
            Self::V1(v) => &v.binding_digest,
            Self::V2(v) => &v.binding_digest,
        }
    }
    pub fn workload_exit(&self) -> WorkloadExit {
        match self {
            Self::V1(v) => v.workload_exit,
            Self::V2(v) => v.workload_exit,
        }
    }
    pub fn child_reaped(&self) -> bool {
        match self {
            Self::V1(v) => v.child_reaped,
            Self::V2(v) => v.child_reaped,
        }
    }
    pub fn execution_observation(&self) -> Option<&ExecutionObservation> {
        match self {
            Self::V1(_) => None,
            Self::V2(v) => Some(&v.execution_observation),
        }
    }
    pub fn command_result(&self) -> io::Result<i32> {
        self.validate()?;
        if let Some(ExecutionObservation::ExecFailed { errno, .. }) = self.execution_observation() {
            return Err(fail(format!(
                "configured shell exec failed (errno{errno}); retained Child status {:?}",
                self.workload_exit()
            )));
        }
        self.workload_exit().mapped()
    }
    pub fn matches(&self, record: &Record) -> io::Result<()> {
        self.validate()?;
        match self {
            Self::V1(v) => v.matches(record),
            Self::V2(v) => {
                if record.execution_binding()? == ExecutionBinding::V2(v.binding.clone()) {
                    Ok(())
                } else {
                    Err(fail("admitted receipt differs from original binding"))
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionBinding {
    V1(CommittedBinding),
    V2(ExecutionBindingV2),
}
impl ExecutionBinding {
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::V1(v) => v.validate(),
            Self::V2(v) => v.validate(),
        }
    }
    pub fn digest(&self) -> io::Result<String> {
        match self {
            Self::V1(v) => v.digest(),
            Self::V2(v) => v.digest(),
        }
    }
    pub fn contract_version(&self) -> u8 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
        }
    }
    pub fn owner_uuid(&self) -> Uuid {
        match self {
            Self::V1(v) => v.owner_uuid,
            Self::V2(v) => v.owner_uuid,
        }
    }
    pub fn owner_generation(&self) -> Uuid {
        match self {
            Self::V1(v) => v.owner_generation,
            Self::V2(v) => v.owner_generation,
        }
    }
    pub fn namespace(&self) -> &NamespaceIdentity {
        match self {
            Self::V1(v) => &v.namespace,
            Self::V2(v) => &v.namespace,
        }
    }
    pub fn completion_socket(&self) -> (u64, u64) {
        match self {
            Self::V1(v) => v.completion_socket,
            Self::V2(v) => v.completion_socket,
        }
    }
    pub fn server(&self) -> &ProcessIdentity {
        match self {
            Self::V1(v) => &v.server,
            Self::V2(v) => &v.server,
        }
    }
    pub fn supervisor(&self) -> &ProcessIdentity {
        match self {
            Self::V1(v) => &v.supervisor,
            Self::V2(v) => &v.supervisor,
        }
    }
    pub fn workload_birth(&self) -> &ProcessIdentity {
        match self {
            Self::V1(v) => &v.workload,
            Self::V2(v) => &v.workload_birth,
        }
    }
}
