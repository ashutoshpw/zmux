//! Canonical main-window membership, selection and identity-preserving moves.
use crate::{Action, ActionError, Effect, WindowId, WorkspaceId, WorkspaceStore};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_WINDOWS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub id: WindowId,
    pub workspace_ids: Vec<WorkspaceId>,
    pub selected_workspace: Option<WorkspaceId>,
}

pub(crate) fn validate_layout(
    workspace_ids: &[WorkspaceId],
    windows: &[Window],
    focused_window: Option<WindowId>,
) -> Result<Option<WorkspaceId>, &'static str> {
    if windows.len() > MAX_WINDOWS {
        return Err("window limit exceeded; multiple native windows are not available");
    }
    if windows.is_empty() != focused_window.is_none()
        || focused_window.is_some_and(|id| !windows.iter().any(|window| window.id == id))
    {
        return Err("focused window does not belong to layout");
    }
    let mut membership = HashSet::new();
    let mut window_ids = HashSet::new();
    for window in windows {
        if window.id.0 == 0 || !window_ids.insert(window.id) {
            return Err("window IDs must be nonzero and unique");
        }
        if window.workspace_ids.is_empty() != window.selected_workspace.is_none()
            || window
                .selected_workspace
                .is_some_and(|id| !window.workspace_ids.contains(&id))
        {
            return Err("selected workspace does not belong to window");
        }
        for id in &window.workspace_ids {
            if !workspace_ids.contains(id) || !membership.insert(*id) {
                return Err("workspace window membership must be unique and live");
            }
        }
    }
    if membership.len() != workspace_ids.len() {
        return Err("every workspace must belong to a window");
    }
    Ok(windows
        .iter()
        .find(|window| Some(window.id) == focused_window)
        .and_then(|window| window.selected_workspace))
}

impl WorkspaceStore {
    pub(crate) fn move_workspace_to_window(
        &mut self,
        workspace: WorkspaceId,
        destination: WindowId,
        index: Option<usize>,
        focus: bool,
    ) -> Result<(), ActionError> {
        let source = self
            .state
            .windows
            .iter()
            .position(|window| window.workspace_ids.contains(&workspace))
            .ok_or(ActionError::UnknownWorkspace(workspace))?;
        let target = self
            .state
            .windows
            .iter()
            .position(|window| window.id == destination)
            .ok_or_else(|| {
                ActionError::InvalidOperation("destination window does not exist".into())
            })?;
        if source == target && index.is_none() {
            return Ok(());
        }
        let length = self.state.windows[target].workspace_ids.len() - usize::from(source == target);
        let index = index.unwrap_or(length);
        if index > length {
            return Err(ActionError::InvalidOperation(
                "workspace insertion index is out of range".into(),
            ));
        }
        let source_index = self.state.windows[source]
            .workspace_ids
            .iter()
            .position(|id| *id == workspace)
            .expect("canonical source membership");
        self.state.windows[source]
            .workspace_ids
            .remove(source_index);
        self.state.windows[target]
            .workspace_ids
            .insert(index, workspace);
        if source != target && self.state.windows[source].selected_workspace == Some(workspace) {
            self.state.windows[source].selected_workspace = self.state.windows[source]
                .workspace_ids
                .get(source_index)
                .copied()
                .or_else(|| self.state.windows[source].workspace_ids.last().copied());
        }
        if focus || self.state.windows[target].selected_workspace.is_none() {
            self.state.windows[target].selected_workspace = Some(workspace);
        }
        if focus {
            self.state.focused_window = Some(destination);
        }
        self.project_window_focus();
        Ok(())
    }

    pub(crate) fn focus_window(&mut self, id: WindowId) -> Result<(), ActionError> {
        if !self.state.windows.iter().any(|window| window.id == id) {
            return Err(ActionError::InvalidOperation(
                "window does not exist".into(),
            ));
        }
        self.state.focused_window = Some(id);
        self.project_window_focus();
        Ok(())
    }

    pub(crate) fn create_window(
        &mut self,
        focus: bool,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        if self.state.windows.len() >= MAX_WINDOWS {
            return Err(ActionError::Capacity {
                resource: "windows",
                limit: MAX_WINDOWS,
            });
        }
        let prior = self.state.focused_window;
        let id = WindowId(self.reserve(1)?);
        self.state.windows.push(Window {
            id,
            workspace_ids: Vec::new(),
            selected_workspace: None,
        });
        self.state.focused_window = Some(id);
        self.create_workspace("Workspace".into(), None, true, effects)?;
        if !focus && prior.is_some() {
            self.state.focused_window = prior;
        }
        self.project_window_focus();
        Ok(())
    }

    pub(crate) fn close_window(
        &mut self,
        id: WindowId,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        let window = self
            .state
            .windows
            .iter()
            .find(|window| window.id == id)
            .ok_or_else(|| ActionError::InvalidOperation("window does not exist".into()))?
            .clone();
        if self.state.windows.len() == 1 {
            return Err(ActionError::InvalidOperation(
                "last window close requires application save and termination".into(),
            ));
        }
        for workspace_id in window.workspace_ids {
            self.surface_action(Action::CloseWorkspace { workspace_id }, effects)?;
        }
        self.state.windows.retain(|window| window.id != id);
        if self.state.focused_window == Some(id) {
            self.state.focused_window = self.state.windows.first().map(|window| window.id);
        }
        self.project_window_focus();
        Ok(())
    }

    fn ensure_window(&mut self) -> Result<(), ActionError> {
        if self.state.windows.is_empty() {
            let id = WindowId(self.reserve(1)?);
            self.state.windows.push(Window {
                id,
                workspace_ids: Vec::new(),
                selected_workspace: None,
            });
            self.state.focused_window = Some(id);
        }
        Ok(())
    }

    pub(crate) fn own_workspace(
        &mut self,
        id: WorkspaceId,
        focus: bool,
    ) -> Result<(), ActionError> {
        self.ensure_window()?;
        let window = self
            .state
            .windows
            .iter_mut()
            .find(|window| Some(window.id) == self.state.focused_window)
            .expect("focused window exists");
        window.workspace_ids.push(id);
        if focus || window.selected_workspace.is_none() {
            window.selected_workspace = Some(id);
        }
        self.project_window_focus();
        Ok(())
    }

    pub(crate) fn focus_workspace(&mut self, id: WorkspaceId) {
        let window = self
            .state
            .windows
            .iter_mut()
            .find(|window| window.workspace_ids.contains(&id))
            .expect("live workspace has an owning window");
        window.selected_workspace = Some(id);
        self.state.focused_window = Some(window.id);
        self.project_window_focus();
    }

    pub(crate) fn forget_workspace(&mut self, id: WorkspaceId) {
        let window = self
            .state
            .windows
            .iter_mut()
            .find(|window| window.workspace_ids.contains(&id))
            .expect("removed workspace has an owning window");
        let index = window
            .workspace_ids
            .iter()
            .position(|existing| *existing == id)
            .expect("owned workspace");
        window.workspace_ids.remove(index);
        if window.selected_workspace == Some(id) {
            window.selected_workspace = window
                .workspace_ids
                .get(index)
                .copied()
                .or_else(|| window.workspace_ids.last().copied());
        }
        self.project_window_focus();
    }

    pub(crate) fn project_window_focus(&mut self) {
        self.state.focused_workspace = self
            .state
            .windows
            .iter()
            .find(|window| Some(window.id) == self.state.focused_window)
            .and_then(|window| window.selected_workspace);
    }

    pub(crate) fn check_window_layout(&self) -> Result<(), ActionError> {
        let workspaces: Vec<_> = self
            .state
            .workspaces
            .iter()
            .map(|workspace| workspace.id)
            .collect();
        let projection =
            validate_layout(&workspaces, &self.state.windows, self.state.focused_window)
                .map_err(|message| ActionError::InvalidOperation(message.into()))?;
        if self.state.focused_workspace != projection {
            return Err(ActionError::InvalidOperation(
                "legacy focus differs from window selection".into(),
            ));
        }
        Ok(())
    }
}
