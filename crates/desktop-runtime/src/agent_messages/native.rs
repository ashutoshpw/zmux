//! Trusted presentation requests. Attribution is captured on the workspace executor,
//! never from another window's focus or from caller-supplied surface provenance.
use super::{MessageAction, MessageRequest, OperationReceipt, Runtime, SettingScope};
use crate::{DesktopError, agent_messages::domain};
use serde::Deserialize;
use uuid::Uuid;
use zmux_workspace_core::{Snapshot, SurfaceKind, targets::CallerContext};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOrigin {
    pub namespace: Uuid,
    pub window_id: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub origin: NativeOrigin,
    pub request_id: Uuid,
    pub action: NativeAction,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeAction {
    List {
        cursor: Option<u64>,
    },
    Reply {
        reply_to: Uuid,
        body: String,
    },
    MarkRead {
        ids: Vec<Uuid>,
    },
    Settings {
        scope: SettingScope,
        target: Option<String>,
        enabled: Option<bool>,
    },
    Status,
}
pub struct NativeAdmissionError {
    pub error: DesktopError,
    pub outcome_unknown: bool,
}

impl Runtime {
    pub(crate) fn native(
        &mut self,
        request: NativeRequest,
        model: &Snapshot,
    ) -> Result<OperationReceipt, DesktopError> {
        let refusal = |message: &str| DesktopError::Failure(message.into());
        if request.origin.namespace != model.identity_namespace || request.request_id.is_nil() {
            return Err(refusal(
                "native message origin namespace or request ID is stale",
            ));
        }
        let window = model
            .windows
            .iter()
            .find(|window| window.id.0 == request.origin.window_id)
            .ok_or_else(|| refusal("native message origin window is closed"))?;
        if matches!(&request.action, NativeAction::Status) {
            return self.status(request.request_id, None);
        }
        if let NativeAction::Settings { scope, target, .. } = &request.action
            && !matches!(scope, SettingScope::Global)
            && target.is_none()
        {
            return Err(refusal(
                "native receiving settings require an explicit target",
            ));
        }
        let selected = window
            .selected_workspace
            .and_then(|id| model.workspaces.iter().find(|w| w.id == id))
            .and_then(|workspace| {
                workspace
                    .root
                    .panes()
                    .into_iter()
                    .find(|pane| pane.id == workspace.focused_pane)
            })
            .and_then(|pane| {
                pane.surfaces
                    .iter()
                    .find(|surface| surface.id == pane.selected_surface)
            })
            .filter(|surface| {
                surface.kind == SurfaceKind::Terminal && surface.session_id.is_some()
            });
        // An empty window or selected nonterminal is explicitly an external local user.
        // Passing only a window hint would let TargetScope choose a different surface.
        let caller = if matches!(&request.action, NativeAction::Reply { .. })
            && let Some(surface) = selected
        {
            let live = domain::surfaces(model)
                .into_iter()
                .find(|live| {
                    live.address.surface_id
                        == model.object_uuid(
                            zmux_workspace_core::targets::TargetKind::Surface,
                            surface.id.0,
                        )
                })
                .ok_or_else(|| refusal("native sender terminal is no longer live"))?;
            CallerContext {
                window: None,
                workspace: Some(live.address.workspace_id.to_string().parse()?),
                surface: Some(live.address.surface_id.to_string().parse()?),
                local_tmux: None,
            }
        } else {
            CallerContext::default()
        };
        let action = match request.action {
            NativeAction::Status => unreachable!("native status returned before sender resolution"),
            NativeAction::List { cursor } => MessageAction::List {
                surface: None,
                states: Vec::new(),
                limit: super::MAX_PAGE,
                cursor,
            },
            NativeAction::Reply { reply_to, body } => MessageAction::Send {
                target: None,
                reply_to: Some(reply_to),
                body,
                sender_name: None,
                thread_id: None,
            },
            NativeAction::MarkRead { ids } => MessageAction::MarkRead { ids },
            NativeAction::Settings {
                scope,
                target,
                enabled,
            } => MessageAction::Settings {
                scope,
                target,
                enabled,
            },
        };
        self.admit(
            MessageRequest {
                request_id: request.request_id,
                caller,
                action,
            },
            None,
        )
    }
}
