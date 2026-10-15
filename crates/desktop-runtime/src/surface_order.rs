//! Same-pane placement resolves and commits on one workspace-executor revision.
use crate::{DesktopError, Worker};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zmux_workspace_core::{
    Action,
    surface_order::Placement,
    targets::{CallerContext, TargetKind, TargetRef, TargetScope},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default)]
    pub identity_namespace: Option<uuid::Uuid>,
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    #[serde(default)]
    pub focus: bool,
    pub placement: OrderPlacement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "placement", rename_all = "snake_case", deny_unknown_fields)]
pub enum OrderPlacement {
    Index { index: i64 },
    Before { surface: TargetRef },
    After { surface: TargetRef },
    Offset { offset: i64 },
}
impl Worker {
    pub(crate) fn surface_order(&mut self, mut request: OrderRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        if request
            .identity_namespace
            .is_some_and(|namespace| namespace != model.identity_namespace)
        {
            return Err(DesktopError::Failure(
                "surface order identity namespace changed".into(),
            ));
        }
        if let Some(expected) = request.expected_model_revision
            && expected != model.revision
        {
            return Err(zmux_workspace_core::targets::TargetError::StaleRevision {
                expected,
                actual: model.revision,
            }
            .into());
        }
        // Explicit workspace indices use the chosen window's current canonical
        // membership; surface/pane indices then resolve inside that workspace.
        if let Some(workspace) = &request.scope.workspace
            && workspace.is_index()?
        {
            let window = TargetScope {
                window: request.scope.window.clone(),
                ..Default::default()
            }
            .resolve_window(&model, &request.caller)?;
            let (_, id) = model.resolve_order_workspace(workspace, Some(window))?;
            request.scope.workspace = Some(format!("workspace:{}", id.0).parse()?);
        }
        let source = request.scope.resolve(&model, &request.caller)?;
        let anchor = |surface: TargetRef| -> Result<_, DesktopError> {
            Ok(TargetScope {
                window: Some(format!("window:{}", source.window.0).parse()?),
                workspace: Some(format!("workspace:{}", source.workspace.0).parse()?),
                pane: Some(format!("pane:{}", source.pane.0).parse()?),
                surface: Some(surface),
            }
            .resolve(&model, &CallerContext::default())?
            .surface)
        };
        let placement = match request.placement {
            OrderPlacement::Index { index } => Placement::Index { index },
            OrderPlacement::Before { surface } => Placement::Before {
                surface_id: anchor(surface)?,
            },
            OrderPlacement::After { surface } => Placement::After {
                surface_id: anchor(surface)?,
            },
            OrderPlacement::Offset { offset } => Placement::Offset { offset },
        };
        let plan = model
            .plan_surface_order(source.surface, &placement)
            .map_err(|error| DesktopError::Failure(error.to_string()))?;
        // Reserve an exact fresh native focus request before committing order.
        // A queue/host failure must not leave a partially accepted reorder.
        let mut native = None;
        if request.focus {
            if self.windows.host(source.window).is_none() {
                return Err(DesktopError::Failure(
                    "surface order native window host is not ready".into(),
                ));
            }
            let mut candidate = self.windows.clone();
            let receipt = candidate.request_move_focus(&model, source.window)?;
            native = Some((candidate, receipt));
        }
        self.action(Action::ReorderSurface {
            surface_id: source.surface,
            index: plan.to_index,
            focus: request.focus,
        })?;
        let native_focus = if let Some((candidate, receipt)) = native {
            self.windows = candidate;
            self.acknowledge_focused();
            Some(receipt)
        } else {
            None
        };
        self.surface_order_result = Some(json!({
            "state":"order_committed","surface_id":source.surface,"surface_uuid":model.object_uuid(TargetKind::Surface,source.surface.0),
            "pane_id":source.pane,"pane_uuid":model.object_uuid(TargetKind::Pane,source.pane.0),
            "workspace_id":source.workspace,"workspace_uuid":model.object_uuid(TargetKind::Workspace,source.workspace.0),
            "window_id":source.window,"window_uuid":model.object_uuid(TargetKind::Window,source.window.0),
            "from_index":plan.from_index,"to_index":plan.to_index,"index":plan.to_index,
            "focus_requested":request.focus,"native_focus":native_focus,"model_revision":self.model.snapshot().revision
        }));
        Ok(())
    }
}
