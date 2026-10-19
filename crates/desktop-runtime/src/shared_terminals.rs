//! Narrow, grant-scoped access to existing terminal sessions for registered devices.
//!
//! This API deliberately carries only terminal reads and mutations. It is not a
//! transport for the general desktop control protocol.

use crate::{Command, Desktop, DesktopError, Frame, Request, Snapshot};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::{collections::HashSet, fmt, sync::mpsc, time::Instant};
use thiserror::Error;
use uuid::Uuid;
use zmux_terminal_adapter::KeyEvent;
use zmux_workspace_core::{PaneId, SessionId, Snapshot as Model, SurfaceId, SurfaceKind};

pub const MAX_SHARED_TERMINAL_REQUEST_BYTES: usize = 128 * 1024;
pub const MAX_SHARED_TERMINAL_RESPONSE_BYTES: usize = 256 * 1024;

const SHARED_TERMINAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_SHARED_TERMINAL_TITLE_CHARS: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedTerminalEnvelope {
    pub grant: Uuid,
    pub request: SharedTerminalOperation,
}

/// The complete operation set available to a registered remote device.
#[derive(Debug)]
pub enum SharedTerminalOperation {
    List,
    Frame {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
    },
    Input {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
        text: String,
    },
    Key {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
        event: KeyEvent,
    },
    Paste {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
        text: String,
    },
    Resize {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
        rows: u16,
        cols: u16,
    },
    Scroll {
        namespace: Uuid,
        surface_id: SurfaceId,
        session_id: SessionId,
        lines: i32,
    },
}

fn required_shared_field<T, E>(field: Option<T>, name: &'static str) -> Result<T, E>
where
    E: de::Error,
{
    field.ok_or_else(|| E::missing_field(name))
}

impl<'de> Deserialize<'de> for SharedTerminalOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OperationVisitor;

        impl<'de> Visitor<'de> for OperationVisitor {
            type Value = SharedTerminalOperation;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a strict shared terminal operation object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut seen = HashSet::new();
                let mut operation = None;
                let mut namespace = None;
                let mut surface_id = None;
                let mut session_id = None;
                let mut text = None;
                let mut event = None;
                let mut rows = None;
                let mut cols = None;
                let mut lines = None;
                while let Some(name) = map.next_key::<String>()? {
                    if !seen.insert(name.clone()) {
                        return Err(de::Error::custom(format!("duplicate field `{name}`")));
                    }
                    match name.as_str() {
                        "operation" => operation = Some(map.next_value()?),
                        "namespace" => namespace = Some(map.next_value()?),
                        "surface_id" => surface_id = Some(map.next_value()?),
                        "session_id" => session_id = Some(map.next_value()?),
                        "text" => text = Some(map.next_value()?),
                        "event" => event = Some(map.next_value()?),
                        "rows" => rows = Some(map.next_value()?),
                        "cols" => cols = Some(map.next_value()?),
                        "lines" => lines = Some(map.next_value()?),
                        _ => {
                            return Err(de::Error::custom(format!("unknown field `{name}`")));
                        }
                    }
                }

                let operation: String = required_shared_field(operation, "operation")?;
                let allowed_fields: &[&str] = match operation.as_str() {
                    "list" => &["operation"],
                    "frame" => &["operation", "namespace", "surface_id", "session_id"],
                    "input" | "paste" => {
                        &["operation", "namespace", "surface_id", "session_id", "text"]
                    }
                    "key" => &[
                        "operation",
                        "namespace",
                        "surface_id",
                        "session_id",
                        "event",
                    ],
                    "resize" => &[
                        "operation",
                        "namespace",
                        "surface_id",
                        "session_id",
                        "rows",
                        "cols",
                    ],
                    "scroll" => &[
                        "operation",
                        "namespace",
                        "surface_id",
                        "session_id",
                        "lines",
                    ],
                    _ => {
                        return Err(de::Error::unknown_variant(
                            &operation,
                            &["list", "frame", "input", "key", "paste", "resize", "scroll"],
                        ));
                    }
                };
                if let Some(name) = seen
                    .iter()
                    .find(|name| !allowed_fields.contains(&name.as_str()))
                {
                    return Err(de::Error::custom(format!(
                        "field `{name}` is not valid for operation `{operation}`"
                    )));
                }

                match operation.as_str() {
                    "list" => Ok(SharedTerminalOperation::List),
                    "frame" => Ok(SharedTerminalOperation::Frame {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                    }),
                    "input" => Ok(SharedTerminalOperation::Input {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                        text: required_shared_field(text, "text")?,
                    }),
                    "key" => Ok(SharedTerminalOperation::Key {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                        event: required_shared_field(event, "event")?,
                    }),
                    "paste" => Ok(SharedTerminalOperation::Paste {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                        text: required_shared_field(text, "text")?,
                    }),
                    "resize" => Ok(SharedTerminalOperation::Resize {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                        rows: required_shared_field(rows, "rows")?,
                        cols: required_shared_field(cols, "cols")?,
                    }),
                    "scroll" => Ok(SharedTerminalOperation::Scroll {
                        namespace: required_shared_field(namespace, "namespace")?,
                        surface_id: required_shared_field(surface_id, "surface_id")?,
                        session_id: required_shared_field(session_id, "session_id")?,
                        lines: required_shared_field(lines, "lines")?,
                    }),
                    _ => unreachable!("unknown operation was rejected above"),
                }
            }
        }

        deserializer.deserialize_map(OperationVisitor)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SharedTerminalSession {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub session_id: SessionId,
    pub title: String,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum SharedTerminalResponse {
    List {
        ok: bool,
        namespace: Uuid,
        sessions: Vec<SharedTerminalSession>,
    },
    Frame {
        ok: bool,
        frame: Frame,
    },
    Accepted {
        ok: bool,
        accepted: bool,
    },
}

#[derive(Debug)]
pub struct SharedTerminalCallError {
    pub error: DesktopError,
    /// True means the owner may have admitted the terminal event; callers must not replay it.
    pub outcome_unknown: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TerminalSharingGrant {
    pub grant: Uuid,
    pub namespace: Uuid,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SharedTerminalAuthorization {
    pub grant: Uuid,
    pub namespace: Uuid,
    pub surface_id: SurfaceId,
    pub session_id: SessionId,
    pub deadline: Instant,
}

#[derive(Debug, Error)]
pub enum TerminalSharingError {
    #[error("terminal sharing is disabled")]
    Disabled,
    #[error("terminal sharing grant is stale or invalid")]
    InvalidGrant,
    #[error("terminal identity namespace is stale")]
    StaleNamespace,
    #[error("terminal surface and session no longer identify the same terminal")]
    StaleTarget,
    #[error("terminal sharing authorization state is unavailable")]
    Unavailable,
    #[error("shared terminal request expired before executor admission")]
    AdmissionExpired,
}

impl TerminalSharingError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "sharing_disabled",
            Self::InvalidGrant => "invalid_grant",
            Self::StaleNamespace => "stale_namespace",
            Self::StaleTarget => "stale_target",
            Self::Unavailable => "internal",
            Self::AdmissionExpired => "admission_expired",
        }
    }
}

#[derive(Clone, Copy)]
struct Target {
    namespace: Uuid,
    surface_id: SurfaceId,
    session_id: SessionId,
}

impl SharedTerminalOperation {
    fn target(&self) -> Option<Target> {
        match self {
            Self::List => None,
            Self::Frame {
                namespace,
                surface_id,
                session_id,
            }
            | Self::Input {
                namespace,
                surface_id,
                session_id,
                ..
            }
            | Self::Key {
                namespace,
                surface_id,
                session_id,
                ..
            }
            | Self::Paste {
                namespace,
                surface_id,
                session_id,
                ..
            }
            | Self::Resize {
                namespace,
                surface_id,
                session_id,
                ..
            }
            | Self::Scroll {
                namespace,
                surface_id,
                session_id,
                ..
            } => Some(Target {
                namespace: *namespace,
                surface_id: *surface_id,
                session_id: *session_id,
            }),
        }
    }

    fn into_command(self) -> Option<Command> {
        Some(match self {
            Self::List | Self::Frame { .. } => return None,
            Self::Input {
                session_id, text, ..
            } => Command::Input { session_id, text },
            Self::Key {
                session_id, event, ..
            } => Command::Key { session_id, event },
            Self::Paste {
                session_id, text, ..
            } => Command::Paste { session_id, text },
            Self::Resize {
                session_id,
                rows,
                cols,
                ..
            } => Command::Resize {
                session_id,
                rows,
                cols,
            },
            Self::Scroll {
                session_id, lines, ..
            } => Command::Scroll { session_id, lines },
        })
    }
}

impl Desktop {
    /// Enable sharing and return the UUID used as the relay presence epoch.
    /// Repeated enable calls preserve the current grant while its namespace remains current.
    pub fn set_terminal_sharing(&self, enabled: bool) -> Result<Option<Uuid>, DesktopError> {
        if !enabled {
            self.revoke_terminal_sharing()?;
            return Ok(None);
        }
        if self.shared.stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err(DesktopError::Closed);
        }
        let mut state = self
            .shared
            .terminal_sharing
            .write()
            .map_err(|_| TerminalSharingError::Unavailable)?;
        if self.shared.stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err(DesktopError::Closed);
        }
        let snapshot = self.snapshot()?;
        if let Some(current) = *state
            && current.namespace == snapshot.model.identity_namespace
        {
            return Ok(Some(current.grant));
        }
        let grant = Uuid::new_v4();
        *state = Some(TerminalSharingGrant {
            grant,
            namespace: snapshot.model.identity_namespace,
        });
        Ok(Some(grant))
    }

    /// Execute one grant-scoped terminal operation. Mutations wait for the workspace executor to
    /// admit them to the existing session worker queue; that acknowledgment is acceptance, not a
    /// claim that the child process consumed the bytes.
    pub fn shared_terminal_operation(
        &self,
        envelope: SharedTerminalEnvelope,
    ) -> Result<SharedTerminalResponse, SharedTerminalCallError> {
        let sharing = self
            .shared
            .terminal_sharing
            .read()
            .map_err(|_| call_error(TerminalSharingError::Unavailable.into(), false))?;
        let scope = sharing
            .as_ref()
            .ok_or_else(|| call_error(TerminalSharingError::Disabled.into(), false))?;
        if scope.grant != envelope.grant {
            return Err(call_error(TerminalSharingError::InvalidGrant.into(), false));
        }
        let snapshot = self.snapshot().map_err(|error| call_error(error, false))?;
        if snapshot.model.identity_namespace != scope.namespace {
            return Err(call_error(
                TerminalSharingError::StaleNamespace.into(),
                false,
            ));
        }

        match envelope.request {
            SharedTerminalOperation::List => Ok(SharedTerminalResponse::List {
                ok: true,
                namespace: scope.namespace,
                sessions: list_terminals(&snapshot),
            }),
            SharedTerminalOperation::Frame {
                namespace,
                surface_id,
                session_id,
            } => {
                let target = Target {
                    namespace,
                    surface_id,
                    session_id,
                };
                validate_target(&snapshot.model, scope.namespace, target)
                    .map_err(|error| call_error(error.into(), false))?;
                let frame = snapshot
                    .control_frames
                    .iter()
                    .find(|frame| frame.session_id == session_id)
                    .cloned()
                    .ok_or_else(|| call_error(DesktopError::TerminalUnavailable, false))?;
                Ok(SharedTerminalResponse::Frame { ok: true, frame })
            }
            request => {
                let target = request.target().expect("mutation has a target");
                validate_target(&snapshot.model, scope.namespace, target)
                    .map_err(|error| call_error(error.into(), false))?;
                let command = request.into_command().expect("mutation has a command");
                let request = Request {
                    version: 1,
                    request_id: 0,
                    payload: command,
                };
                self.validate(&request)
                    .map_err(|error| call_error(error, false))?;
                drop(sharing);
                let (sender, receiver) = mpsc::sync_channel(1);
                let deadline = Instant::now() + SHARED_TERMINAL_TIMEOUT;
                self.send(crate::Envelope {
                    request,
                    acknowledgement: Some(sender),
                    authorization: None,
                    reply_deadline: None,
                    shared_terminal: Some(SharedTerminalAuthorization {
                        grant: envelope.grant,
                        namespace: target.namespace,
                        surface_id: target.surface_id,
                        session_id: target.session_id,
                        deadline,
                    }),
                })
                .map_err(|error| call_error(error, false))?;
                let execution = receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .map_err(|error| {
                        call_error(
                            match error {
                                mpsc::RecvTimeoutError::Timeout => DesktopError::Timeout,
                                mpsc::RecvTimeoutError::Disconnected => DesktopError::Closed,
                            },
                            true,
                        )
                    })?;
                execution.result.map_err(|error| call_error(error, false))?;
                Ok(SharedTerminalResponse::Accepted {
                    ok: true,
                    accepted: true,
                })
            }
        }
    }

    pub(crate) fn revoke_terminal_sharing(&self) -> Result<(), DesktopError> {
        self.shared.revoke_terminal_sharing()
    }
}

impl crate::Shared {
    pub(crate) fn revoke_terminal_sharing(&self) -> Result<(), DesktopError> {
        *self
            .terminal_sharing
            .write()
            .map_err(|_| TerminalSharingError::Unavailable)? = None;
        Ok(())
    }
}

pub(crate) fn authorize_at_executor(
    state: Option<&TerminalSharingGrant>,
    authorization: SharedTerminalAuthorization,
    model: &Model,
) -> Result<(), DesktopError> {
    if Instant::now() >= authorization.deadline {
        return Err(TerminalSharingError::AdmissionExpired.into());
    }
    let scope = state.ok_or(TerminalSharingError::Disabled)?;
    if scope.grant != authorization.grant {
        return Err(TerminalSharingError::InvalidGrant.into());
    }
    if scope.namespace != authorization.namespace
        || model.identity_namespace != authorization.namespace
    {
        return Err(TerminalSharingError::StaleNamespace.into());
    }
    Ok(validate_target(
        model,
        scope.namespace,
        Target {
            namespace: authorization.namespace,
            surface_id: authorization.surface_id,
            session_id: authorization.session_id,
        },
    )?)
}

fn list_terminals(snapshot: &Snapshot) -> Vec<SharedTerminalSession> {
    snapshot
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| {
            pane.surfaces.iter().filter_map(move |surface| {
                let session_id = surface.session_id?;
                (surface.kind == SurfaceKind::Terminal
                    && snapshot
                        .control_frames
                        .iter()
                        .any(|frame| frame.session_id == session_id))
                .then(|| SharedTerminalSession {
                    pane_id: pane.id,
                    surface_id: surface.id,
                    session_id,
                    title: surface
                        .title
                        .chars()
                        .take(MAX_SHARED_TERMINAL_TITLE_CHARS)
                        .collect(),
                })
            })
        })
        .collect()
}

fn validate_target(
    model: &Model,
    expected_namespace: Uuid,
    target: Target,
) -> Result<(), TerminalSharingError> {
    if target.namespace != expected_namespace || model.identity_namespace != expected_namespace {
        return Err(TerminalSharingError::StaleNamespace);
    }
    model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .any(|surface| {
            surface.kind == SurfaceKind::Terminal
                && surface.id == target.surface_id
                && surface.session_id == Some(target.session_id)
        })
        .then_some(())
        .ok_or(TerminalSharingError::StaleTarget)
}

fn call_error(error: DesktopError, outcome_unknown: bool) -> SharedTerminalCallError {
    SharedTerminalCallError {
        error,
        outcome_unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use zmux_workspace_core::WorkspaceStore;

    #[test]
    fn executor_rejects_expired_admission_before_reading_grant_or_target() {
        let model = WorkspaceStore::default().snapshot();
        let authorization = SharedTerminalAuthorization {
            grant: Uuid::new_v4(),
            namespace: model.identity_namespace,
            surface_id: SurfaceId(1),
            session_id: SessionId(1),
            deadline: Instant::now() - Duration::from_millis(1),
        };

        let error = authorize_at_executor(None, authorization, &model).unwrap_err();
        assert!(matches!(
            error,
            DesktopError::TerminalSharing(TerminalSharingError::AdmissionExpired)
        ));
    }

    #[test]
    fn executor_rejects_revoked_grant_before_reading_target() {
        let model = WorkspaceStore::default().snapshot();
        let authorization = SharedTerminalAuthorization {
            grant: Uuid::new_v4(),
            namespace: model.identity_namespace,
            surface_id: SurfaceId(1),
            session_id: SessionId(1),
            deadline: Instant::now() + Duration::from_secs(1),
        };

        let error = authorize_at_executor(None, authorization, &model).unwrap_err();
        assert!(matches!(
            error,
            DesktopError::TerminalSharing(TerminalSharingError::Disabled)
        ));
    }

    #[test]
    fn executor_rejects_expired_or_revoked_terminal_input_without_pty_output() {
        let desktop = Desktop::new(crate::Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap();
        let setup_deadline = Instant::now() + Duration::from_secs(5);
        let snapshot = loop {
            if let Ok(snapshot) = desktop.snapshot()
                && snapshot.control_frames.iter().any(|frame| {
                    frame.snapshot.as_ref().is_some_and(|terminal| {
                        terminal.state == zmux_session_runtime::State::Running
                    })
                })
            {
                break snapshot;
            }
            assert!(Instant::now() < setup_deadline, "terminal did not start");
            std::thread::sleep(Duration::from_millis(5));
        };
        let pane = snapshot.model.workspaces[0].root.panes()[0];
        let surface = pane.selected();
        let namespace = snapshot.model.identity_namespace;
        let surface_id = surface.id;
        let session_id = surface.session_id.unwrap();
        let grant = desktop.set_terminal_sharing(true).unwrap().unwrap();
        let marker = "EXPIRED_SHARED_TERMINAL_INPUT";
        let (acknowledgement, receiver) = mpsc::sync_channel(1);
        desktop
            .send(crate::Envelope {
                request: Request {
                    version: 1,
                    request_id: 991,
                    payload: Command::Input {
                        session_id,
                        text: format!("printf '{marker}\\n'\r"),
                    },
                },
                acknowledgement: Some(acknowledgement),
                authorization: None,
                reply_deadline: None,
                shared_terminal: Some(SharedTerminalAuthorization {
                    grant,
                    namespace,
                    surface_id,
                    session_id,
                    deadline: Instant::now() - Duration::from_millis(1),
                }),
            })
            .unwrap();
        let execution = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            execution.result,
            Err(DesktopError::TerminalSharing(
                TerminalSharingError::AdmissionExpired
            ))
        ));
        assert!(!execution.snapshot.control_frames.iter().any(|frame| {
            frame
                .snapshot
                .as_ref()
                .is_some_and(|terminal| terminal.viewport.contents.contains(marker))
        }));

        // Model work admitted with a valid grant but waiting at the executor when sharing is
        // disabled. Keep the captured authorization fresh so revocation, rather than expiry,
        // must refuse it at the real owner boundary.
        let revoked_authorization = SharedTerminalAuthorization {
            grant,
            namespace,
            surface_id,
            session_id,
            deadline: Instant::now() + Duration::from_secs(10),
        };
        desktop.set_terminal_sharing(false).unwrap();
        let marker = "REVOKED_SHARED_TERMINAL_INPUT";
        let (acknowledgement, receiver) = mpsc::sync_channel(1);
        desktop
            .send(crate::Envelope {
                request: Request {
                    version: 1,
                    request_id: 992,
                    payload: Command::Input {
                        session_id,
                        text: format!("printf '{marker}\\n'\r"),
                    },
                },
                acknowledgement: Some(acknowledgement),
                authorization: None,
                reply_deadline: None,
                shared_terminal: Some(revoked_authorization),
            })
            .unwrap();
        let execution = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            execution.result,
            Err(DesktopError::TerminalSharing(
                TerminalSharingError::Disabled
            ))
        ));
        let output_contains_marker = |snapshot: &Snapshot| {
            snapshot.control_frames.iter().any(|frame| {
                frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|terminal| terminal.viewport.contents.contains(marker))
            })
        };
        assert!(!output_contains_marker(&execution.snapshot));
        let observe_until = Instant::now() + Duration::from_millis(250);
        loop {
            assert!(!output_contains_marker(&desktop.snapshot().unwrap()));
            if Instant::now() >= observe_until {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}
