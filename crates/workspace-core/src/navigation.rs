//! Navigation changes selection only; live sessions remain owned by their original surfaces.
use crate::{ActionError, Axis, PaneId, PaneTree, SurfaceId, WorkspaceId, WorkspaceStore};
use serde::{Deserialize, Serialize};

pub const MAX_FOCUS_HISTORY: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
impl Direction {
    pub(crate) fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Up | Self::Down => Axis::Vertical,
        }
    }
    pub(crate) fn before(self) -> bool {
        matches!(self, Self::Left | Self::Up)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Navigation {
    Next,
    Previous,
    Last,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusHistoryDirection {
    Back,
    Forward,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FocusHistory {
    back: Vec<SurfaceId>,
    forward: Vec<SurfaceId>,
    recent: Vec<SurfaceId>,
    recent_panes: Vec<PaneId>,
    recent_workspaces: Vec<WorkspaceId>,
}
fn push_bounded<T: Copy + PartialEq>(history: &mut Vec<T>, id: T) {
    if history.last() == Some(&id) {
        return;
    }
    if history.len() == MAX_FOCUS_HISTORY {
        history.remove(0);
    }
    history.push(id);
}
fn unavailable() -> ActionError {
    ActionError::InvalidOperation("no matching navigation target is available".into())
}
fn step(index: usize, len: usize, direction: Navigation) -> usize {
    match direction {
        Navigation::Next => (index + 1) % len,
        Navigation::Previous => (index + len - 1) % len,
        Navigation::Last => unreachable!("last is resolved from focus history"),
    }
}

impl WorkspaceStore {
    pub(crate) fn focused_surface(&self) -> Option<SurfaceId> {
        self.focused().map(|(_, _, surface)| surface)
    }

    pub(crate) fn record_focus(
        &mut self,
        prior: Option<(WorkspaceId, PaneId, SurfaceId)>,
        history_navigation: bool,
    ) {
        let live: Vec<_> = self
            .state
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| p.surfaces.iter().map(|s| s.id))
            .collect();
        self.focus_history.back.retain(|id| live.contains(id));
        self.focus_history.forward.retain(|id| live.contains(id));
        self.focus_history.recent.retain(|id| live.contains(id));
        let live_panes: Vec<_> = self
            .state
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .map(|p| p.id)
            .collect();
        let live_workspaces: Vec<_> = self.state.workspaces.iter().map(|w| w.id).collect();
        self.focus_history
            .recent_panes
            .retain(|id| live_panes.contains(id));
        self.focus_history
            .recent_workspaces
            .retain(|id| live_workspaces.contains(id));
        let current = self.focused();
        if prior != current {
            if !history_navigation {
                if let Some((_, _, id)) = prior.filter(|(_, _, id)| live.contains(id)) {
                    push_bounded(&mut self.focus_history.back, id);
                }
                self.focus_history.forward.clear();
            }
            if let Some((workspace, pane, surface)) = current {
                self.focus_history
                    .recent
                    .retain(|previous| *previous != surface);
                push_bounded(&mut self.focus_history.recent, surface);
                self.focus_history
                    .recent_panes
                    .retain(|previous| *previous != pane);
                push_bounded(&mut self.focus_history.recent_panes, pane);
                self.focus_history
                    .recent_workspaces
                    .retain(|previous| *previous != workspace);
                push_bounded(&mut self.focus_history.recent_workspaces, workspace);
            }
        }
    }

    pub(crate) fn select_surface(&mut self, id: SurfaceId) -> Result<(), ActionError> {
        let (wi, pane_id, _) = self.location(id)?;
        self.state.workspaces[wi]
            .root
            .pane_mut(pane_id)
            .expect("validated pane")
            .selected_surface = id;
        self.focus(wi, pane_id);
        Ok(())
    }

    pub(crate) fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), ActionError> {
        let workspace = self
            .state
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .ok_or(ActionError::UnknownWorkspace(id))?;
        let pane = workspace.focused_pane;
        let wi = self.workspace_index(pane)?;
        self.focus(wi, pane);
        Ok(())
    }

    pub(crate) fn navigate_workspace(
        &mut self,
        id: WorkspaceId,
        direction: Navigation,
    ) -> Result<(), ActionError> {
        let wi = self
            .state
            .workspaces
            .iter()
            .position(|w| w.id == id)
            .ok_or(ActionError::UnknownWorkspace(id))?;
        let destination = if direction == Navigation::Last {
            self.focus_history
                .recent_workspaces
                .iter()
                .rev()
                .copied()
                .find(|workspace| *workspace != id)
                .ok_or_else(unavailable)?
        } else {
            self.state.workspaces[step(wi, self.state.workspaces.len(), direction)].id
        };
        self.select_workspace(destination)
    }

    pub(crate) fn navigate_surface(
        &mut self,
        id: SurfaceId,
        direction: Navigation,
    ) -> Result<(), ActionError> {
        let (_, pane_id, index) = self.location(id)?;
        let destination = if direction == Navigation::Last {
            self.focus_history
                .recent
                .iter()
                .rev()
                .find_map(|surface| {
                    let (_, owner, _) = self.location(*surface).ok()?;
                    (owner == pane_id && *surface != id).then_some(*surface)
                })
                .ok_or_else(unavailable)?
        } else {
            let pane = self.pane(pane_id)?;
            pane.surfaces[step(index, pane.surfaces.len(), direction)].id
        };
        self.select_surface(destination)
    }

    pub(crate) fn navigate_pane(
        &mut self,
        id: PaneId,
        direction: Navigation,
    ) -> Result<(), ActionError> {
        let wi = self.workspace_index(id)?;
        let panes = self.state.workspaces[wi].root.panes();
        let destination = if direction == Navigation::Last {
            self.focus_history
                .recent_panes
                .iter()
                .rev()
                .copied()
                .find(|pane| *pane != id && self.workspace_index(*pane).ok() == Some(wi))
                .ok_or_else(unavailable)?
        } else {
            let index = panes
                .iter()
                .position(|pane| pane.id == id)
                .expect("validated pane");
            panes[step(index, panes.len(), direction)].id
        };
        self.focus(wi, destination);
        Ok(())
    }

    pub(crate) fn navigate_focus(
        &mut self,
        direction: FocusHistoryDirection,
    ) -> Result<(), ActionError> {
        let current = self.focused_surface();
        loop {
            let candidate = match direction {
                FocusHistoryDirection::Back => self.focus_history.back.pop(),
                FocusHistoryDirection::Forward => self.focus_history.forward.pop(),
            }
            .ok_or_else(unavailable)?;
            if Some(candidate) == current || self.location(candidate).is_err() {
                continue;
            }
            if let Some(current) = current {
                let destination = match direction {
                    FocusHistoryDirection::Back => &mut self.focus_history.forward,
                    FocusHistoryDirection::Forward => &mut self.focus_history.back,
                };
                push_bounded(destination, current);
            }
            return self.select_surface(candidate);
        }
    }

    pub(crate) fn focus_pane_direction(
        &mut self,
        id: PaneId,
        direction: Direction,
    ) -> Result<(), ActionError> {
        let wi = self.workspace_index(id)?;
        let mut rectangles = Vec::new();
        self.state.workspaces[wi].root.rectangles(
            Rect {
                x: 0,
                y: 0,
                width: 1 << 16,
                height: 1 << 16,
            },
            &mut rectangles,
        );
        let source = rectangles
            .iter()
            .find(|(pane, _)| *pane == id)
            .expect("validated pane")
            .1;
        let destination = rectangles
            .into_iter()
            .filter(|(pane, _)| *pane != id)
            .filter_map(|(pane, rect)| {
                source
                    .neighbor_rank(rect, direction)
                    .map(|rank| (rank, pane))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, pane)| pane)
            .ok_or_else(unavailable)?;
        self.focus(wi, destination);
        Ok(())
    }
}

/// Integer normalized layout remains exact at the model's maximum split depth.
/// Pane navigation follows topology; native divider ratios are presentation-owned.
#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
impl Rect {
    fn neighbor_rank(self, other: Self, direction: Direction) -> Option<(u32, u32, u32)> {
        let (distance, start, end, other_start, other_end) = match direction {
            Direction::Left if other.x + other.width <= self.x => (
                self.x - other.x - other.width,
                self.y,
                self.y + self.height,
                other.y,
                other.y + other.height,
            ),
            Direction::Right if self.x + self.width <= other.x => (
                other.x - self.x - self.width,
                self.y,
                self.y + self.height,
                other.y,
                other.y + other.height,
            ),
            Direction::Up if other.y + other.height <= self.y => (
                self.y - other.y - other.height,
                self.x,
                self.x + self.width,
                other.x,
                other.x + other.width,
            ),
            Direction::Down if self.y + self.height <= other.y => (
                other.y - self.y - self.height,
                self.x,
                self.x + self.width,
                other.x,
                other.x + other.width,
            ),
            _ => return None,
        };
        let overlap = end.min(other_end).saturating_sub(start.max(other_start));
        (overlap > 0).then_some((
            distance,
            u32::MAX - overlap,
            (start + end).abs_diff(other_start + other_end),
        ))
    }
}
impl PaneTree {
    fn rectangles(&self, rect: Rect, result: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Leaf { pane } => result.push((pane.id, rect)),
            Self::Split {
                axis,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    Axis::Horizontal => {
                        let width = rect.width / 2;
                        (
                            Rect { width, ..rect },
                            Rect {
                                x: rect.x + width,
                                width: rect.width - width,
                                ..rect
                            },
                        )
                    }
                    Axis::Vertical => {
                        let height = rect.height / 2;
                        (
                            Rect { height, ..rect },
                            Rect {
                                y: rect.y + height,
                                height: rect.height - height,
                                ..rect
                            },
                        )
                    }
                };
                first.rectangles(a, result);
                second.rectangles(b, result);
            }
        }
    }
}
