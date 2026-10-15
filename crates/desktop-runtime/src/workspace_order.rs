//! Resolve all order selectors on the workspace executor's immutable revision.
use crate::{DesktopError, Worker};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zmux_workspace_core::{
    Action, WindowId,
    targets::{CallerContext, TargetKind, TargetRef, TargetScope},
    workspace_order::{Placement, WorkspaceOrder},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderRequest {
    #[serde(default)]
    pub window: Option<TargetRef>,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    #[serde(default)]
    pub dry_run: bool,
    pub order: OrderCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum OrderCommand {
    Single {
        workspace: TargetRef,
        placement: OrderPlacement,
    },
    Bulk {
        workspaces: Vec<TargetRef>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "placement", rename_all = "snake_case", deny_unknown_fields)]
pub enum OrderPlacement {
    Index { index: i64 },
    Before { workspace: TargetRef },
    After { workspace: TargetRef },
}
impl Worker {
    pub(crate) fn workspace_order(&mut self, request: OrderRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        if let Some(expected) = request.expected_model_revision
            && model.revision != expected
        {
            return Err(zmux_workspace_core::targets::TargetError::StaleRevision {
                expected,
                actual: model.revision,
            }
            .into());
        }
        let context = || -> Result<WindowId, zmux_workspace_core::targets::TargetError> {
            if let Some(window) = &request.window {
                return model.resolve_window(window);
            }
            let caller = request.caller.bind_local_tmux(&model)?;
            if caller.workspace.is_some() || caller.surface.is_some() {
                // Validate both surviving object claims together. A stale or
                // ambiguous caller must not borrow the focused window.
                return TargetScope {
                    workspace: caller.workspace,
                    surface: caller.surface,
                    ..Default::default()
                }
                .resolve(&model, &CallerContext::default())
                .map(|t| t.window);
            }
            if let Some(window) = &caller.window {
                return model.resolve_window(window);
            }
            model
                .focused_window
                .ok_or(zmux_workspace_core::targets::TargetError::NoFocus(
                    TargetKind::Window,
                ))
        };
        let first = match &request.order {
            OrderCommand::Single { workspace, .. } => workspace,
            OrderCommand::Bulk { workspaces } => workspaces
                .first()
                .ok_or_else(|| DesktopError::Failure("workspace order must not be empty".into()))?,
        };
        let constraint = if request.window.is_some() || first.is_index()? {
            Some(context()?)
        } else {
            None
        };
        let (window, first_id) = model.resolve_order_workspace(first, constraint)?;
        let resolve = |handle: &TargetRef| {
            model
                .resolve_order_workspace(handle, Some(window))
                .map(|(_, id)| id)
        };
        let single = matches!(&request.order, OrderCommand::Single { .. });
        let order = match request.order {
            OrderCommand::Single { placement, .. } => WorkspaceOrder::Single {
                workspace_id: first_id,
                placement: match placement {
                    OrderPlacement::Index { index } => Placement::Index { index },
                    OrderPlacement::Before { workspace } => Placement::Before {
                        workspace_id: resolve(&workspace)?,
                    },
                    OrderPlacement::After { workspace } => Placement::After {
                        workspace_id: resolve(&workspace)?,
                    },
                },
            },
            OrderCommand::Bulk { workspaces } => WorkspaceOrder::Bulk {
                workspace_ids: workspaces.iter().map(resolve).collect::<Result<_, _>>()?,
            },
        };
        let plan = model
            .plan_workspace_order(window, &order)
            .map_err(|e| DesktopError::Failure(e.to_string()))?;
        if !request.dry_run {
            self.action(Action::ReorderWorkspaces {
                window_id: window,
                order,
            })?;
        }
        let current = self.model.snapshot();
        let rows: Vec<_> = plan.requested.iter().map(|row| json!({
            "workspace_id":row.workspace_id, "workspace_uuid":model.object_uuid(TargetKind::Workspace,row.workspace_id.0),
            "from_index":row.from_index, "to_index":row.to_index,
        })).collect();
        let events: Vec<_> = rows
            .iter()
            .filter(|row| !request.dry_run && row["from_index"] != row["to_index"])
            .cloned()
            .collect();
        let mut result = json!({"window_id":window,"window_uuid":model.object_uuid(TargetKind::Window,window.0),
            "dry_run":request.dry_run,"model_revision":current.revision,"plan":rows,"events":events,"workspace_ids":plan.workspace_ids});
        if single {
            let row = &plan.requested[0];
            result["workspace_id"] = json!(row.workspace_id);
            result["workspace_uuid"] =
                json!(model.object_uuid(TargetKind::Workspace, row.workspace_id.0));
            result["from_index"] = json!(row.from_index);
            result["to_index"] = json!(row.to_index);
            result["index"] = json!(row.to_index);
        }
        self.workspace_order_result = Some(result);
        Ok(())
    }
}
