//! Validated SSH connection descriptors and deterministic process arguments.
//!
//! Descriptors validate routes; private connection owners bind auxiliary operations
//! to one authenticated terminal. Upload workers perform bounded file and process I/O.
//! [`SshEndpoint::transport_args`] renders the initial connection route. Auxiliary
//! operations reuse its private authenticated socket and cannot reconnect independently.
//!
//! The descriptor deliberately supports a bounded OpenSSH subset. TCP local,
//! remote, and dynamic forwarding are supported. Unix-socket forwarding,
//! `ProxyCommand`, control-socket overrides, arbitrary `-o` keys, and custom
//! known-hosts files are rejected. A destination name may be either a DNS name
//! or an `ssh_config` alias; resolving configuration is left to OpenSSH.

mod bounded_command;
mod config;
pub mod connection;
pub mod upload;

pub use config::{JumpConfig, SshConnectionConfig};

pub mod local_multiplexer;
pub mod local_zellij;

use std::{fmt, net::IpAddr};
use thiserror::Error;

pub const SSH_PROGRAM: &str = "ssh";
pub const MAX_JUMP_HOSTS: usize = 8;
pub const MAX_FORWARDINGS: usize = 32;
pub const MAX_SSH_OPTIONS: usize = 32;
pub const MAX_COMMAND_ARGS: usize = 128;
pub const MAX_ARGUMENT_BYTES: usize = 4_096;
pub const MAX_COMMAND_BYTES: usize = 64 * 1_024;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DescriptorError {
    #[error("SSH destination is empty")]
    EmptyDestination,
    #[error("SSH destination contains more than one user separator")]
    MultipleUsers,
    #[error("invalid SSH user {0:?}; use ASCII letters, digits, '.', '_' or '-'")]
    InvalidUser(String),
    #[error("invalid SSH host or config alias {0:?}")]
    InvalidHost(String),
    #[error("SSH port must be between 1 and 65535")]
    InvalidPort,
    #[error("SSH identity path is invalid: {0}")]
    InvalidIdentity(&'static str),
    #[error("SSH config path is invalid: {0}")]
    InvalidConfig(&'static str),
    #[error("SSH option must use Key=Value syntax")]
    MalformedOption,
    #[error("unsupported SSH option {0:?}")]
    UnknownOption(String),
    #[error(
        "SSH option {0:?} conflicts with a typed endpoint, route, host-key, forwarding, command, or sharing field"
    )]
    ConflictingOption(String),
    #[error("invalid value for SSH option {name:?}: {reason}")]
    InvalidOptionValue { name: String, reason: &'static str },
    #[error("at most {limit} {kind} are supported")]
    TooMany { kind: &'static str, limit: usize },
    #[error("SSH option {0:?} is repeated")]
    DuplicateOption(String),
    #[error("agent forwarding is repeated")]
    DuplicateAgentForwarding,
    #[error("remote command must contain a non-empty program")]
    EmptyCommand,
    #[error("remote command argument {index} is invalid: {reason}")]
    InvalidCommandArgument { index: usize, reason: &'static str },
    #[error("remote command exceeds {MAX_COMMAND_BYTES} bytes")]
    CommandTooLarge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HostKind {
    Name(String),
    Address(IpAddr),
}

/// A validated host name, `ssh_config` alias, IPv4 address, or IPv6 address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshHost(HostKind);

impl SshHost {
    pub fn parse(value: &str) -> Result<Self, DescriptorError> {
        if value.is_empty() || value.len() > 253 || has_forbidden_text(value) {
            return Err(DescriptorError::InvalidHost(value.into()));
        }
        if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            return inner
                .parse::<std::net::Ipv6Addr>()
                .map(|address| Self(HostKind::Address(IpAddr::V6(address))))
                .map_err(|_| DescriptorError::InvalidHost(value.into()));
        }
        if value.starts_with('[') || value.ends_with(']') {
            return Err(DescriptorError::InvalidHost(value.into()));
        }
        if let Ok(address) = value.parse::<IpAddr>() {
            return Ok(Self(HostKind::Address(address)));
        }
        if value.contains(':')
            || value.starts_with('-')
            || value == "."
            || value.contains("..")
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(DescriptorError::InvalidHost(value.into()));
        }
        Ok(Self(HostKind::Name(value.into())))
    }

    pub fn as_str(&self) -> String {
        match &self.0 {
            HostKind::Name(name) => name.clone(),
            HostKind::Address(address) => address.to_string(),
        }
    }

    fn host_port(&self, port: u16) -> String {
        match &self.0 {
            HostKind::Address(IpAddr::V6(address)) => format!("[{address}]:{port}"),
            _ => format!("{}:{port}", self.as_str()),
        }
    }

    fn jump_host(&self) -> String {
        match &self.0 {
            HostKind::Address(IpAddr::V6(address)) => format!("[{address}]"),
            _ => self.as_str(),
        }
    }
}

impl fmt::Display for SshHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.as_str())
    }
}

/// A destination accepted by OpenSSH: `[user@]host`, including config aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshDestination {
    user: Option<String>,
    host: SshHost,
}

impl SshDestination {
    pub fn parse(value: &str) -> Result<Self, DescriptorError> {
        if value.is_empty() {
            return Err(DescriptorError::EmptyDestination);
        }
        let at_count = value.bytes().filter(|byte| *byte == b'@').count();
        if at_count > 1 {
            return Err(DescriptorError::MultipleUsers);
        }
        let (user, host) = match value.split_once('@') {
            Some((user, host)) => {
                if user.is_empty()
                    || user.len() > 255
                    || user.starts_with('-')
                    || !user.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
                {
                    return Err(DescriptorError::InvalidUser(user.into()));
                }
                (Some(user.into()), host)
            }
            None => (None, value),
        };
        Ok(Self {
            user,
            host: SshHost::parse(host)?,
        })
    }

    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub fn host(&self) -> &SshHost {
        &self.host
    }
}

impl fmt::Display for SshDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.user {
            write!(formatter, "{user}@")?;
        }
        write!(formatter, "{}", self.host)
    }
}

/// One typed `ProxyJump` hop. Raw `ProxyCommand` is intentionally unsupported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpHost {
    destination: SshDestination,
    port: Option<u16>,
}

impl JumpHost {
    pub fn new(destination: SshDestination, port: Option<u16>) -> Result<Self, DescriptorError> {
        validate_optional_port(port)?;
        Ok(Self { destination, port })
    }

    pub fn parse(value: &str, port: Option<u16>) -> Result<Self, DescriptorError> {
        Self::new(SshDestination::parse(value)?, port)
    }

    pub fn destination(&self) -> &SshDestination {
        &self.destination
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    fn argument(&self) -> String {
        let mut value = String::new();
        if let Some(user) = self.destination.user() {
            value.push_str(user);
            value.push('@');
        }
        value.push_str(&self.destination.host.jump_host());
        if let Some(port) = self.port {
            value.push(':');
            value.push_str(&port.to_string());
        }
        value
    }
}

/// Host-key behavior shared by every operation using the endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Preserve OpenSSH and `ssh_config` defaults without a command-line override.
    #[default]
    Config,
    Strict,
    AcceptNew,
    Ask,
}

impl HostKeyPolicy {
    fn option(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Strict => Some("StrictHostKeyChecking=yes"),
            Self::AcceptNew => Some("StrictHostKeyChecking=accept-new"),
            Self::Ask => Some("StrictHostKeyChecking=ask"),
        }
    }
}

/// Audited additional options that are safe to apply through the shared endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SshOptionKind {
    AddressFamily,
    BatchMode,
    Compression,
    ConnectTimeout,
    ConnectionAttempts,
    IdentitiesOnly,
    KeyboardInteractiveAuthentication,
    LogLevel,
    PasswordAuthentication,
    PreferredAuthentications,
    PubkeyAuthentication,
    ServerAliveCountMax,
    ServerAliveInterval,
    TcpKeepAlive,
}

impl SshOptionKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::AddressFamily => "AddressFamily",
            Self::BatchMode => "BatchMode",
            Self::Compression => "Compression",
            Self::ConnectTimeout => "ConnectTimeout",
            Self::ConnectionAttempts => "ConnectionAttempts",
            Self::IdentitiesOnly => "IdentitiesOnly",
            Self::KeyboardInteractiveAuthentication => "KbdInteractiveAuthentication",
            Self::LogLevel => "LogLevel",
            Self::PasswordAuthentication => "PasswordAuthentication",
            Self::PreferredAuthentications => "PreferredAuthentications",
            Self::PubkeyAuthentication => "PubkeyAuthentication",
            Self::ServerAliveCountMax => "ServerAliveCountMax",
            Self::ServerAliveInterval => "ServerAliveInterval",
            Self::TcpKeepAlive => "TCPKeepAlive",
        }
    }
}

/// One validated `-o Key=Value` entry. Descriptor order is preserved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshOption {
    kind: SshOptionKind,
    value: String,
}

impl SshOption {
    pub fn parse(value: &str) -> Result<Self, DescriptorError> {
        let (raw_key, raw_value) = value
            .split_once('=')
            .ok_or(DescriptorError::MalformedOption)?;
        if raw_key.is_empty() || raw_value.is_empty() || has_forbidden_text(raw_key) {
            return Err(DescriptorError::MalformedOption);
        }
        let key = raw_key.to_ascii_lowercase();
        let kind = match key.as_str() {
            "addressfamily" => SshOptionKind::AddressFamily,
            "batchmode" => SshOptionKind::BatchMode,
            "compression" => SshOptionKind::Compression,
            "connecttimeout" => SshOptionKind::ConnectTimeout,
            "connectionattempts" => SshOptionKind::ConnectionAttempts,
            "identitiesonly" => SshOptionKind::IdentitiesOnly,
            "kbdinteractiveauthentication" | "keyboardinteractiveauthentication" => {
                SshOptionKind::KeyboardInteractiveAuthentication
            }
            "loglevel" => SshOptionKind::LogLevel,
            "passwordauthentication" => SshOptionKind::PasswordAuthentication,
            "preferredauthentications" => SshOptionKind::PreferredAuthentications,
            "pubkeyauthentication" => SshOptionKind::PubkeyAuthentication,
            "serveralivecountmax" => SshOptionKind::ServerAliveCountMax,
            "serveraliveinterval" => SshOptionKind::ServerAliveInterval,
            "tcpkeepalive" => SshOptionKind::TcpKeepAlive,
            key if is_conflicting_option(key) => {
                return Err(DescriptorError::ConflictingOption(raw_key.into()));
            }
            _ => return Err(DescriptorError::UnknownOption(raw_key.into())),
        };
        if raw_value.chars().any(char::is_control) {
            return Err(DescriptorError::MalformedOption);
        }
        let value = validate_option_value(kind, raw_value)?;
        Ok(Self { kind, value })
    }

    pub fn kind(&self) -> SshOptionKind {
        self.kind
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn argument(&self) -> String {
        format!("{}={}", self.kind.key(), self.value)
    }
}

/// A TCP listen point. `host=None` uses OpenSSH's default loopback binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenEndpoint {
    host: Option<SshHost>,
    port: u16,
}

impl ListenEndpoint {
    pub fn new(host: Option<SshHost>, port: u16) -> Result<Self, DescriptorError> {
        validate_port(port)?;
        Ok(Self { host, port })
    }

    pub fn parse(host: Option<&str>, port: u16) -> Result<Self, DescriptorError> {
        Self::new(host.map(SshHost::parse).transpose()?, port)
    }

    pub fn host(&self) -> Option<&SshHost> {
        self.host.as_ref()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn argument(&self) -> String {
        match &self.host {
            Some(host) => host.host_port(self.port),
            None => self.port.to_string(),
        }
    }
}

/// A TCP connect target for local or remote forwarding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectEndpoint {
    host: SshHost,
    port: u16,
}

impl ConnectEndpoint {
    pub fn new(host: SshHost, port: u16) -> Result<Self, DescriptorError> {
        validate_port(port)?;
        Ok(Self { host, port })
    }

    pub fn parse(host: &str, port: u16) -> Result<Self, DescriptorError> {
        Self::new(SshHost::parse(host)?, port)
    }

    pub fn host(&self) -> &SshHost {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn argument(&self) -> String {
        self.host.host_port(self.port)
    }
}

/// Supported forwarding forms. The vector order in [`SshDescriptor`] is retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forwarding {
    Agent {
        enabled: bool,
    },
    Local {
        listen: ListenEndpoint,
        target: ConnectEndpoint,
    },
    Remote {
        listen: ListenEndpoint,
        target: ConnectEndpoint,
    },
    Dynamic {
        listen: ListenEndpoint,
    },
}

impl Forwarding {
    fn arguments(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Agent { enabled: true } => ("-A", None),
            Self::Agent { enabled: false } => ("-a", None),
            Self::Local { listen, target } => (
                "-L",
                Some(format!("{}:{}", listen.argument(), target.argument())),
            ),
            Self::Remote { listen, target } => (
                "-R",
                Some(format!("{}:{}", listen.argument(), target.argument())),
            ),
            Self::Dynamic { listen } => ("-D", Some(listen.argument())),
        }
    }
}

/// Route and authentication data shared by terminal and file operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshEndpoint {
    destination: SshDestination,
    config_file: Option<String>,
    port: Option<u16>,
    identity: Option<String>,
    jump_hosts: Vec<JumpHost>,
    host_key_policy: HostKeyPolicy,
    options: Vec<SshOption>,
}

impl SshEndpoint {
    pub fn destination(&self) -> &SshDestination {
        &self.destination
    }

    pub fn config_file(&self) -> Option<&str> {
        self.config_file.as_deref()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    pub fn jump_hosts(&self) -> &[JumpHost] {
        &self.jump_hosts
    }

    pub fn host_key_policy(&self) -> HostKeyPolicy {
        self.host_key_policy
    }

    pub fn options(&self) -> &[SshOption] {
        &self.options
    }

    /// Render OpenSSH `ssh` flags for operations on this endpoint.
    /// `scp`/`sftp` use different flag/destination syntax (including `-P` for port);
    /// those clients require an adapter rather than this argv verbatim.
    pub fn transport_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(config_file) = &self.config_file {
            args.extend(["-F".into(), config_file.clone()]);
        }
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string()]);
        }
        if let Some(identity) = &self.identity {
            args.extend(["-i".into(), identity.clone()]);
        }
        if !self.jump_hosts.is_empty() {
            args.extend([
                "-J".into(),
                self.jump_hosts
                    .iter()
                    .map(JumpHost::argument)
                    .collect::<Vec<_>>()
                    .join(","),
            ]);
        }
        if let Some(host_key_option) = self.host_key_policy.option() {
            args.extend(["-o".into(), host_key_option.into()]);
        }
        for option in &self.options {
            args.extend(["-o".into(), option.argument()]);
        }
        args
    }

    /// Render transport flags plus an option-terminated destination.
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = self.transport_args();
        args.push("--".into());
        args.push(self.destination.to_string());
        args
    }
}

/// A process description that can be copied directly into a portable-pty launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
}

/// A validated terminal SSH descriptor. It performs no shell interpolation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshDescriptor {
    endpoint: SshEndpoint,
    forwarding: Vec<Forwarding>,
    command: Option<Vec<String>>,
}

impl SshDescriptor {
    pub fn builder(destination: &str) -> Result<SshDescriptorBuilder, DescriptorError> {
        Ok(SshDescriptorBuilder {
            destination: SshDestination::parse(destination)?,
            config_file: None,
            port: None,
            identity: None,
            jump_hosts: Vec::new(),
            host_key_policy: HostKeyPolicy::default(),
            options: Vec::new(),
            forwarding: Vec::new(),
            command: None,
        })
    }

    pub fn endpoint(&self) -> &SshEndpoint {
        &self.endpoint
    }

    pub fn forwarding(&self) -> &[Forwarding] {
        &self.forwarding
    }

    pub fn command(&self) -> Option<&[String]> {
        self.command.as_deref()
    }

    /// Produce `ssh` plus argv for portable-pty. No local shell is involved.
    ///
    /// OpenSSH sends a requested remote command through the remote account's
    /// configured shell. Callers needing literal remote argv semantics must use
    /// a separately designed remote protocol rather than infer it here.
    pub fn process_spec(&self) -> ProcessSpec {
        let mut args = self.endpoint.transport_args();
        for forwarding in &self.forwarding {
            let (flag, value) = forwarding.arguments();
            args.push(flag.into());
            if let Some(value) = value {
                args.push(value);
            }
        }
        args.push("--".into());
        args.push(self.endpoint.destination.to_string());
        if let Some(command) = &self.command {
            args.extend(command.iter().cloned());
        }
        ProcessSpec {
            program: SSH_PROGRAM.into(),
            args,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SshDescriptorBuilder {
    destination: SshDestination,
    config_file: Option<String>,
    port: Option<u16>,
    identity: Option<String>,
    jump_hosts: Vec<JumpHost>,
    host_key_policy: HostKeyPolicy,
    options: Vec<SshOption>,
    forwarding: Vec<Forwarding>,
    command: Option<Vec<String>>,
}

impl SshDescriptorBuilder {
    pub fn config_file(mut self, config_file: impl Into<String>) -> Self {
        self.config_file = Some(config_file.into());
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    pub fn jump_host(mut self, jump_host: JumpHost) -> Self {
        self.jump_hosts.push(jump_host);
        self
    }

    pub fn host_key_policy(mut self, policy: HostKeyPolicy) -> Self {
        self.host_key_policy = policy;
        self
    }

    pub fn option(mut self, option: SshOption) -> Self {
        self.options.push(option);
        self
    }

    pub fn forwarding(mut self, forwarding: Forwarding) -> Self {
        self.forwarding.push(forwarding);
        self
    }

    pub fn command<I, S>(mut self, command: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.command = Some(command.into_iter().map(Into::into).collect());
        self
    }

    pub fn build(self) -> Result<SshDescriptor, DescriptorError> {
        validate_optional_port(self.port)?;
        if self.jump_hosts.len() > MAX_JUMP_HOSTS {
            return Err(DescriptorError::TooMany {
                kind: "jump hosts",
                limit: MAX_JUMP_HOSTS,
            });
        }
        if self.options.len() > MAX_SSH_OPTIONS {
            return Err(DescriptorError::TooMany {
                kind: "SSH options",
                limit: MAX_SSH_OPTIONS,
            });
        }
        if self.forwarding.len() > MAX_FORWARDINGS {
            return Err(DescriptorError::TooMany {
                kind: "forwardings",
                limit: MAX_FORWARDINGS,
            });
        }
        if let Some(config_file) = &self.config_file {
            validate_path(config_file).map_err(DescriptorError::InvalidConfig)?;
        }
        if let Some(identity) = &self.identity {
            validate_path(identity).map_err(DescriptorError::InvalidIdentity)?;
        }
        for (index, option) in self.options.iter().enumerate() {
            if self.options[..index]
                .iter()
                .any(|earlier| earlier.kind == option.kind)
            {
                return Err(DescriptorError::DuplicateOption(option.kind.key().into()));
            }
        }
        if self
            .forwarding
            .iter()
            .filter(|forward| matches!(forward, Forwarding::Agent { .. }))
            .count()
            > 1
        {
            return Err(DescriptorError::DuplicateAgentForwarding);
        }
        validate_command(self.command.as_deref())?;
        Ok(SshDescriptor {
            endpoint: SshEndpoint {
                destination: self.destination,
                config_file: self.config_file,
                port: self.port,
                identity: self.identity,
                jump_hosts: self.jump_hosts,
                host_key_policy: self.host_key_policy,
                options: self.options,
            },
            forwarding: self.forwarding,
            command: self.command,
        })
    }
}

fn validate_port(port: u16) -> Result<(), DescriptorError> {
    if port == 0 {
        Err(DescriptorError::InvalidPort)
    } else {
        Ok(())
    }
}

fn validate_optional_port(port: Option<u16>) -> Result<(), DescriptorError> {
    port.map(validate_port).transpose().map(|_| ())
}

fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    if path.len() > MAX_ARGUMENT_BYTES {
        return Err("path is too long");
    }
    if path.chars().any(char::is_control) {
        return Err("path contains a NUL, newline, or other control character");
    }
    Ok(())
}

fn has_forbidden_text(value: &str) -> bool {
    value.chars().any(char::is_control) || value.chars().any(|character| character.is_whitespace())
}

fn is_conflicting_option(key: &str) -> bool {
    matches!(
        key,
        "bindaddress"
            | "bindinterface"
            | "canonicaldomains"
            | "canonicalizefallbacklocal"
            | "canonicalizehostname"
            | "canonicalizemaxdots"
            | "certificatefile"
            | "controlmaster"
            | "controlpath"
            | "controlpersist"
            | "dynamicforward"
            | "exitonforwardfailure"
            | "forwardagent"
            | "globalknownhostsfile"
            | "hostkeyalias"
            | "hostname"
            | "identityagent"
            | "identityfile"
            | "localcommand"
            | "localforward"
            | "permitlocalcommand"
            | "port"
            | "proxycommand"
            | "proxyjump"
            | "proxyusefdpass"
            | "remotecommand"
            | "remoteforward"
            | "stricthostkeychecking"
            | "user"
            | "userknownhostsfile"
    )
}

fn validate_option_value(kind: SshOptionKind, value: &str) -> Result<String, DescriptorError> {
    let invalid = |reason| DescriptorError::InvalidOptionValue {
        name: kind.key().into(),
        reason,
    };
    match kind {
        SshOptionKind::AddressFamily => match value.to_ascii_lowercase().as_str() {
            "any" | "inet" | "inet6" => Ok(value.to_ascii_lowercase()),
            _ => Err(invalid("expected any, inet, or inet6")),
        },
        SshOptionKind::BatchMode
        | SshOptionKind::Compression
        | SshOptionKind::IdentitiesOnly
        | SshOptionKind::KeyboardInteractiveAuthentication
        | SshOptionKind::PasswordAuthentication
        | SshOptionKind::PubkeyAuthentication
        | SshOptionKind::TcpKeepAlive => match value.to_ascii_lowercase().as_str() {
            "yes" | "no" => Ok(value.to_ascii_lowercase()),
            _ => Err(invalid("expected yes or no")),
        },
        SshOptionKind::ConnectTimeout | SshOptionKind::ServerAliveInterval => {
            validate_number(kind, value, 0, 86_400)
        }
        SshOptionKind::ConnectionAttempts => validate_number(kind, value, 1, 100),
        SshOptionKind::ServerAliveCountMax => validate_number(kind, value, 0, 1_000),
        SshOptionKind::LogLevel => {
            let upper = value.to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "QUIET"
                    | "FATAL"
                    | "ERROR"
                    | "INFO"
                    | "VERBOSE"
                    | "DEBUG"
                    | "DEBUG1"
                    | "DEBUG2"
                    | "DEBUG3"
            ) {
                Ok(upper)
            } else {
                Err(invalid("unsupported OpenSSH log level"))
            }
        }
        SshOptionKind::PreferredAuthentications => {
            let mut methods = Vec::new();
            for method in value.split(',') {
                let method = method.to_ascii_lowercase();
                if !matches!(
                    method.as_str(),
                    "gssapi-with-mic"
                        | "hostbased"
                        | "publickey"
                        | "keyboard-interactive"
                        | "password"
                ) || methods.contains(&method)
                {
                    return Err(invalid(
                        "expected a unique comma-separated authentication list",
                    ));
                }
                methods.push(method);
            }
            Ok(methods.join(","))
        }
    }
}

fn validate_number(
    kind: SshOptionKind,
    value: &str,
    minimum: u32,
    maximum: u32,
) -> Result<String, DescriptorError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| DescriptorError::InvalidOptionValue {
            name: kind.key().into(),
            reason: "expected a decimal integer",
        })?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(DescriptorError::InvalidOptionValue {
            name: kind.key().into(),
            reason: "value is outside the supported bound",
        });
    }
    Ok(parsed.to_string())
}

fn validate_command(command: Option<&[String]>) -> Result<(), DescriptorError> {
    let Some(command) = command else {
        return Ok(());
    };
    if command.is_empty() || command[0].is_empty() {
        return Err(DescriptorError::EmptyCommand);
    }
    if command.len() > MAX_COMMAND_ARGS {
        return Err(DescriptorError::TooMany {
            kind: "remote command arguments",
            limit: MAX_COMMAND_ARGS,
        });
    }
    let mut total = 0usize;
    for (index, argument) in command.iter().enumerate() {
        if argument.len() > MAX_ARGUMENT_BYTES {
            return Err(DescriptorError::InvalidCommandArgument {
                index,
                reason: "argument is too long",
            });
        }
        if argument.chars().any(char::is_control) {
            return Err(DescriptorError::InvalidCommandArgument {
                index,
                reason: "argument contains a NUL, newline, or other control character",
            });
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_COMMAND_BYTES {
        return Err(DescriptorError::CommandTooLarge);
    }
    Ok(())
}
