//! Nonempty, window-owned groups of existing workspaces. No process or UI work.
use crate::{
    ActionError, Window, WindowId, WorkspaceGroupId, WorkspaceId, WorkspaceStore, valid_name,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_WORKSPACE_GROUPS: usize = crate::MAX_WORKSPACES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGroup {
    pub id: WorkspaceGroupId,
    pub window_id: WindowId,
    pub name: String,
    /// Canonical contiguous order, including the anchor in the first position.
    pub workspace_ids: Vec<WorkspaceId>,
    pub collapsed: bool,
    pub external_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroupAction {
    Create {
        window_id: WindowId,
        name: String,
        workspace_ids: Vec<WorkspaceId>,
        #[serde(default)]
        external_id: Option<String>,
    },
    Rename {
        group_id: WorkspaceGroupId,
        name: String,
    },
    SetCollapsed {
        group_id: WorkspaceGroupId,
        collapsed: bool,
    },
    Add {
        group_id: WorkspaceGroupId,
        workspace_id: WorkspaceId,
    },
    Remove {
        workspace_id: WorkspaceId,
    },
    SetAnchor {
        group_id: WorkspaceGroupId,
        workspace_id: WorkspaceId,
    },
    Move {
        group_id: WorkspaceGroupId,
        index: i64,
    },
    Ungroup {
        group_id: WorkspaceGroupId,
    },
}
fn invalid(message: &str) -> ActionError {
    ActionError::InvalidOperation(message.into())
}
fn valid_external(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= crate::MAX_NAME_CHARS
        && !value.chars().any(char::is_control)
}
pub(crate) fn validate(groups: &[WorkspaceGroup], windows: &[Window]) -> Result<(), ActionError> {
    if groups.len() > MAX_WORKSPACE_GROUPS {
        return Err(invalid("workspace group limit exceeded"));
    }
    let mut identities = HashSet::new();
    let mut memberships = HashSet::new();
    let mut external = HashSet::new();
    for group in groups {
        if group.id.0 == 0 || !identities.insert(group.id) {
            return Err(invalid("workspace group IDs must be nonzero and unique"));
        }
        valid_name(group.name.clone())?;
        if let Some(key) = &group.external_id
            && (!valid_external(key) || !external.insert((group.window_id, key)))
        {
            return Err(invalid(
                "workspace group external identity must be bounded and unique within its window",
            ));
        }
        let window = windows
            .iter()
            .find(|w| w.id == group.window_id)
            .ok_or_else(|| invalid("workspace group window does not exist"))?;
        let anchor = group
            .workspace_ids
            .first()
            .ok_or_else(|| invalid("workspace groups must have an explicit existing anchor"))?;
        let start = window
            .workspace_ids
            .iter()
            .position(|id| id == anchor)
            .ok_or_else(|| invalid("workspace group anchor does not belong to its window"))?;
        if window
            .workspace_ids
            .get(start..start + group.workspace_ids.len())
            != Some(group.workspace_ids.as_slice())
        {
            return Err(invalid(
                "workspace group members must form one contiguous ordered block",
            ));
        }
        if group
            .workspace_ids
            .iter()
            .any(|id| !memberships.insert(*id))
        {
            return Err(invalid("workspace group membership must be unique"));
        }
    }
    Ok(())
}
/// Shared close/move reconciliation. An explicit anchor change is the only
/// operation that can move a surviving anchor behind another group member.
pub(crate) fn reconcile(
    groups: &mut Vec<WorkspaceGroup>,
    windows: &[Window],
) -> Result<(), ActionError> {
    for group in groups.iter_mut() {
        if let Some(window) = windows.iter().find(|w| w.id == group.window_id) {
            group
                .workspace_ids
                .retain(|id| window.workspace_ids.contains(id));
            let anchor = group.workspace_ids.first().copied();
            group
                .workspace_ids
                .sort_by_key(|id| window.workspace_ids.iter().position(|w| w == id));
            if group.workspace_ids.first().copied() != anchor {
                return Err(invalid(
                    "workspace reordering cannot replace a surviving group anchor",
                ));
            }
        } else {
            group.workspace_ids.clear();
        }
    }
    groups.retain(|g| !g.workspace_ids.is_empty());
    validate(groups, windows)
}
impl crate::Snapshot {
    pub(crate) fn check_group_order(
        &self,
        window: WindowId,
        ordered: &[WorkspaceId],
    ) -> Result<(), ActionError> {
        let mut windows = self.windows.clone();
        windows
            .iter_mut()
            .find(|w| w.id == window)
            .ok_or_else(|| invalid("workspace order window does not exist"))?
            .workspace_ids = ordered.to_vec();
        reconcile(&mut self.workspace_groups.clone(), &windows)
    }
}
impl WorkspaceStore {
    fn group_index(&self, id: WorkspaceGroupId) -> Result<usize, ActionError> {
        self.state
            .workspace_groups
            .iter()
            .position(|g| g.id == id)
            .ok_or_else(|| invalid("workspace group does not exist"))
    }
    fn detach_group_member(&mut self, id: WorkspaceId) {
        for group in &mut self.state.workspace_groups {
            group.workspace_ids.retain(|member| *member != id);
        }
        self.state
            .workspace_groups
            .retain(|g| !g.workspace_ids.is_empty());
    }
    pub(crate) fn group_action(&mut self, command: GroupAction) -> Result<(), ActionError> {
        match command {
            GroupAction::Create {
                window_id,
                name,
                workspace_ids,
                external_id,
            } => {
                let name = valid_name(name)?;
                let window = self
                    .state
                    .windows
                    .iter()
                    .position(|w| w.id == window_id)
                    .ok_or_else(|| invalid("workspace group window does not exist"))?;
                if let Some(key) = &external_id {
                    if !valid_external(key) {
                        return Err(invalid(
                            "workspace group external identity must contain 1–128 characters without controls",
                        ));
                    }
                    if self
                        .state
                        .workspace_groups
                        .iter()
                        .any(|g| g.window_id == window_id && g.external_id.as_ref() == Some(key))
                    {
                        return Ok(());
                    }
                }
                if self.state.workspace_groups.len() >= MAX_WORKSPACE_GROUPS {
                    return Err(invalid("workspace group limit exceeded"));
                }
                let anchor = workspace_ids.first().ok_or_else(|| {
                    invalid("workspace group create requires explicit existing workspaces")
                })?;
                let mut seen = HashSet::new();
                for id in &workspace_ids {
                    if !seen.insert(*id) {
                        return Err(invalid(
                            "workspace group create contains duplicate workspaces",
                        ));
                    }
                    if !self.state.windows[window].workspace_ids.contains(id) {
                        return Err(invalid(
                            "workspace group member does not belong to its window",
                        ));
                    }
                    if self
                        .state
                        .workspace_groups
                        .iter()
                        .any(|g| g.workspace_ids.contains(id))
                    {
                        return Err(invalid(
                            "workspace already belongs to a group; remove it before creating another group",
                        ));
                    }
                }
                let original = &self.state.windows[window].workspace_ids;
                let index = original
                    .iter()
                    .take_while(|id| *id != anchor)
                    .filter(|id| !seen.contains(id))
                    .count();
                let mut ordered: Vec<_> = original
                    .iter()
                    .copied()
                    .filter(|id| !seen.contains(id))
                    .collect();
                ordered.splice(index..index, workspace_ids.iter().copied());
                let id = WorkspaceGroupId(self.reserve(1)?);
                self.state.windows[window].workspace_ids = ordered;
                self.state.workspace_groups.push(WorkspaceGroup {
                    id,
                    window_id,
                    name,
                    workspace_ids,
                    collapsed: false,
                    external_id,
                });
            }
            GroupAction::Rename { group_id, name } => {
                let name = valid_name(name)?;
                let i = self.group_index(group_id)?;
                self.state.workspace_groups[i].name = name;
            }
            GroupAction::SetCollapsed {
                group_id,
                collapsed,
            } => {
                let i = self.group_index(group_id)?;
                self.state.workspace_groups[i].collapsed = collapsed;
            }
            GroupAction::Ungroup { group_id } => {
                let i = self.group_index(group_id)?;
                self.state.workspace_groups.remove(i);
            }
            GroupAction::Remove { workspace_id } => {
                if !self.state.workspaces.iter().any(|w| w.id == workspace_id) {
                    return Err(ActionError::UnknownWorkspace(workspace_id));
                }
                if let Some(group) = self
                    .state
                    .workspace_groups
                    .iter()
                    .find(|g| g.workspace_ids.contains(&workspace_id))
                    .cloned()
                {
                    self.detach_group_member(workspace_id);
                    if let Some(tail) = group
                        .workspace_ids
                        .iter()
                        .rev()
                        .find(|id| **id != workspace_id)
                    {
                        let window = self
                            .state
                            .windows
                            .iter_mut()
                            .find(|w| w.id == group.window_id)
                            .expect("validated group window");
                        window.workspace_ids.retain(|id| *id != workspace_id);
                        let end = window
                            .workspace_ids
                            .iter()
                            .position(|id| id == tail)
                            .expect("surviving group member")
                            + 1;
                        window.workspace_ids.insert(end, workspace_id);
                    }
                }
            }
            GroupAction::Add {
                group_id,
                workspace_id,
            } => {
                let i = self.group_index(group_id)?;
                let group = self.state.workspace_groups[i].clone();
                let window = self
                    .state
                    .windows
                    .iter()
                    .position(|w| w.id == group.window_id)
                    .expect("validated group window");
                if !self.state.windows[window]
                    .workspace_ids
                    .contains(&workspace_id)
                {
                    return Err(invalid("new group member must belong to the same window"));
                }
                if group.workspace_ids.contains(&workspace_id) {
                    return Ok(());
                }
                self.detach_group_member(workspace_id);
                let window = &mut self.state.windows[window];
                window.workspace_ids.retain(|id| *id != workspace_id);
                let tail = group.workspace_ids.last().expect("nonempty group");
                let end = window
                    .workspace_ids
                    .iter()
                    .position(|id| id == tail)
                    .expect("group member is live")
                    + 1;
                window.workspace_ids.insert(end, workspace_id);
                let i = self.group_index(group_id)?;
                self.state.workspace_groups[i]
                    .workspace_ids
                    .push(workspace_id);
            }
            GroupAction::SetAnchor {
                group_id,
                workspace_id,
            } => {
                let i = self.group_index(group_id)?;
                let group = self.state.workspace_groups[i].clone();
                if !group.workspace_ids.contains(&workspace_id) {
                    return Err(invalid("group anchor must already be a member"));
                }
                let mut members = group.workspace_ids.clone();
                members.retain(|id| *id != workspace_id);
                members.insert(0, workspace_id);
                let window = self
                    .state
                    .windows
                    .iter_mut()
                    .find(|w| w.id == group.window_id)
                    .expect("validated group window");
                let start = window
                    .workspace_ids
                    .iter()
                    .position(|id| Some(id) == group.workspace_ids.first())
                    .expect("live group anchor");
                window
                    .workspace_ids
                    .splice(start..start + members.len(), members.iter().copied());
                self.state.workspace_groups[i].workspace_ids = members;
            }
            GroupAction::Move { group_id, index } => {
                let i = self.group_index(group_id)?;
                let group = self.state.workspace_groups[i].clone();
                let window = self
                    .state
                    .windows
                    .iter_mut()
                    .find(|w| w.id == group.window_id)
                    .expect("validated group window");
                window
                    .workspace_ids
                    .retain(|id| !group.workspace_ids.contains(id));
                let index = index.clamp(0, window.workspace_ids.len() as i64) as usize;
                window
                    .workspace_ids
                    .splice(index..index, group.workspace_ids.iter().copied());
            }
        }
        Ok(())
    }
}
