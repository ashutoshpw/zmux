//! Authorization for the local socket. Native presentation is a separate trusted entrypoint;
//! remote relays cannot enter this interface until object-scoped authorization is implemented.
use crate::control::ControlCommand;
use crate::targets::TargetOperation;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    sync::{Arc, RwLock, RwLockReadGuard},
};
use subtle::ConstantTimeEq;
use thiserror::Error;

const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Clone)]
pub struct SocketPassword(String);

impl SocketPassword {
    pub fn new(value: String) -> Result<Self, PolicyError> {
        if value.is_empty()
            || value.len() > MAX_PASSWORD_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(PolicyError::InvalidPassword);
        }
        Ok(Self(value))
    }

    fn matches(&self, supplied: Option<&Self>) -> bool {
        let Some(supplied) = supplied else {
            return false;
        };
        // Fixed-size comparison also avoids making the password length a comparison shortcut.
        let mut expected = [0_u8; MAX_PASSWORD_BYTES + 8];
        let mut actual = [0_u8; MAX_PASSWORD_BYTES + 8];
        expected[..self.0.len()].copy_from_slice(self.0.as_bytes());
        actual[..supplied.0.len()].copy_from_slice(supplied.0.as_bytes());
        expected[MAX_PASSWORD_BYTES..].copy_from_slice(&(self.0.len() as u64).to_be_bytes());
        actual[MAX_PASSWORD_BYTES..].copy_from_slice(&(supplied.0.len() as u64).to_be_bytes());
        bool::from(expected.ct_eq(&actual))
    }
}
impl fmt::Debug for SocketPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SocketPassword([redacted])")
    }
}
impl Serialize for SocketPassword {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for SocketPassword {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAccess {
    Full,
    ReadOnly,
    Disabled,
}

#[derive(Clone, Debug)]
pub struct ControlPolicy {
    access: ControlAccess,
    password: Option<SocketPassword>,
}

impl Default for ControlPolicy {
    fn default() -> Self {
        Self::same_user(ControlAccess::Full)
    }
}
impl ControlPolicy {
    pub fn same_user(access: ControlAccess) -> Self {
        Self {
            access,
            password: None,
        }
    }
    pub fn password(access: ControlAccess, password: SocketPassword) -> Self {
        Self {
            access,
            password: Some(password),
        }
    }
    pub fn access(&self) -> ControlAccess {
        self.access
    }
    pub fn authentication(&self) -> &'static str {
        if self.password.is_some() {
            "same_uid_and_password"
        } else {
            "same_uid"
        }
    }
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("socket password must contain 1–1024 UTF-8 bytes without control characters")]
    InvalidPassword,
    #[error("local socket access is disabled")]
    Disabled,
    #[error("local socket authentication failed")]
    Unauthorized,
    #[error("local socket access is read-only")]
    ReadOnly,
    #[error("socket authorization changed; request is no longer authorized")]
    Revoked,
    #[error("remote relay access is denied")]
    RelayDenied,
    #[error("socket authorization is unavailable")]
    Unavailable,
}
impl PolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Revoked => "authorization_revoked",
            Self::Unavailable => "authorization_unavailable",
            _ => "policy_denied",
        }
    }
}

#[derive(Clone, Copy)]
pub enum ControlOrigin {
    LocalOwner,
    RemoteRelay,
}

struct State {
    generation: u64,
    policy: ControlPolicy,
}

/// Shared by listener and executor. Replacing a policy revokes admissions still in queues.
#[derive(Clone)]
pub struct ControlGate {
    state: Arc<RwLock<State>>,
}

#[derive(Clone)]
pub(crate) struct ControlAdmission {
    state: Arc<RwLock<State>>,
    generation: u64,
}

impl ControlGate {
    pub fn new(policy: ControlPolicy) -> Self {
        Self {
            state: Arc::new(RwLock::new(State {
                generation: 0,
                policy,
            })),
        }
    }
    /// May wait for an executing model mutation; call from a worker, never the UI thread.
    /// Completed launches/resources are not canceled by this socket-only policy owner.
    pub fn replace(&self, policy: ControlPolicy) -> Result<(), PolicyError> {
        let mut state = self.state.write().map_err(|_| PolicyError::Unavailable)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(PolicyError::Unavailable)?;
        state.policy = policy;
        Ok(())
    }
    pub(crate) fn admit(
        &self,
        origin: ControlOrigin,
        command: &ControlCommand,
        password: Option<&SocketPassword>,
    ) -> Result<ControlAdmission, PolicyError> {
        if matches!(origin, ControlOrigin::RemoteRelay) {
            return Err(PolicyError::RelayDenied);
        }
        let state = self.state.read().map_err(|_| PolicyError::Unavailable)?;
        if state.policy.access == ControlAccess::Disabled {
            return Err(PolicyError::Disabled);
        }
        if state
            .policy
            .password
            .as_ref()
            .is_some_and(|expected| !expected.matches(password))
        {
            return Err(PolicyError::Unauthorized);
        }
        if state.policy.access == ControlAccess::ReadOnly && !is_read(command) {
            return Err(PolicyError::ReadOnly);
        }
        Ok(ControlAdmission {
            state: Arc::clone(&self.state),
            generation: state.generation,
        })
    }
}

pub(crate) struct AdmissionGuard<'a> {
    state: RwLockReadGuard<'a, State>,
}
impl AdmissionGuard<'_> {
    pub fn policy(&self) -> &ControlPolicy {
        &self.state.policy
    }
}
impl ControlAdmission {
    pub(crate) fn check(&self) -> Result<AdmissionGuard<'_>, PolicyError> {
        let state = self.state.read().map_err(|_| PolicyError::Unavailable)?;
        if state.generation != self.generation {
            return Err(PolicyError::Revoked);
        }
        Ok(AdmissionGuard { state })
    }
}

pub(crate) fn is_read(command: &ControlCommand) -> bool {
    match command {
        ControlCommand::SurfaceOrder { .. } => false,
        ControlCommand::WorkspaceOrder { request } => request.dry_run,
        ControlCommand::WindowStatus { .. } => true,
        ControlCommand::Windows { request } => matches!(
            request.action,
            crate::windows::WindowAction::List | crate::windows::WindowAction::Current
        ),
        ControlCommand::DownloadWaitStatus { .. }
        | ControlCommand::LocalTmuxStatus { .. }
        | ControlCommand::BrowserProfilesStatus { .. }
        | ControlCommand::BrowserStatus { .. }
        | ControlCommand::SessionStatus { .. }
        | ControlCommand::ListUploads
        | ControlCommand::Status
        | ControlCommand::Current { .. }
        | ControlCommand::Capabilities
        | ControlCommand::ListPaneSurfaces { .. }
        | ControlCommand::RecentlyClosed
        | ControlCommand::ListNotifications
        | ControlCommand::ListWorkspaces
        | ControlCommand::ListPanes { .. }
        | ControlCommand::ReadScreen { .. } => true,
        ControlCommand::Browser { request } => request.action.is_read(),
        ControlCommand::Downloads { request } => !matches!(
            request.action,
            crate::downloads::DownloadAction::Cancel { .. }
        ),
        ControlCommand::Targeted { request } => matches!(
            request.operation,
            TargetOperation::ReadScreen
                | TargetOperation::ListPanes
                | TargetOperation::ListPaneSurfaces
                | TargetOperation::Metadata {
                    action: crate::metadata::MetadataAction::ListStatus
                        | crate::metadata::MetadataAction::ListLog { .. }
                }
        ),
        ControlCommand::LocalTmux { .. }
        | ControlCommand::BrowserProfiles { .. }
        | ControlCommand::Session { .. }
        | ControlCommand::Transfer { .. }
        | ControlCommand::Action { .. }
        | ControlCommand::Attention { .. }
        | ControlCommand::NewWorkspace { .. }
        | ControlCommand::RenameWorkspace { .. }
        | ControlCommand::FocusPane { .. }
        | ControlCommand::SplitPane { .. }
        | ControlCommand::ClosePane { .. }
        | ControlCommand::SendText { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Desktop, DesktopError, Request, Shell};
    use std::time::{Duration, Instant};
    use zmux_workspace_core::Action;

    #[test]
    fn a_revoked_admission_cannot_mutate_the_workspace_executor() {
        let desktop = Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let initial = loop {
            if let Ok(snapshot) = desktop.snapshot()
                && snapshot.model.workspaces.len() == 1
            {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "initial workspace not ready");
            std::thread::sleep(Duration::from_millis(1));
        };
        let gate = ControlGate::new(ControlPolicy::default());
        let action = Action::CreateWorkspace {
            name: "Revoked".into(),
        };
        let admission = gate
            .admit(
                ControlOrigin::LocalOwner,
                &ControlCommand::Action {
                    action: action.clone(),
                },
                None,
            )
            .unwrap();
        gate.replace(ControlPolicy::same_user(ControlAccess::ReadOnly))
            .unwrap();
        let execution = desktop
            .execute_authorized(
                Request {
                    version: 1,
                    request_id: 7,
                    payload: Command::Action { action },
                },
                Duration::from_secs(2),
                admission,
            )
            .unwrap();
        assert!(matches!(
            execution.result,
            Err(DesktopError::Policy(PolicyError::Revoked))
        ));
        assert_eq!(execution.snapshot.model.revision, initial.model.revision);
        assert_eq!(execution.snapshot.model.workspaces.len(), 1);
        desktop.shutdown(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn remote_origin_never_inherits_local_owner_authority() {
        let gate = ControlGate::new(ControlPolicy::default());
        assert!(matches!(
            gate.admit(ControlOrigin::RemoteRelay, &ControlCommand::Status, None),
            Err(PolicyError::RelayDenied)
        ));
        assert!(
            gate.admit(ControlOrigin::LocalOwner, &ControlCommand::Status, None)
                .is_ok()
        );
    }
    #[test]
    fn browser_reads_follow_readonly_policy_and_native_relay_stays_denied() {
        use crate::browser::{
            BrowserAction, BrowserRequest, DEFAULT_OPERATION_TIMEOUT_MS, GetKind, SnapshotOptions,
        };
        let gate = ControlGate::new(ControlPolicy::same_user(ControlAccess::ReadOnly));
        for (action, expected_read) in [
            (
                BrowserAction::Get {
                    kind: GetKind::Url,
                    selector: None,
                    attribute: None,
                    property: None,
                },
                true,
            ),
            (
                BrowserAction::Snapshot {
                    options: SnapshotOptions::default(),
                },
                true,
            ),
            (
                BrowserAction::Eval {
                    script: "1+1".into(),
                },
                false,
            ),
            (
                BrowserAction::Reload {
                    snapshot_after: false,
                },
                false,
            ),
        ] {
            let command = ControlCommand::Browser {
                request: BrowserRequest {
                    scope: Default::default(),
                    caller: Default::default(),
                    expected_model_revision: None,
                    timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
                    action,
                },
            };
            assert_eq!(
                gate.admit(ControlOrigin::LocalOwner, &command, None)
                    .is_ok(),
                expected_read
            );
            assert!(matches!(
                gate.admit(ControlOrigin::RemoteRelay, &command, None),
                Err(PolicyError::RelayDenied)
            ));
        }
        assert!(
            gate.admit(
                ControlOrigin::LocalOwner,
                &ControlCommand::BrowserStatus { operation_id: 1 },
                None
            )
            .is_ok()
        );
    }
    #[test]
    fn bulk_notification_acknowledgements_and_removals_require_mutation_authority() {
        use crate::attention::{
            DismissSelection, NotificationAction, NotificationScope, NotificationSelection,
        };
        let readonly = ControlGate::new(ControlPolicy::same_user(ControlAccess::ReadOnly));
        let full = ControlGate::new(ControlPolicy::same_user(ControlAccess::Full));
        for action in [
            NotificationAction::MarkRead {
                selection: NotificationSelection::All,
            },
            NotificationAction::Dismiss {
                selection: DismissSelection::AllRead,
            },
            NotificationAction::Clear {
                scope: NotificationScope::All,
                through_id: None,
                expected_epoch: None,
            },
        ] {
            let command = ControlCommand::Targeted {
                request: crate::targets::TargetRequest {
                    operation: crate::targets::TargetOperation::Notifications { action },
                    ..Default::default()
                },
            };
            assert!(matches!(
                readonly.admit(ControlOrigin::LocalOwner, &command, None),
                Err(PolicyError::ReadOnly)
            ));
            assert!(
                full.admit(ControlOrigin::LocalOwner, &command, None)
                    .is_ok()
            );
            assert!(matches!(
                full.admit(ControlOrigin::RemoteRelay, &command, None),
                Err(PolicyError::RelayDenied)
            ));
        }
        assert!(
            readonly
                .admit(
                    ControlOrigin::LocalOwner,
                    &ControlCommand::ListNotifications,
                    None
                )
                .is_ok()
        );
    }
}
