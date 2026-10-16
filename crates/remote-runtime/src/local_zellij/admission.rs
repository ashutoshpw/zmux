//! Explicit V2 admission of a retained direct Child, including legitimate exec.
use super::{
    completion::{CommittedBinding, TERMINAL_LIMIT, WorkloadExit},
    files::NamespaceIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io, path::PathBuf};
use uuid::Uuid;
pub use zmux_session_runtime::child::ProcessLifetime as ChildLifetime;
use zmux_session_runtime::child::{ProcessIdentity, ProcessLifetime};

pub const EXECUTION_DOMAIN: &str = "zmux.local-zellij.execution.v2";
pub const TERMINAL_DOMAIN: &str = "zmux.local-zellij.terminal.v2";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContract {
    AdmittedLifetimeV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedInvocation {
    pub shell: PathBuf,
    pub invocation_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBindingV2 {
    pub version: u8,
    pub domain: String,
    pub owner_uuid: Uuid,
    pub owner_generation: Uuid,
    pub namespace: NamespaceIdentity,
    pub completion_socket: (u64, u64),
    pub server: ProcessIdentity,
    pub supervisor: ProcessIdentity,
    pub workload_birth: ProcessIdentity,
    pub lifetime: ChildLifetime,
    pub invocation: AuthorizedInvocation,
    pub admission_id: Uuid,
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(crate) fn canonical_digest<T: Serialize>(domain: &str, value: &T) -> io::Result<String> {
    canonical_digest_bounded(domain, value, TERMINAL_LIMIT)
}
pub(crate) fn canonical_digest_bounded<T: Serialize>(
    domain: &str,
    value: &T,
    limit: usize,
) -> io::Result<String> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.len() > limit {
        return Err(super::fail("execution evidence exceeds8KiB"));
    }
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update([0]);
    hash.update(body);
    Ok(format!("{:x}", hash.finalize()))
}
impl ExecutionBindingV2 {
    pub fn validate(&self) -> io::Result<()> {
        let birth = CommittedBinding {
            version: 1,
            owner_uuid: self.owner_uuid,
            owner_generation: self.owner_generation,
            namespace: self.namespace.clone(),
            completion_socket: self.completion_socket,
            server: self.server.clone(),
            supervisor: self.supervisor.clone(),
            workload: self.workload_birth.clone(),
        };
        birth.validate()?;
        if self.version != 2
            || self.domain != EXECUTION_DOMAIN
            || self.admission_id.is_nil()
            || self.lifetime != ProcessLifetime::from(&self.workload_birth)
            || self.workload_birth.executable != self.supervisor.executable
            || !self.invocation.shell.is_absolute()
            || self
                .invocation
                .shell
                .to_str()
                .is_none_or(|s| s.len() > 4096 || s.contains('\0'))
            || !valid_digest(&self.invocation.invocation_digest)
        {
            return Err(super::fail("invalid admitted execution binding"));
        }
        canonical_digest(EXECUTION_DOMAIN, self)?;
        Ok(())
    }
    pub fn digest(&self) -> io::Result<String> {
        self.validate()?;
        canonical_digest(EXECUTION_DOMAIN, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecFailureStage {
    ConfiguredShellExec,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionObservation {
    NotObserved,
    ShellObserved,
    ExecFailed { stage: ExecFailureStage, errno: i32 },
}
impl ExecutionObservation {
    pub fn validate(&self) -> io::Result<()> {
        if matches!(self,Self::ExecFailed{errno,..} if *errno<=0) {
            return Err(super::fail("exec failure requires positive errno"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalReceiptV2 {
    pub version: u8,
    pub domain: String,
    pub binding: ExecutionBindingV2,
    pub binding_digest: String,
    pub terminal_sequence: u64,
    pub workload_exit: WorkloadExit,
    pub child_reaped: bool,
    pub execution_observation: ExecutionObservation,
}
impl TerminalReceiptV2 {
    pub fn validate(&self) -> io::Result<()> {
        self.binding.validate()?;
        self.workload_exit.validate()?;
        self.execution_observation.validate()?;
        if self.version != 2
            || self.domain != TERMINAL_DOMAIN
            || self.terminal_sequence != 1
            || !self.child_reaped
            || self.binding_digest != self.binding.digest()?
        {
            return Err(super::fail("invalid admitted lifetime terminal receipt"));
        }
        canonical_digest(TERMINAL_DOMAIN, self)?;
        Ok(())
    }
    pub fn digest(&self) -> io::Result<String> {
        self.validate()?;
        canonical_digest(TERMINAL_DOMAIN, self)
    }
}
