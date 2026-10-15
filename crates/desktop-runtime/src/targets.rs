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
        .resolve(model, &CallerContext::default())
        .map(|target| NotificationAddress::Workspace(target.workspace))
}
