//! Identity-preserving ordering of a window's canonical workspace membership.
use crate::{ActionError, Snapshot, WindowId, WorkspaceId, WorkspaceStore};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "placement", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    Index { index: i64 },
    Before { workspace_id: WorkspaceId },
    After { workspace_id: WorkspaceId },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceOrder {
    Single {
        workspace_id: WorkspaceId,
        placement: Placement,
    },
    Bulk {
        workspace_ids: Vec<WorkspaceId>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OrderRow {
    pub workspace_id: WorkspaceId,
    pub from_index: usize,
    pub to_index: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OrderPlan {
    pub window_id: WindowId,
    pub workspace_ids: Vec<WorkspaceId>,
    /// Requested workspaces, including those whose index stays unchanged.
    pub requested: Vec<OrderRow>,
}
fn invalid(message: &str) -> ActionError {
    ActionError::InvalidOperation(message.into())
}
impl Snapshot {
    /// Pure planning is shared by preview and the mutation owner. All validation
    /// happens before the caller can replace a window's membership.
    pub fn plan_workspace_order(
        &self,
        window_id: WindowId,
        request: &WorkspaceOrder,
    ) -> Result<OrderPlan, ActionError> {
        let original = &self
            .windows
            .iter()
            .find(|w| w.id == window_id)
            .ok_or_else(|| invalid("reorder window does not exist"))?
            .workspace_ids;
        let position = |id: WorkspaceId| {
            original
                .iter()
                .position(|candidate| *candidate == id)
                .ok_or_else(|| invalid("reorder workspace does not belong to the window"))
        };
        let mut ordered = original.clone();
        let requested = match request {
            WorkspaceOrder::Single {
                workspace_id,
                placement,
            } => {
                let from = position(*workspace_id)?;
                let maximum = original.len() - 1;
                let to = match placement {
                    Placement::Index { index } => (*index).clamp(0, maximum as i64) as usize,
                    Placement::Before {
                        workspace_id: anchor,
                    } => {
                        let anchor = position(*anchor)?;
                        anchor - usize::from(from < anchor)
                    }
                    Placement::After {
                        workspace_id: anchor,
                    } => {
                        let anchor = position(*anchor)?;
                        // Frozen command behavior advances after-self one place,
                        // clamped at the end; before-self stays in place.
                        (anchor + 1 - usize::from(from < anchor)).min(maximum)
                    }
                };
                ordered.remove(from);
                ordered.insert(to, *workspace_id);
                vec![*workspace_id]
            }
            WorkspaceOrder::Bulk { workspace_ids } => {
                if workspace_ids.is_empty() {
                    return Err(invalid("workspace order must not be empty"));
                }
                let mut unique = HashSet::new();
                if workspace_ids.iter().any(|id| !unique.insert(*id)) {
                    return Err(invalid("workspace order contains duplicate workspaces"));
                }
                for id in workspace_ids {
                    position(*id)?;
                }
                ordered = workspace_ids
                    .iter()
                    .copied()
                    .chain(original.iter().copied().filter(|id| !unique.contains(id)))
                    .collect();
                workspace_ids.clone()
            }
        };
        let requested = requested
            .into_iter()
            .map(|workspace_id| OrderRow {
                workspace_id,
                from_index: original
                    .iter()
                    .position(|id| *id == workspace_id)
                    .expect("validated membership"),
                to_index: ordered
                    .iter()
                    .position(|id| *id == workspace_id)
                    .expect("preserved membership"),
            })
            .collect();
        self.check_group_order(window_id, &ordered)?;
        Ok(OrderPlan {
            window_id,
            workspace_ids: ordered,
            requested,
        })
    }
}
impl WorkspaceStore {
    pub(crate) fn reorder_workspaces(
        &mut self,
        window_id: WindowId,
        order: WorkspaceOrder,
    ) -> Result<(), ActionError> {
        let plan = self.state.plan_workspace_order(window_id, &order)?;
        self.state
            .windows
            .iter_mut()
            .find(|w| w.id == window_id)
            .expect("validated window")
            .workspace_ids = plan.workspace_ids;
        Ok(())
    }
}
