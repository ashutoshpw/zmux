//! Managed-SSH transport for the per-account remote scheduler.
//!
//! Surface/session ownership is resolved by the serialized desktop executor. The
//! bounded SSH process itself runs on the control/native caller worker, never on
//! the executor or AppKit thread.

use crate::{DesktopError, SessionId, SurfaceId};
use filedescriptor::{AsRawFileDescriptor, FileDescriptor, POLLIN, POLLOUT, poll, pollfd};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    os::fd::{AsRawFd, OwnedFd},
    os::unix::net::UnixStream,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;
use zmux_remote_runtime::{
    connection::ManagedConnection,
    scheduler::{
        SCHEDULER_PROTOCOL_VERSION, SchedulerAction, SchedulerReply, SchedulerRequest,
        SchedulerResponse, SchedulerResult, ServiceMode, StoreHealth,
    },
};

pub const MAX_SCHEDULE_ENVELOPE_BYTES: usize = 16 * 1024;
pub const MAX_ACTIVE_OPERATIONS: usize = 4;
pub const SCHEDULE_RPC_TIMEOUT: Duration = Duration::from_secs(15);
const ROUTE_CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
const SCHEDULE_SETUP_INSTALL_TIMEOUT: Duration = Duration::from_secs(120);
const PROCESS_EXIT_PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_STDERR_BYTES: usize = 8 * 1024;
const IO_CHUNK_BYTES: usize = 4096;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_ERROR_MESSAGE_CHARS: usize = 512;
const SETUP_INSTALL_ACK: &str = "__ZMUX_SCHEDULER_INSTALL_COMPLETE__";
const SETUP_MARKER_PREFIX: &str = "__ZMUX_SCHEDULER_SETUP_";

/// Native requests bind the numeric surface/session pair to one model namespace.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub identity_namespace: Uuid,
    pub surface_id: SurfaceId,
    pub expected_session_id: SessionId,
    pub request: SchedulerRequest,
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub enum TargetSelector {
    SurfaceUuid(Uuid),
    Native {
        identity_namespace: Uuid,
        surface_id: SurfaceId,
        expected_session_id: SessionId,
    },
}

/// A transport-level reply. `ok` means the helper returned a valid protocol
/// response; service errors remain in `reply.result`.
#[derive(Clone, Debug, Serialize)]
pub struct Execution {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<SchedulerResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<TransportError>,
    pub outcome_unknown: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransportError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
}

impl Execution {
    pub fn failure(
        request_id: Option<Uuid>,
        code: impl Into<String>,
        message: impl Into<String>,
        outcome_unknown: bool,
    ) -> Self {
        Self {
            ok: false,
            reply: None,
            error: Some(TransportError {
                code: code.into(),
                message: message
                    .into()
                    .chars()
                    .take(MAX_ERROR_MESSAGE_CHARS)
                    .collect(),
                request_id,
            }),
            outcome_unknown,
        }
    }
}

/// A captured managed route. All identity and cancellation fields are taken at
/// one executor boundary and stay paired with this exact SSH ControlMaster.
#[doc(hidden)]
pub struct PreparedRoute {
    pub identity_namespace: Uuid,
    pub surface_id: SurfaceId,
    pub session_id: SessionId,
    pub launch_generation: u64,
    pub cancellation: Arc<AtomicBool>,
    pub connection: Arc<ManagedConnection>,
}

pub struct Runtime {
    helper_path: Option<PathBuf>,
    active: Arc<AtomicUsize>,
}

impl Runtime {
    pub fn new(helper_path: Option<PathBuf>) -> Result<Self, ConfigurationError> {
        let helper_path = match helper_path {
            Some(path) => Some(validate_helper_path(path)?),
            None => match std::env::var_os("ZMUX_SCHEDULER_REMOTE_HELPER") {
                Some(value) => Some(validate_helper_path(PathBuf::from(value))?),
                None => None,
            },
        };
        Ok(Self {
            helper_path,
            active: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn execute(
        &self,
        route: PreparedRoute,
        request: SchedulerRequest,
        desktop_stopping: &AtomicBool,
    ) -> Execution {
        let request_id = request.request_id;
        if let Err(error) = request.validate() {
            return Execution::failure(
                request_id,
                serde_json::to_value(error.error_code())
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "invalid_request".into()),
                error.to_string(),
                false,
            );
        }
        if desktop_stopping.load(Ordering::Acquire) || route.cancellation.load(Ordering::Acquire) {
            return Execution::failure(
                request_id,
                "connection_unavailable",
                "the selected managed SSH session is closing",
                false,
            );
        }
        let Some(_permit) = OperationPermit::try_acquire(Arc::clone(&self.active)) else {
            return Execution::failure(
                request_id,
                "capacity",
                format!("at most {MAX_ACTIVE_OPERATIONS} scheduler operations may run at once"),
                false,
            );
        };
        let request_bytes = match encode_request(&request) {
            Ok(bytes) => bytes,
            Err(message) => {
                return Execution::failure(request_id, "invalid_request", message, false);
            }
        };

        // Keep the captured launch generation explicit: reconnect always creates a
        // new token/connection pair, even though the model may reuse its SessionId.
        let _launch_generation = route.launch_generation;
        let _route_identity = (route.identity_namespace, route.surface_id, route.session_id);

        if matches!(
            &request.action,
            SchedulerAction::SetupCheck | SchedulerAction::SetupInstall
        ) {
            return self.execute_setup(route, request, desktop_stopping);
        }

        let remote_command = match self.rpc_command() {
            Ok(command) => command,
            Err(error) => {
                return Execution::failure(
                    request_id,
                    "invalid_helper_path",
                    error.to_string(),
                    false,
                );
            }
        };
        let spec = match route.connection.operation_spec(&remote_command) {
            Ok(spec) => spec,
            Err(error) => {
                return Execution::failure(
                    request_id,
                    "ssh_operation_invalid",
                    error.to_string(),
                    false,
                );
            }
        };
        match run_process(
            &spec.program,
            &spec.args,
            &request_bytes,
            SCHEDULE_RPC_TIMEOUT,
            &route.cancellation,
            desktop_stopping,
        ) {
            Ok(output) if !output.status.success() => {
                let detail = output
                    .stderr
                    .as_deref()
                    .map(safe_diagnostic)
                    .filter(|text| !text.is_empty())
                    .map(|text| format!("remote scheduler command failed: {text}"))
                    .unwrap_or_else(|| "remote scheduler command failed".into());
                Execution::failure(
                    request_id,
                    "remote_command_failed",
                    detail,
                    request.action.requires_request_id(),
                )
            }
            Ok(output) => match serde_json::from_slice::<SchedulerResponse>(&output.stdout) {
                Ok(reply)
                    if reply.protocol_version == SCHEDULER_PROTOCOL_VERSION
                        && reply.request_id == request.request_id =>
                {
                    let outcome_unknown = match &reply.result {
                        SchedulerResult::Success(_) => false,
                        SchedulerResult::Error(error) => error.outcome_unknown,
                    };
                    Execution {
                        ok: true,
                        reply: Some(reply),
                        error: None,
                        outcome_unknown,
                    }
                }
                Ok(_) => Execution::failure(
                    request_id,
                    "remote_protocol_error",
                    "remote scheduler response did not match the request protocol or ID",
                    request.action.requires_request_id(),
                ),
                Err(_) => Execution::failure(
                    request_id,
                    "remote_protocol_error",
                    "remote scheduler returned an invalid response",
                    request.action.requires_request_id(),
                ),
            },
            Err(error) => Execution::failure(
                request_id,
                error.code,
                error.message,
                request.action.requires_request_id() && error.after_spawn,
            ),
        }
    }

    fn rpc_command(&self) -> Result<String, ConfigurationError> {
        Ok(match &self.helper_path {
            Some(path) => format!(
                "{} rpc",
                zmux_remote_runtime::upload::quote_shell_path(
                    path.to_str().ok_or(ConfigurationError::InvalidHelperPath)?
                )
            ),
            None => "\"$HOME/.local/bin/zmux-scheduler\" rpc".into(),
        })
    }

    fn probe_scheduler_health(
        &self,
        route: &PreparedRoute,
        desktop_stopping: &AtomicBool,
    ) -> Result<(), SchedulerHealthFailure> {
        let request_id = Uuid::new_v4();
        let request = SchedulerRequest {
            protocol_version: SCHEDULER_PROTOCOL_VERSION,
            request_id: Some(request_id),
            action: SchedulerAction::Status {
                receipt_request_id: None,
            },
        };
        let request_bytes = encode_request(&request).map_err(|_| {
            SchedulerHealthFailure::Response("could not encode the scheduler Status request")
        })?;
        let remote_command = self.rpc_command().map_err(|_| {
            SchedulerHealthFailure::Response("the scheduler helper path is invalid")
        })?;
        let spec = route
            .connection
            .operation_spec(&remote_command)
            .map_err(|_| {
                SchedulerHealthFailure::Response("the captured managed SSH route is invalid")
            })?;
        let output = run_process(
            &spec.program,
            &spec.args,
            &request_bytes,
            SCHEDULE_RPC_TIMEOUT,
            &route.cancellation,
            desktop_stopping,
        )
        .map_err(SchedulerHealthFailure::Transport)?;
        if !output.status.success() {
            return Err(SchedulerHealthFailure::Transport(ProcessError {
                code: "remote_command_failed",
                message: "the remote scheduler Status command failed".into(),
                after_spawn: true,
            }));
        }
        parse_scheduler_health_response(&output.stdout, request_id)
    }

    fn helper_assignment(&self) -> Result<String, ConfigurationError> {
        Ok(match &self.helper_path {
            Some(path) => format!(
                "helper={}",
                zmux_remote_runtime::upload::quote_shell_path(
                    path.to_str().ok_or(ConfigurationError::InvalidHelperPath)?
                )
            ),
            None => "helper=\"${HOME:-}/.local/bin/zmux-scheduler\"".into(),
        })
    }

    fn execute_setup(
        &self,
        route: PreparedRoute,
        request: SchedulerRequest,
        desktop_stopping: &AtomicBool,
    ) -> Execution {
        let request_id = request.request_id;
        let install = matches!(&request.action, SchedulerAction::SetupInstall);
        let (assets, bundle_error) = match crate::schedule_bundle::BundleAssets::discover() {
            Ok(assets) => (Some(assets), None),
            Err(_) => (
                None,
                Some("the app bundle has no verified scheduler helper payload".to_owned()),
            ),
        };
        let before = match self.inspect_setup(&route, assets.as_ref(), desktop_stopping) {
            Ok(observation) => observation,
            Err(error) => {
                return Execution::failure(
                    request_id,
                    error.code,
                    error.message,
                    install && error.after_spawn,
                );
            }
        };
        let helper = assets
            .as_ref()
            .and_then(|assets| assets.helper_for(&before.os, &before.arch));
        let missing =
            setup_missing_prerequisites(&before, helper.is_some(), bundle_error.as_deref());
        let supported = missing.is_empty();
        if !install {
            let message = setup_message(&before, &missing, false);
            let health_message = if helper_service_is_probeable(&before) {
                match self.probe_scheduler_health(&route, desktop_stopping) {
                    Ok(()) => {
                        "The active scheduler answered Status with protocol 1 and a healthy store."
                            .to_owned()
                    }
                    Err(error) => format!(
                        "Scheduler health is unavailable: {}; SetupCheck did not start the service.",
                        error.message()
                    ),
                }
            } else {
                "Scheduler health is unavailable because the verified helper and active user service are not both present; SetupCheck did not start the service.".to_owned()
            };
            let message = format!("{message} {health_message}");
            return setup_execution(&request, &before, supported, missing, message, "check");
        }
        if !supported {
            let message = setup_message(&before, &missing, true);
            return Execution::failure(request_id, "setup_unsupported", message, false);
        }

        let asset = helper.expect("supported setup has a verified compatible helper");
        let command = match self.install_command(&asset.sha256) {
            Ok(command) => command,
            Err(error) => {
                return Execution::failure(
                    request_id,
                    "invalid_helper_path",
                    error.to_string(),
                    false,
                );
            }
        };
        let spec = match route.connection.operation_spec(&command) {
            Ok(spec) => spec,
            Err(error) => {
                return Execution::failure(
                    request_id,
                    "ssh_operation_invalid",
                    error.to_string(),
                    false,
                );
            }
        };
        match run_process(
            &spec.program,
            &spec.args,
            asset.bytes(),
            SCHEDULE_SETUP_INSTALL_TIMEOUT,
            &route.cancellation,
            desktop_stopping,
        ) {
            Ok(output) if !output.status.success() => {
                let detail = output
                    .stderr
                    .as_deref()
                    .map(safe_diagnostic)
                    .filter(|text| !text.is_empty())
                    .map(|text| format!("remote scheduler setup failed: {text}"))
                    .unwrap_or_else(|| "remote scheduler setup failed".into());
                Execution::failure(request_id, "setup_install_failed", detail, true)
            }
            Ok(output) if !has_setup_install_ack(&output.stdout) => Execution::failure(
                request_id,
                "setup_install_failed",
                "remote scheduler installation did not return its completion marker",
                true,
            ),
            Ok(_) => {
                let after = match self.inspect_setup(&route, assets.as_ref(), desktop_stopping) {
                    Ok(observation) => observation,
                    Err(error) => {
                        return Execution::failure(request_id, error.code, error.message, true);
                    }
                };
                if !after.helper_installed
                    || after.helper_matches_bundle != Some(true)
                    || after.helper_protocol_version != Some(SCHEDULER_PROTOCOL_VERSION)
                    || !after.service_enabled
                    || !after.service_active
                {
                    let message = format!(
                        "remote scheduler install completed, but verification failed (helper_installed={}, helper_matches_bundle={}, helper_protocol_version={}, service_enabled={}, service_active={})",
                        after.helper_installed,
                        after
                            .helper_matches_bundle
                            .map_or("unknown".to_owned(), |value| value.to_string()),
                        after
                            .helper_protocol_version
                            .map_or("unknown".to_owned(), |value| value.to_string()),
                        after.service_enabled,
                        after.service_active,
                    );
                    return Execution::failure(
                        request_id,
                        "setup_install_unverified",
                        message,
                        false,
                    );
                }
                if let Err(error) = self.probe_scheduler_health(&route, desktop_stopping) {
                    return Execution::failure(
                        request_id,
                        "setup_install_unverified",
                        format!(
                            "remote scheduler installation completed, but Status health verification failed: {}",
                            error.message()
                        ),
                        error.outcome_unknown(),
                    );
                }
                let missing = setup_missing_prerequisites(&after, helper.is_some(), None);
                let message = format!(
                    "{} The active scheduler answered Status with protocol 1 and a healthy store.",
                    setup_message(&after, &missing, false)
                );
                setup_execution(
                    &request,
                    &after,
                    missing.is_empty(),
                    missing,
                    message,
                    "install",
                )
            }
            Err(error) => {
                Execution::failure(request_id, error.code, error.message, error.after_spawn)
            }
        }
    }

    fn inspect_setup(
        &self,
        route: &PreparedRoute,
        assets: Option<&crate::schedule_bundle::BundleAssets>,
        desktop_stopping: &AtomicBool,
    ) -> Result<SetupObservation, ProcessError> {
        let nonce = Uuid::new_v4().simple().to_string();
        let (begin, end) = setup_markers(&nonce);
        let command = self
            .probe_command(assets, &begin, &end)
            .map_err(|error| ProcessError {
                code: "invalid_helper_path",
                message: error.to_string(),
                after_spawn: false,
            })?;
        let spec = route
            .connection
            .operation_spec(&command)
            .map_err(|error| ProcessError {
                code: "ssh_operation_invalid",
                message: error.to_string(),
                after_spawn: false,
            })?;
        let output = run_process(
            &spec.program,
            &spec.args,
            &[],
            SCHEDULE_RPC_TIMEOUT,
            &route.cancellation,
            desktop_stopping,
        )?;
        if !output.status.success() {
            let detail = output
                .stderr
                .as_deref()
                .map(safe_diagnostic)
                .filter(|text| !text.is_empty())
                .map(|text| format!("remote scheduler setup check failed: {text}"))
                .unwrap_or_else(|| "remote scheduler setup check failed".into());
            return Err(ProcessError {
                code: "setup_check_failed",
                message: detail,
                after_spawn: false,
            });
        }
        parse_setup_observation(&output.stdout, &begin, &end).map_err(|message| ProcessError {
            code: "setup_protocol_error",
            message,
            after_spawn: false,
        })
    }

    fn probe_command(
        &self,
        assets: Option<&crate::schedule_bundle::BundleAssets>,
        begin: &str,
        end: &str,
    ) -> Result<String, ConfigurationError> {
        let helper_assignment = self.helper_assignment()?;
        let digest = |key: &str| {
            assets
                .and_then(|assets| assets.helper_for_key(key))
                .map(|asset| asset.sha256.as_str())
                .unwrap_or("")
        };
        let helper_hash_x86 = digest("linux-x86_64");
        let helper_hash_arm = digest("linux-aarch64");
        let helper_hash_darwin = digest("darwin-universal");
        Ok(format!(
            r#"LC_ALL=C; export LC_ALL; {helper_assignment}; digest_file() {{ if command -v sha256sum >/dev/null 2>&1; then result=$(sha256sum <"$1" 2>/dev/null) || return 1; printf '%s' "${{result%% *}}"; elif command -v shasum >/dev/null 2>&1; then result=$(shasum -a 256 <"$1" 2>/dev/null) || return 1; printf '%s' "${{result%% *}}"; else return 1; fi; }}; raw_os=$(uname -s 2>/dev/null || :); case "$raw_os" in Linux) os=linux;; Darwin) os=darwin;; *) os=unknown;; esac; raw_arch=$(uname -m 2>/dev/null || :); case "$raw_arch" in x86_64|amd64) arch=x86_64;; aarch64|arm64) arch=aarch64;; *) arch=other;; esac; expected=; case "$os:$arch" in linux:x86_64) expected={helper_hash_x86};; linux:aarch64) expected={helper_hash_arm};; darwin:*) expected={helper_hash_darwin};; esac; helper_installed=no; helper_matches_bundle=unknown; helper_protocol_version=unknown; if [ -x "$helper" ]; then helper_installed=yes; if [ -n "$expected" ]; then actual=$(digest_file "$helper" 2>/dev/null || :); if [ -n "$actual" ]; then if [ "$actual" = "$expected" ]; then helper_matches_bundle=yes; else helper_matches_bundle=no; fi; fi; fi; protocol=$("$helper" protocol-version 2>/dev/null || :); case "$protocol" in 1) helper_protocol_version=1;; esac; fi; service_manager=none; service_active=no; service_enabled=no; if command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then service_manager=systemd_user; if systemctl --user is-active --quiet zmux-scheduler.service >/dev/null 2>&1; then service_active=yes; fi; if systemctl --user is-enabled --quiet zmux-scheduler.service >/dev/null 2>&1; then service_enabled=yes; fi; fi; linger=unknown; if command -v loginctl >/dev/null 2>&1 && [ -n "${{USER:-}}" ]; then value=$(loginctl show-user "$USER" -p Linger --value 2>/dev/null || :); case "$value" in yes|true) linger=yes;; no|false) linger=no;; esac; fi; tmux_available=no; if command -v tmux >/dev/null 2>&1; then tmux_available=yes; fi; clock_available=no; now=$(date -u +%s 2>/dev/null || :); case "$now" in ''|*[!0-9]*) :;; *) clock_available=yes;; esac; storage_available=no; if [ -n "${{XDG_STATE_HOME:-}}" ]; then case "$XDG_STATE_HOME" in /*) state_home=$XDG_STATE_HOME;; *) state_home=;; esac; else state_home="${{HOME:-}}/.local/state"; fi; if [ -n "$state_home" ]; then state="$state_home/zmux/scheduler"; if [ -L "$state" ]; then :; elif [ -e "$state" ]; then if [ -d "$state" ] && [ -r "$state" ] && [ -w "$state" ] && [ -x "$state" ]; then storage_available=yes; fi; else probe=$state; while [ ! -d "$probe" ]; do parent=${{probe%/*}}; [ -n "$parent" ] || parent=/; [ "$parent" != "$probe" ] || break; probe=$parent; done; if [ -d "$probe" ] && [ -w "$probe" ] && [ -x "$probe" ]; then storage_available=yes; fi; fi; fi; helper_install_writable=no; helper_dir=${{helper%/*}}; [ -n "$helper_dir" ] || helper_dir=/; probe=$helper_dir; while [ ! -d "$probe" ]; do parent=${{probe%/*}}; [ -n "$parent" ] || parent=/; [ "$parent" != "$probe" ] || break; probe=$parent; done; if [ -d "$probe" ] && [ -w "$probe" ] && [ -x "$probe" ]; then helper_install_writable=yes; fi; unit_install_writable=no; unit_dir="${{HOME:-}}/.config/systemd/user"; probe=$unit_dir; while [ ! -d "$probe" ]; do parent=${{probe%/*}}; [ -n "$parent" ] || parent=/; [ "$parent" != "$probe" ] || break; probe=$parent; done; if [ -d "$probe" ] && [ -w "$probe" ] && [ -x "$probe" ]; then unit_install_writable=yes; fi; sha256_available=no; if command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; then sha256_available=yes; fi; printf '%s\n' {begin}; printf 'os=%s\narch=%s\nhelper_installed=%s\nhelper_matches_bundle=%s\nhelper_protocol_version=%s\nservice_manager=%s\nservice_active=%s\nservice_enabled=%s\nlinger_enabled=%s\ntmux_available=%s\nstorage_available=%s\nclock_available=%s\nhelper_install_writable=%s\nunit_install_writable=%s\nsha256_available=%s\n' "$os" "$arch" "$helper_installed" "$helper_matches_bundle" "$helper_protocol_version" "$service_manager" "$service_active" "$service_enabled" "$linger" "$tmux_available" "$storage_available" "$clock_available" "$helper_install_writable" "$unit_install_writable" "$sha256_available"; printf '%s\n' {end}"#,
            helper_hash_x86 = zmux_remote_runtime::upload::quote_shell_path(helper_hash_x86),
            helper_hash_arm = zmux_remote_runtime::upload::quote_shell_path(helper_hash_arm),
            helper_hash_darwin = zmux_remote_runtime::upload::quote_shell_path(helper_hash_darwin),
            begin = zmux_remote_runtime::upload::quote_shell_path(begin),
            end = zmux_remote_runtime::upload::quote_shell_path(end),
        ))
    }

    fn install_command(&self, expected_digest: &str) -> Result<String, ConfigurationError> {
        let helper_assignment = self.helper_assignment()?;
        let helper_invocation = match &self.helper_path {
            Some(path) => format!(
                "exec {} serve",
                zmux_remote_runtime::upload::quote_shell_path(
                    path.to_str().ok_or(ConfigurationError::InvalidHelperPath)?
                )
            ),
            None => "exec \"$HOME/.local/bin/zmux-scheduler\" serve".into(),
        };
        let wrapper_contents = format!("#!/bin/sh\n{helper_invocation}\n");
        let unit_contents = "[Unit]\nDescription=Zmux per-user scheduled tmux messages\nAfter=default.target\n\n[Service]\nType=simple\nExecStart=%h/.local/bin/zmux-scheduler-service\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n";
        Ok(format!(
            r#"set -u; umask 077; {helper_assignment}; digest_file() {{ if command -v sha256sum >/dev/null 2>&1; then result=$(sha256sum <"$1") || return 1; printf '%s' "${{result%% *}}"; elif command -v shasum >/dev/null 2>&1; then result=$(shasum -a 256 <"$1") || return 1; printf '%s' "${{result%% *}}"; else return 1; fi; }}; helper_dir=${{helper%/*}}; [ -n "$helper_dir" ] || helper_dir=/; mkdir -p -- "$helper_dir" || exit 81; tmp=$(mktemp "$helper_dir/.zmux-scheduler.XXXXXXXXXX") || exit 82; cleanup() {{ [ -z "${{tmp:-}}" ] || rm -f -- "$tmp"; [ -z "${{wrapper_tmp:-}}" ] || rm -f -- "$wrapper_tmp"; [ -z "${{unit_tmp:-}}" ] || rm -f -- "$unit_tmp"; }}; trap cleanup EXIT HUP INT TERM; cat >"$tmp" || exit 83; actual=$(digest_file "$tmp") || exit 84; [ "$actual" = {expected_digest} ] || exit 85; chmod 700 "$tmp" || exit 86; mv -f -- "$tmp" "$helper" || exit 87; tmp=; mkdir -p -- "$HOME/.local/bin" "$HOME/.config/systemd/user" || exit 88; wrapper_tmp=$(mktemp "$HOME/.local/bin/.zmux-scheduler-service.XXXXXXXXXX") || exit 89; printf '%s\n' {wrapper_contents} >"$wrapper_tmp" || exit 90; chmod 700 "$wrapper_tmp" || exit 91; mv -f -- "$wrapper_tmp" "$HOME/.local/bin/zmux-scheduler-service" || exit 92; wrapper_tmp=; unit_tmp=$(mktemp "$HOME/.config/systemd/user/.zmux-scheduler.service.XXXXXXXXXX") || exit 93; printf '%s\n' {unit_contents} >"$unit_tmp" || exit 94; chmod 600 "$unit_tmp" || exit 95; mv -f -- "$unit_tmp" "$HOME/.config/systemd/user/zmux-scheduler.service" || exit 96; unit_tmp=; systemctl --user daemon-reload >/dev/null 2>&1 || exit 97; systemctl --user enable zmux-scheduler.service >/dev/null 2>&1 || exit 98; if systemctl --user is-active --quiet zmux-scheduler.service >/dev/null 2>&1; then systemctl --user restart zmux-scheduler.service >/dev/null 2>&1 || exit 99; else systemctl --user start zmux-scheduler.service >/dev/null 2>&1 || exit 100; fi; trap - EXIT HUP INT TERM; printf '%s\n' {ack}"#,
            expected_digest = zmux_remote_runtime::upload::quote_shell_path(expected_digest),
            wrapper_contents = quote_shell_lines(&wrapper_contents),
            unit_contents = quote_shell_lines(unit_contents),
            ack = zmux_remote_runtime::upload::quote_shell_path(SETUP_INSTALL_ACK),
        ))
    }
}

// These generated files end in a newline. Keep their line separators out of the
// SSH command itself: the strict operation guard rejects literal control bytes.
// A fixed printf format restores newlines while preserving each line verbatim.
fn quote_shell_lines(contents: &str) -> String {
    contents
        .lines()
        .map(zmux_remote_runtime::upload::quote_shell_path)
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug)]
struct SetupObservation {
    os: String,
    arch: String,
    helper_installed: bool,
    helper_matches_bundle: Option<bool>,
    helper_protocol_version: Option<u16>,
    service_manager: String,
    service_active: bool,
    service_enabled: bool,
    linger_enabled: Option<bool>,
    tmux_available: bool,
    storage_available: bool,
    clock_available: bool,
    helper_install_writable: bool,
    unit_install_writable: bool,
    sha256_available: bool,
}

fn setup_markers(nonce: &str) -> (String, String) {
    (
        format!("{SETUP_MARKER_PREFIX}{nonce}_BEGIN__"),
        format!("{SETUP_MARKER_PREFIX}{nonce}_END__"),
    )
}

fn parse_setup_observation(
    bytes: &[u8],
    begin: &str,
    end: &str,
) -> Result<SetupObservation, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "remote setup response was not UTF-8")?;
    let lines: Vec<_> = text.lines().collect();
    let starts: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == begin).then_some(index))
        .collect();
    let ends: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == end).then_some(index))
        .collect();
    if starts.len() != 1 || ends.len() != 1 || starts[0] >= ends[0] {
        return Err("remote setup response was missing its unique boundary markers".into());
    }
    let mut values = BTreeMap::new();
    for line in &lines[starts[0] + 1..ends[0]] {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "remote setup response contained an invalid field".to_owned())?;
        if values.insert(key, value).is_some() {
            return Err("remote setup response contained a duplicate field".into());
        }
    }
    const FIELDS: [&str; 15] = [
        "os",
        "arch",
        "helper_installed",
        "helper_matches_bundle",
        "helper_protocol_version",
        "service_manager",
        "service_active",
        "service_enabled",
        "linger_enabled",
        "tmux_available",
        "storage_available",
        "clock_available",
        "helper_install_writable",
        "unit_install_writable",
        "sha256_available",
    ];
    if values.len() != FIELDS.len() || FIELDS.iter().any(|key| !values.contains_key(key)) {
        return Err("remote setup response did not match the expected field set".into());
    }
    let value = |key: &str| values.get(key).copied().expect("required setup field");
    let boolean = |key: &str| parse_remote_bool(value(key));
    let optional_boolean = |key: &str| parse_remote_optional_bool(value(key));
    let os = match value("os") {
        "linux" | "darwin" | "unknown" => value("os").to_owned(),
        _ => return Err("remote setup response reported an invalid OS".into()),
    };
    let arch = match value("arch") {
        "x86_64" | "aarch64" | "other" => value("arch").to_owned(),
        _ => return Err("remote setup response reported an invalid architecture".into()),
    };
    let service_manager = match value("service_manager") {
        "systemd_user" | "none" => value("service_manager").to_owned(),
        _ => return Err("remote setup response reported an invalid service manager".into()),
    };
    let helper_protocol_version = match value("helper_protocol_version") {
        "unknown" => None,
        "1" => Some(1),
        _ => {
            return Err("remote setup response reported an invalid helper protocol version".into());
        }
    };
    Ok(SetupObservation {
        os,
        arch,
        helper_installed: boolean("helper_installed")?,
        helper_matches_bundle: optional_boolean("helper_matches_bundle")?,
        helper_protocol_version,
        service_manager,
        service_active: boolean("service_active")?,
        service_enabled: boolean("service_enabled")?,
        linger_enabled: optional_boolean("linger_enabled")?,
        tmux_available: boolean("tmux_available")?,
        storage_available: boolean("storage_available")?,
        clock_available: boolean("clock_available")?,
        helper_install_writable: boolean("helper_install_writable")?,
        unit_install_writable: boolean("unit_install_writable")?,
        sha256_available: boolean("sha256_available")?,
    })
}

fn parse_remote_bool(value: &str) -> Result<bool, String> {
    match value {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err("remote setup response contained an invalid boolean".into()),
    }
}

fn parse_remote_optional_bool(value: &str) -> Result<Option<bool>, String> {
    match value {
        "yes" => Ok(Some(true)),
        "no" => Ok(Some(false)),
        "unknown" => Ok(None),
        _ => Err("remote setup response contained an invalid optional boolean".into()),
    }
}

fn helper_service_is_probeable(observation: &SetupObservation) -> bool {
    observation.helper_installed
        && observation.helper_matches_bundle == Some(true)
        && observation.helper_protocol_version == Some(SCHEDULER_PROTOCOL_VERSION)
        && observation.service_active
}

fn parse_scheduler_health_response(
    bytes: &[u8],
    request_id: Uuid,
) -> Result<(), SchedulerHealthFailure> {
    let response = serde_json::from_slice::<SchedulerResponse>(bytes).map_err(|_| {
        SchedulerHealthFailure::Response("the scheduler Status response was invalid")
    })?;
    if response.protocol_version != SCHEDULER_PROTOCOL_VERSION
        || response.request_id != Some(request_id)
    {
        return Err(SchedulerHealthFailure::Response(
            "the scheduler Status response did not match protocol 1 and its request ID",
        ));
    }
    match response.result {
        SchedulerResult::Success(SchedulerReply::Status { status }) => {
            if status.protocol_version != SCHEDULER_PROTOCOL_VERSION {
                return Err(SchedulerHealthFailure::Response(
                    "the scheduler Status payload did not report protocol 1",
                ));
            }
            if status.service_mode != ServiceMode::SystemdUser {
                return Err(SchedulerHealthFailure::Response(
                    "the scheduler Status response did not come from the systemd user service",
                ));
            }
            if status.store_health != StoreHealth::Healthy {
                return Err(SchedulerHealthFailure::Response(
                    "the scheduler store reported degraded health",
                ));
            }
            Ok(())
        }
        SchedulerResult::Success(_) => Err(SchedulerHealthFailure::Response(
            "the scheduler returned the wrong reply for its Status request",
        )),
        SchedulerResult::Error(_) => Err(SchedulerHealthFailure::Response(
            "the scheduler rejected its Status health probe",
        )),
    }
}

fn setup_missing_prerequisites(
    observation: &SetupObservation,
    has_payload: bool,
    bundle_error: Option<&str>,
) -> Vec<String> {
    let mut missing = Vec::new();
    if observation.os != "linux" {
        missing.push("automatic installation currently supports Linux only".into());
    }
    if !has_payload {
        missing.push(
            bundle_error
                .unwrap_or("the app bundle has no helper for this remote architecture")
                .into(),
        );
    }
    if observation.service_manager != "systemd_user" {
        missing.push("a reachable systemd user manager is unavailable".into());
    }
    if !observation.tmux_available {
        missing.push("tmux is unavailable on the remote host".into());
    }
    if !observation.storage_available {
        missing.push("the per-user scheduler state directory is not safely accessible".into());
    }
    if !observation.clock_available {
        missing.push("the remote UTC clock is unavailable".into());
    }
    if !observation.helper_install_writable {
        missing.push("the helper install directory is not writable by this account".into());
    }
    if !observation.unit_install_writable {
        missing.push("the systemd user unit directory is not writable by this account".into());
    }
    if !observation.sha256_available {
        missing.push("no SHA-256 utility is available on the remote host".into());
    }
    missing
}

fn setup_message(
    observation: &SetupObservation,
    missing: &[String],
    install_not_attempted: bool,
) -> String {
    if !missing.is_empty() {
        let lead = if install_not_attempted {
            "Installation was not attempted"
        } else {
            "Automatic installation is unavailable"
        };
        return format!("{lead}: {}.", missing.join("; "));
    }
    let mut message = if observation.helper_installed
        && observation.service_active
        && observation.service_enabled
    {
        "The scheduler helper is installed and its systemd user service is enabled and active."
            .to_owned()
    } else {
        "The remote host has the prerequisites for the Linux systemd user scheduler.".to_owned()
    };
    match observation.linger_enabled {
        Some(false) => message.push_str(
            " User lingering is disabled, so the service may stop after the last login; enable it with `loginctl enable-linger $USER` if it must run after logout.",
        ),
        Some(true) => message.push_str(" User lingering is enabled, so the service can continue after logout."),
        None => message.push_str(" User lingering state could not be determined."),
    }
    message
}

fn setup_execution(
    request: &SchedulerRequest,
    observation: &SetupObservation,
    supported: bool,
    missing: Vec<String>,
    message: String,
    operation: &str,
) -> Execution {
    let setup = serde_json::json!({
        "os": observation.os,
        "arch": observation.arch,
        "supported": supported,
        "helper_installed": observation.helper_installed,
        "helper_matches_bundle": observation.helper_matches_bundle,
        "helper_protocol_version": observation.helper_protocol_version,
        "service_manager": observation.service_manager,
        "service_active": observation.service_active,
        "service_enabled": observation.service_enabled,
        "linger_enabled": observation.linger_enabled,
        "tmux_available": observation.tmux_available,
        "missing_prerequisites": missing,
        "message": message,
        "operation": operation,
    });
    Execution {
        ok: true,
        reply: Some(SchedulerResponse::success(
            request,
            zmux_remote_runtime::scheduler::SchedulerReply::Setup { setup },
        )),
        error: None,
        outcome_unknown: false,
    }
}

fn has_setup_install_ack(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .is_some_and(|text| text.lines().any(|line| line == SETUP_INSTALL_ACK))
}

#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error(
        "remote scheduler helper path must be absolute UTF-8 without control characters and at most 4096 bytes"
    )]
    InvalidHelperPath,
}

fn validate_helper_path(path: PathBuf) -> Result<PathBuf, ConfigurationError> {
    let value = path
        .to_str()
        .filter(|value| {
            value.starts_with('/')
                && value.len() <= 4096
                && !value.is_empty()
                && !value.chars().any(char::is_control)
        })
        .ok_or(ConfigurationError::InvalidHelperPath)?;
    if value.contains('\0') {
        return Err(ConfigurationError::InvalidHelperPath);
    }
    Ok(path)
}

fn encode_request(request: &SchedulerRequest) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(request)
        .map_err(|_| "scheduler request could not be encoded".to_owned())?;
    if bytes.len() > zmux_remote_runtime::scheduler::MAX_REQUEST_BYTES {
        return Err("scheduler request exceeds its 16 KiB bound".into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}

struct OperationPermit {
    active: Arc<AtomicUsize>,
}

impl OperationPermit {
    fn try_acquire(active: Arc<AtomicUsize>) -> Option<Self> {
        loop {
            let current = active.load(Ordering::Acquire);
            if current >= MAX_ACTIVE_OPERATIONS {
                return None;
            }
            if active
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(Self { active });
            }
        }
    }
}

impl Drop for OperationPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Option<Vec<u8>>,
}

struct ProcessOwner {
    child: Child,
    reaped: bool,
    process_group_terminated: bool,
}

impl ProcessOwner {
    fn terminate_process_group(&mut self) {
        if self.process_group_terminated {
            return;
        }
        use nix::{
            sys::signal::{Signal, killpg},
            unistd::Pid,
        };
        if let Err(error) = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL)
            && error != nix::errno::Errno::ESRCH
        {
            eprintln!("zmux scheduler process cancellation: {error}");
        }
        self.process_group_terminated = true;
    }

    fn terminate_and_reap(&mut self) {
        self.terminate_process_group();
        if !self.reaped {
            if let Err(error) = self.child.wait() {
                eprintln!("zmux scheduler process reap: {error}");
            }
            self.reaped = true;
        }
    }
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        self.terminate_and_reap();
    }
}

#[derive(Debug)]
struct ProcessError {
    code: &'static str,
    message: String,
    after_spawn: bool,
}

#[derive(Debug)]
enum SchedulerHealthFailure {
    Transport(ProcessError),
    Response(&'static str),
}

impl SchedulerHealthFailure {
    fn message(&self) -> &str {
        match self {
            Self::Transport(error) => &error.message,
            Self::Response(message) => message,
        }
    }

    fn outcome_unknown(&self) -> bool {
        match self {
            Self::Transport(error) => error.after_spawn,
            Self::Response(_) => false,
        }
    }
}

fn run_process(
    program: &str,
    args: &[String],
    input: &[u8],
    timeout: Duration,
    cancellation: &AtomicBool,
    desktop_stopping: &AtomicBool,
) -> Result<ProcessOutput, ProcessError> {
    if cancellation.load(Ordering::Acquire) || desktop_stopping.load(Ordering::Acquire) {
        return Err(ProcessError {
            code: "connection_unavailable",
            message: "the selected managed SSH session is closing".into(),
            after_spawn: false,
        });
    }
    // A nonblocking UnixStream writer gives this worker bounded, cancellable input
    // without changing the blocking mode of the SSH process's stdin endpoint.
    let (input_writer, input_reader) = UnixStream::pair().map_err(|error| ProcessError {
        code: "ssh_io_unavailable",
        message: format!("could not prepare managed SSH scheduler input: {error}"),
        after_spawn: false,
    })?;
    input_writer
        .set_nonblocking(true)
        .map_err(|error| ProcessError {
            code: "ssh_io_unavailable",
            message: format!("could not configure managed SSH scheduler input: {error}"),
            after_spawn: false,
        })?;
    let mut command = Command::new(program);
    command
        .args(args)
        .process_group(0)
        .stdin(Stdio::from(OwnedFd::from(input_reader)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut owner = ProcessOwner {
        child: command.spawn().map_err(|error| ProcessError {
            code: "ssh_unavailable",
            message: format!("could not start the managed SSH scheduler operation: {error}"),
            after_spawn: false,
        })?,
        reaped: false,
        process_group_terminated: false,
    };
    let mut stdin = Some(input_writer);
    let child_stdout = owner.child.stdout.take().expect("piped SSH stdout");
    let child_stderr = owner.child.stderr.take().expect("piped SSH stderr");
    let mut stdout = FileDescriptor::dup(&child_stdout).map_err(|error| ProcessError {
        code: "ssh_io_unavailable",
        message: format!("could not prepare managed SSH scheduler output: {error}"),
        after_spawn: true,
    })?;
    let mut stderr = FileDescriptor::dup(&child_stderr).map_err(|error| ProcessError {
        code: "ssh_io_unavailable",
        message: format!("could not prepare managed SSH scheduler diagnostics: {error}"),
        after_spawn: true,
    })?;
    stdout
        .set_non_blocking(true)
        .map_err(|error| ProcessError {
            code: "ssh_io_unavailable",
            message: format!("could not configure managed SSH scheduler output: {error}"),
            after_spawn: true,
        })?;
    stderr
        .set_non_blocking(true)
        .map_err(|error| ProcessError {
            code: "ssh_io_unavailable",
            message: format!("could not configure managed SSH scheduler diagnostics: {error}"),
            after_spawn: true,
        })?;

    let deadline = Instant::now() + timeout;
    let mut status = None;
    let mut input_offset = 0;
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let (mut stdout_eof, mut stderr_eof) = (false, false);
    let mut drain_deadline = None;
    loop {
        if cancellation.load(Ordering::Acquire) || desktop_stopping.load(Ordering::Acquire) {
            return Err(ProcessError {
                code: "cancelled",
                message: "the managed SSH scheduler operation was cancelled".into(),
                after_spawn: true,
            });
        }
        if Instant::now() >= deadline {
            return Err(ProcessError {
                code: "timeout",
                message: format!(
                    "remote scheduler did not respond within {} seconds",
                    timeout.as_secs()
                ),
                after_spawn: true,
            });
        }

        if let Some(writer) = stdin.as_mut()
            && input_offset < input.len()
        {
            match writer.write(&input[input_offset..]) {
                Ok(0) => {
                    return Err(ProcessError {
                        code: "request_write_failed",
                        message: "managed SSH closed before the scheduler request was sent".into(),
                        after_spawn: input_offset == input.len(),
                    });
                }
                Ok(count) => input_offset += count,
                Err(error) if retryable(&error) => {}
                Err(error) => {
                    return Err(ProcessError {
                        code: "request_write_failed",
                        message: format!(
                            "could not send the scheduler request over managed SSH: {error}"
                        ),
                        after_spawn: input_offset == input.len(),
                    });
                }
            }
        }
        if input_offset == input.len() {
            stdin.take();
        }

        drain_bounded(
            &mut stdout,
            &mut stdout_bytes,
            zmux_remote_runtime::scheduler::MAX_RESPONSE_BYTES,
            &mut stdout_eof,
        )
        .map_err(|error| ProcessError {
            code: "remote_response_too_large",
            message: error.to_string(),
            after_spawn: true,
        })?;
        drain_bounded(
            &mut stderr,
            &mut stderr_bytes,
            MAX_STDERR_BYTES,
            &mut stderr_eof,
        )
        .map_err(|error| ProcessError {
            code: "remote_diagnostic_too_large",
            message: error.to_string(),
            after_spawn: true,
        })?;

        if status.is_none() {
            match owner.child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    owner.reaped = true;
                    // A descendant may have inherited stdout or stderr. Kill every
                    // process in the operation group even though the SSH leader has
                    // already been reaped, then drain only for a short bounded grace.
                    owner.terminate_process_group();
                    drain_deadline = Some(Instant::now() + PROCESS_EXIT_PIPE_DRAIN_TIMEOUT);
                    stdin.take();
                    if input_offset != input.len() {
                        return Err(ProcessError {
                            code: "request_write_failed",
                            message:
                                "managed SSH exited before the full scheduler request was sent"
                                    .into(),
                            after_spawn: false,
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(ProcessError {
                        code: "ssh_process_failed",
                        message: format!("could not inspect managed SSH process: {error}"),
                        after_spawn: true,
                    });
                }
            }
        }
        if let Some(status) = status {
            if stdout_eof && stderr_eof {
                return Ok(ProcessOutput {
                    status,
                    stdout: std::mem::take(&mut stdout_bytes),
                    stderr: Some(std::mem::take(&mut stderr_bytes)),
                });
            }
            if drain_deadline.is_some_and(|at| Instant::now() >= at) {
                return Err(ProcessError {
                    code: "ssh_process_leaked_pipe",
                    message: "managed SSH exited but a child kept its output pipe open".into(),
                    after_spawn: true,
                });
            }
        }

        let mut fds = Vec::with_capacity(3);
        if !stdout_eof {
            fds.push(pollfd {
                fd: stdout.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if !stderr_eof {
            fds.push(pollfd {
                fd: stderr.as_raw_file_descriptor(),
                events: POLLIN,
                revents: 0,
            });
        }
        if let Some(stdin) = &stdin
            && input_offset < input.len()
        {
            fds.push(pollfd {
                fd: stdin.as_raw_fd(),
                events: POLLOUT,
                revents: 0,
            });
        }
        if let Err(error) = poll(&mut fds, Some(IO_POLL_INTERVAL))
            && !matches!(&error, filedescriptor::Error::Poll(error) if error.kind() == io::ErrorKind::Interrupted)
        {
            return Err(ProcessError {
                code: "ssh_io_failed",
                message: format!("could not poll managed SSH scheduler I/O: {error}"),
                after_spawn: true,
            });
        }
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn drain_bounded(
    pipe: &mut FileDescriptor,
    output: &mut Vec<u8>,
    limit: usize,
    eof: &mut bool,
) -> io::Result<()> {
    if *eof {
        return Ok(());
    }
    for _ in 0..4 {
        let mut buffer = [0; IO_CHUNK_BYTES];
        match pipe.read(&mut buffer) {
            Ok(0) => {
                *eof = true;
                break;
            }
            Ok(count) => {
                if output.len() + count > limit {
                    return Err(io::Error::other(format!(
                        "stream exceeds its {limit} byte bound"
                    )));
                }
                output.extend_from_slice(&buffer[..count]);
            }
            Err(error) if retryable(&error) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn safe_diagnostic(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_ERROR_MESSAGE_CHARS)
        .collect()
}

#[derive(Debug)]
#[doc(hidden)]
pub enum CaptureError {
    Desktop(DesktopError),
    Route {
        code: &'static str,
        message: &'static str,
    },
}

impl From<DesktopError> for CaptureError {
    fn from(error: DesktopError) -> Self {
        Self::Desktop(error)
    }
}

pub(crate) const fn route_capture_timeout() -> Duration {
    ROUTE_CAPTURE_TIMEOUT
}

pub(crate) fn desktop_error_code(error: &DesktopError) -> &'static str {
    match error {
        DesktopError::NotTerminal => "not_terminal",
        DesktopError::TerminalUnavailable => "connection_unavailable",
        DesktopError::QueueFull => "capacity",
        DesktopError::Closed => "desktop_closed",
        DesktopError::Busy => "desktop_busy",
        DesktopError::Timeout => "executor_timeout",
        DesktopError::Policy(error) => error.code(),
        _ => "route_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::AtomicBool, thread};
    use zmux_remote_runtime::scheduler::SchedulerStatus;

    fn encoded_status_response(
        response_request_id: Uuid,
        payload_protocol: u16,
        service_mode: ServiceMode,
        store_health: StoreHealth,
    ) -> Vec<u8> {
        serde_json::to_vec(&SchedulerResponse {
            protocol_version: SCHEDULER_PROTOCOL_VERSION,
            request_id: Some(response_request_id),
            result: SchedulerResult::Success(SchedulerReply::Status {
                status: SchedulerStatus {
                    helper_instance_id: Uuid::new_v4(),
                    remote_uid: 1000,
                    helper_version: "test".into(),
                    protocol_version: payload_protocol,
                    service_mode,
                    store_health,
                    checked_at_utc: "2026-10-22T00:00:00Z".into(),
                    tmux_available: true,
                    default_tmux_socket_id: None,
                    server_incarnation: None,
                    linger_enabled: Some(true),
                },
            }),
        })
        .expect("encode Status response")
    }

    #[test]
    fn scheduler_health_requires_matching_protocol_systemd_service_and_healthy_store() {
        let request_id = Uuid::new_v4();
        let healthy = encoded_status_response(
            request_id,
            SCHEDULER_PROTOCOL_VERSION,
            ServiceMode::SystemdUser,
            StoreHealth::Healthy,
        );
        assert!(parse_scheduler_health_response(&healthy, request_id).is_ok());

        let degraded = encoded_status_response(
            request_id,
            SCHEDULER_PROTOCOL_VERSION,
            ServiceMode::SystemdUser,
            StoreHealth::Degraded,
        );
        assert!(parse_scheduler_health_response(&degraded, request_id).is_err());

        let wrong_service = encoded_status_response(
            request_id,
            SCHEDULER_PROTOCOL_VERSION,
            ServiceMode::ManualForeground,
            StoreHealth::Healthy,
        );
        assert!(parse_scheduler_health_response(&wrong_service, request_id).is_err());

        assert!(parse_scheduler_health_response(&healthy, Uuid::new_v4()).is_err());
    }

    #[test]
    fn leader_exit_kills_descendant_that_inherited_output_pipe() {
        let marker =
            std::env::temp_dir().join(format!("zmux-scheduler-inherited-pipe-{}", Uuid::new_v4()));
        let marker_path = zmux_remote_runtime::upload::quote_shell_path(
            marker.to_str().expect("temporary path is UTF-8"),
        );
        let script = format!("(sleep 0.3; printf done > {marker_path}) & exit 0");
        let args = vec!["-c".to_owned(), script];
        let cancellation = AtomicBool::new(false);
        let stopping = AtomicBool::new(false);
        let started = Instant::now();

        let output = run_process(
            "/bin/sh",
            &args,
            b"",
            Duration::from_secs(3),
            &cancellation,
            &stopping,
        )
        .expect("the parent shell should exit cleanly after its child is killed");

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(started.elapsed() < Duration::from_millis(500));
        thread::sleep(Duration::from_millis(400));
        assert!(!marker.exists(), "the inherited-pipe child must be killed");
        let _ = fs::remove_file(marker);
    }

    #[test]
    fn scheduler_operation_permits_are_released_after_completion() {
        let active = Arc::new(AtomicUsize::new(0));
        let permits: Vec<_> = (0..MAX_ACTIVE_OPERATIONS)
            .map(|_| OperationPermit::try_acquire(Arc::clone(&active)).expect("permit available"))
            .collect();
        assert_eq!(active.load(Ordering::Acquire), MAX_ACTIVE_OPERATIONS);
        assert!(OperationPermit::try_acquire(Arc::clone(&active)).is_none());
        drop(permits);
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert!(OperationPermit::try_acquire(Arc::clone(&active)).is_some());
    }
}

#[cfg(test)]
#[path = "schedule_ssh_tests.rs"]
mod ssh_tests;
