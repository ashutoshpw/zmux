//! Navigation changes selection only; live sessions remain owned by their original surfaces.
use crate::{
    ActionError, Axis, PaneId, PaneTree, SurfaceId, WindowId, WorkspaceId, WorkspaceStore,
};
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

    fn history(&self, window: WindowId) -> FocusHistory {
        self.focus_history.get(&window).cloned().unwrap_or_default()
    }
    fn window_for_workspace(&self, id: WorkspaceId) -> Result<WindowId, ActionError> {
        self.state
            .windows
            .iter()
            .find(|window| window.workspace_ids.contains(&id))
            .map(|window| window.id)
            .ok_or_else(unavailable)
    }
    pub(crate) fn record_focus(
        &mut self,
        prior: Option<(WorkspaceId, PaneId, SurfaceId)>,
        prior_window: Option<WindowId>,
        history_navigation: bool,
        record_current: bool,
    ) {
        self.focus_history
            .retain(|window, _| self.state.windows.iter().any(|live| live.id == *window));
        for window in &self.state.windows {
            let workspaces: Vec<_> = self
                .state
                .workspaces
                .iter()
                .filter(|w| window.workspace_ids.contains(&w.id))
                .collect();
            let live: Vec<_> = workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .flat_map(|p| p.surfaces.iter().map(|s| s.id))
                .collect();
            let panes: Vec<_> = workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .map(|p| p.id)
                .collect();
            let history = self.focus_history.entry(window.id).or_default();
            history.back.retain(|id| live.contains(id));
            history.forward.retain(|id| live.contains(id));
            history.recent.retain(|id| live.contains(id));
            history.recent_panes.retain(|id| panes.contains(id));
            history
                .recent_workspaces
                .retain(|id| window.workspace_ids.contains(id));
        }
        let current = self.focused();
        if !record_current || prior == current {
            return;
        }
        let Some(window) = self.state.focused_window else {
            return;
        };
        let prior = prior
            .filter(|(workspace, _, _)| self.window_for_workspace(*workspace).ok() == Some(window));
        let history = self.focus_history.entry(window).or_default();
        if !history_navigation && prior_window == Some(window) {
            if let Some((_, _, id)) = prior {
                push_bounded(&mut history.back, id);
            }
            history.forward.clear();
        }
        if let Some((workspace, pane, surface)) = current {
            history.recent.retain(|previous| *previous != surface);
            push_bounded(&mut history.recent, surface);
            history.recent_panes.retain(|previous| *previous != pane);
            push_bounded(&mut history.recent_panes, pane);
            history
                .recent_workspaces
                .retain(|previous| *previous != workspace);
            push_bounded(&mut history.recent_workspaces, workspace);
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
        let window = self.window_for_workspace(id)?;
        let members = &self
            .state
            .windows
            .iter()
            .find(|w| w.id == window)
            .expect("owned window")
            .workspace_ids;
        let wi = members
            .iter()
            .position(|workspace| *workspace == id)
            .ok_or_else(unavailable)?;
        let destination = if direction == Navigation::Last {
            self.history(window)
                .recent_workspaces
                .iter()
                .rev()
                .copied()
                .find(|workspace| *workspace != id)
                .ok_or_else(unavailable)?
        } else {
            members[step(wi, members.len(), direction)]
        };
        self.select_workspace(destination)
    }

    pub(crate) fn navigate_surface(
        &mut self,
        id: SurfaceId,
        direction: Navigation,
    ) -> Result<(), ActionError> {
        let (_, pane_id, index) = self.location(id)?;
        let destination =
            if direction == Navigation::Last {
                self.history(self.window_for_workspace(
                    self.state.workspaces[self.workspace_index(pane_id)?].id,
                )?)
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
            self.history(self.window_for_workspace(self.state.workspaces[wi].id)?)
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
        let window = self.state.focused_window.ok_or_else(unavailable)?;
        loop {
            let history = self.focus_history.entry(window).or_default();
            let candidate = match direction {
                FocusHistoryDirection::Back => history.back.pop(),
                FocusHistoryDirection::Forward => history.forward.pop(),
            }
            .ok_or_else(unavailable)?;
            let live = self.location(candidate).ok().and_then(|(wi, _, _)| {
                self.window_for_workspace(self.state.workspaces[wi].id).ok()
            }) == Some(window);
            if Some(candidate) == current || !live {
                continue;
            }
            if let Some(current) = current {
                let history = self.focus_history.entry(window).or_default();
                let destination = match direction {
                    FocusHistoryDirection::Back => &mut history.forward,
                    FocusHistoryDirection::Forward => &mut history.back,
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
