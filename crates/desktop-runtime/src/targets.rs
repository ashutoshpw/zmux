//! Control requests retain symbolic targets until the workspace executor accepts the operation.
use serde::{Deserialize, Serialize};
use zmux_workspace_core::{
    Snapshot,
    targets::{CallerContext, TargetAction, TargetError, TargetScope},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    /// Optional optimistic concurrency guard for callers acting on an observed catalog/index.
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    pub operation: TargetOperation,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetOperation {
    SelectWorkspaceNumber {
        digit: u8,
    },
    Action {
        action: TargetAction,
    },
    PasteFiles {
        paths: Vec<std::path::PathBuf>,
    },
    SendText {
        text: String,
    },
    Notify {
        #[serde(default = "crate::attention::default_notification_title")]
        title: String,
        #[serde(default)]
        subtitle: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        desktop: Option<bool>,
        #[serde(default)]
        reply: bool,
    },
    Notifications {
        action: crate::attention::NotificationAction,
    },
    Metadata {
        action: crate::metadata::MetadataAction,
    },
    ReadScreen,
    #[default]
    ListPanes,
    ListPaneSurfaces,
}
impl TargetRequest {
    pub fn validate_revision(&self, model: &Snapshot) -> Result<(), TargetError> {
        if let Some(expected) = self.expected_model_revision
            && expected != model.revision
        {
            return Err(TargetError::StaleRevision {
                expected,
                actual: model.revision,
            });
        }
        Ok(())
    }
}

/// Notification attribution must not borrow mutable pane selection when only a
/// workspace has been proven. Other target operations keep their usual defaults.
pub(crate) fn notification_address(
    model: &Snapshot,
    scope: &TargetScope,
    caller: &CallerContext,
) -> Result<crate::attention::NotificationAddress, TargetError> {
    let address = notification_address_unchecked(model, scope, caller)?;
    if let Some(window) = &scope.window {
        let expected = model.resolve_window(window)?;
        let workspace = match address {
            crate::attention::NotificationAddress::Workspace(id) => id,
            crate::attention::NotificationAddress::Surface(id) => {
                model
                    .workspaces
                    .iter()
                    .find(|w| {
                        w.root
                            .panes()
                            .iter()
                            .any(|p| p.surfaces.iter().any(|s| s.id == id))
                    })
                    .ok_or(TargetError::NoFocus(
                        zmux_workspace_core::targets::TargetKind::Workspace,
                    ))?
                    .id
            }
        };
        if !model
            .windows
            .iter()
            .any(|window| window.id == expected && window.workspace_ids.contains(&workspace))
        {
            return Err(TargetError::ScopeMismatch {
                kind: zmux_workspace_core::targets::TargetKind::Window,
            });
        }
    }
    Ok(address)
}
fn notification_address_unchecked(
    model: &Snapshot,
    scope: &TargetScope,
    caller: &CallerContext,
) -> Result<crate::attention::NotificationAddress, TargetError> {
    use crate::attention::NotificationAddress;
    if scope.surface.is_some() || scope.pane.is_some() {
        return scope
            .resolve(model, &CallerContext::default())
            .map(|target| NotificationAddress::Surface(target.surface));
    }
    if scope.workspace.is_some() {
        let workspace = scope.resolve(model, &CallerContext::default())?.workspace;
        if let Some(surface) = &caller.surface {
            let constrained = TargetScope {
                workspace: scope.workspace.clone(),
                surface: Some(surface.clone()),
                ..Default::default()
            };
            // An ambient surface may identify a pane inside the explicit
            // workspace, but stale or moved claims cannot cross its hard scope.
            if let Ok(target) = constrained.resolve(model, &CallerContext::default()) {
                return Ok(NotificationAddress::Surface(target.surface));
            }
        }
        return Ok(NotificationAddress::Workspace(workspace));
    }
    let mut surface_error = None;
    if let Some(surface) = &caller.surface {
        let scope = TargetScope {
            surface: Some(surface.clone()),
            ..Default::default()
        };
        match scope.resolve(model, &CallerContext::default()) {
            Ok(target) => return Ok(NotificationAddress::Surface(target.surface)),
            Err(error) => surface_error = Some(error),
        }
    }
    if let Some(workspace) = &caller.workspace {
        let scope = TargetScope {
            workspace: Some(workspace.clone()),
            ..Default::default()
        };
        return scope
            .resolve(model, &CallerContext::default())
            .map(|target| NotificationAddress::Workspace(target.workspace));
    }
    if let Some(error) = surface_error {
        return Err(error);
    }
    scope
        .resolve(model, caller)
        .map(|target| NotificationAddress::Workspace(target.workspace))
}

/// Metadata belongs to the caller workspace when present. Validate an accompanying
/// surface claim instead of borrowing foreground selection or crossing owners.
pub(crate) fn metadata_workspace(
    model: &Snapshot,
    scope: &TargetScope,
    caller: &CallerContext,
) -> Result<zmux_workspace_core::WorkspaceId, TargetError> {
    if scope.workspace.is_some() || scope.pane.is_some() || scope.surface.is_some() {
        return scope
            .resolve(model, &CallerContext::default())
            .map(|t| t.workspace);
    }
    TargetScope {
        window: scope.window.clone(),
        workspace: caller.workspace.clone(),
        surface: caller.surface.clone(),
        ..Default::default()
    }
    .resolve(model, caller)
    .map(|t| t.workspace)
}
