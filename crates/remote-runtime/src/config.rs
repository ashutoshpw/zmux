use serde::{Deserialize, Serialize};

use crate::{DescriptorError, Forwarding, JumpHost, SshDescriptor, SshOption};

/// Serializable input for one hop in an SSH jump route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JumpConfig {
    pub destination: String,
    #[serde(default)]
    pub port: Option<u16>,
}

/// Serializable SSH connection input retained by the workspace owner.
///
/// Deserialization preserves the supplied values. Callers must use [`Self::validate`]
/// before creating or mutating runtime state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshConnectionConfig {
    pub destination: String,
    #[serde(default)]
    pub config_file: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub forward_agent: Option<bool>,
    #[serde(default)]
    pub ssh_options: Vec<String>,
    #[serde(default)]
    pub jump_hosts: Vec<JumpConfig>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
}

impl SshConnectionConfig {
    /// Validate untrusted wire or stored input through the typed descriptor owner.
    pub fn validate(&self) -> Result<SshDescriptor, DescriptorError> {
        let mut builder = SshDescriptor::builder(&self.destination)?;

        if let Some(config_file) = &self.config_file {
            builder = builder.config_file(config_file);
        }
        if let Some(port) = self.port {
            builder = builder.port(port);
        }
        if let Some(identity) = &self.identity {
            builder = builder.identity(identity);
        }
        for jump in &self.jump_hosts {
            builder = builder.jump_host(JumpHost::parse(&jump.destination, jump.port)?);
        }
        for option in &self.ssh_options {
            builder = builder.option(SshOption::parse(option)?);
        }
        if let Some(enabled) = self.forward_agent {
            builder = builder.forwarding(Forwarding::Agent { enabled });
        }
        if let Some(command) = &self.command {
            builder = builder.command(command);
        }

        builder.build()
    }

    /// Preserve connection routing while preventing a stored job from replaying.
    pub fn for_restore(&self) -> Self {
        let mut restored = self.clone();
        restored.command = None;
        restored
    }
}
