//! A finite result belongs to one originally bound owner lifetime. Durable bytes
//! alone are storage consistency; authority requires the same live supervisor.
use super::{Record, files::NamespaceIdentity};
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
) -> io::Result<Option<TerminalReceiptV1>> {
    super::completion_control::authenticate(record, deadline)
}
