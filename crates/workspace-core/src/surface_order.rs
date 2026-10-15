//! Pure final-position planning for a single pane's existing live tabs.
use crate::{ActionError, PaneId, Snapshot, SurfaceId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "placement", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    Index { index: i64 },
    Offset { offset: i64 },
    Before { surface_id: SurfaceId },
    After { surface_id: SurfaceId },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub surface_id: SurfaceId,
    pub pane_id: PaneId,
    pub from_index: usize,
    pub to_index: usize,
}
impl Snapshot {
    pub fn plan_surface_order(
        &self,
        surface_id: SurfaceId,
        placement: &Placement,
    ) -> Result<Plan, ActionError> {
        let (pane, from_index) = self
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .find_map(|p| {
                p.surfaces
                    .iter()
                    .position(|s| s.id == surface_id)
                    .map(|i| (p, i))
            })
            .ok_or(ActionError::UnknownSurface(surface_id))?;
        let to_index = match placement {
            Placement::Offset { offset } => (from_index as i64)
                .saturating_add(*offset)
                .clamp(0, (pane.surfaces.len() - 1) as i64)
                as usize,
            Placement::Index { index } => {
                (*index).clamp(0, (pane.surfaces.len() - 1) as i64) as usize
            }
            Placement::Before { surface_id: anchor } | Placement::After { surface_id: anchor } => {
                let anchor_index = pane
                    .surfaces
                    .iter()
                    .position(|s| s.id == *anchor)
                    .ok_or_else(|| {
                        ActionError::InvalidOperation(
                            "reorder anchor must belong to the same pane".into(),
                        )
                    })?;
                if *anchor == surface_id {
                    from_index
                } else {
                    anchor_index + usize::from(matches!(placement, Placement::After { .. }))
                        - usize::from(from_index < anchor_index)
                }
            }
        };
        Ok(Plan {
            surface_id,
            pane_id: pane.id,
            from_index,
            to_index,
        })
    }
}
