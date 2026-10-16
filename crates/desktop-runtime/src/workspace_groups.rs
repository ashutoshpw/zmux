//! Group selectors and mutations resolve together on one workspace executor revision.
use crate::{DesktopError, Worker};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zmux_workspace_core::{
    Action, Snapshot, WindowId, WorkspaceGroupId,
    targets::{CallerContext, TargetKind, TargetRef, TargetScope},
    workspace_groups::GroupAction,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupRequest {
    #[serde(default)]
    pub window: Option<TargetRef>,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    pub action: GroupCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroupCommand {
    List,
    Create {
        name: String,
        workspaces: Vec<TargetRef>,
        #[serde(default)]
        external_id: Option<String>,
    },
    Rename {
        group: TargetRef,
        name: String,
    },
    SetCollapsed {
        group: TargetRef,
        collapsed: bool,
    },
    Add {
        group: TargetRef,
        workspace: TargetRef,
    },
    Remove {
        workspace: TargetRef,
    },
    SetAnchor {
        group: TargetRef,
        workspace: TargetRef,
    },
    Move {
        group: TargetRef,
        index: i64,
    },
    Ungroup {
        group: TargetRef,
    },
}
impl GroupCommand {
    pub fn is_read(&self) -> bool {
        matches!(self, Self::List)
    }
}
fn context(model: &Snapshot, request: &GroupRequest) -> Result<WindowId, DesktopError> {
    if let Some(window) = &request.window {
        return Ok(model.resolve_window(window)?);
    }
    let caller = request.caller.bind_local_tmux(model)?;
    // A stale window-only claim is an error; it may not borrow focused state.
    if caller.workspace.is_none()
        && caller.surface.is_none()
        && let Some(window) = &caller.window
    {
        return Ok(model.resolve_window(window)?);
    }
    Ok(TargetScope::default().resolve_window(model, &caller)?)
}
fn row(model: &Snapshot, id: WorkspaceGroupId) -> Value {
    let group = model
        .workspace_groups
        .iter()
        .find(|g| g.id == id)
        .expect("live result group");
    let groups = model.workspace_groups_in_window(group.window_id);
    json!({"group_id":group.id,"group_uuid":model.object_uuid(TargetKind::WorkspaceGroup,group.id.0),
        "reference":format!("workspace_group:{}",group.id.0),"index":groups.iter().position(|g|g.id==id).expect("group window membership")+1,
        "window_id":group.window_id,"window_uuid":model.object_uuid(TargetKind::Window,group.window_id.0),
        "name":group.name,"collapsed":group.collapsed,"external_id":group.external_id,
        "anchor_workspace_id":group.workspace_ids[0],"workspace_ids":group.workspace_ids,
        "workspace_uuids":group.workspace_ids.iter().map(|id|model.object_uuid(TargetKind::Workspace,id.0)).collect::<Vec<_>>()})
}
fn result(model: &Snapshot, window: WindowId) -> Value {
    let owner = model
        .windows
        .iter()
        .find(|w| w.id == window)
        .expect("resolved result window");
    json!({"window_id":window,"window_uuid":model.object_uuid(TargetKind::Window,window.0),"model_revision":model.revision,
        "workspace_ids":owner.workspace_ids,"groups":model.workspace_groups_in_window(window).iter().map(|g|row(model,g.id)).collect::<Vec<_>>()})
}
impl Worker {
    pub(crate) fn workspace_group(&mut self, request: GroupRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        if let Some(expected) = request.expected_model_revision
            && expected != model.revision
        {
            return Err(zmux_workspace_core::targets::TargetError::StaleRevision {
                expected,
                actual: model.revision,
            }
            .into());
        }
        let resolve_group = |handle: &TargetRef| -> Result<WorkspaceGroupId, DesktopError> {
            let constraint = if request.window.is_some() || handle.is_index()? {
                Some(context(&model, &request)?)
            } else {
                None
            };
            Ok(model.resolve_workspace_group(handle, constraint)?)
        };
        let explicit = request
            .window
            .as_ref()
            .map(|w| model.resolve_window(w))
            .transpose()?;
        let group_id;
        let window;
        let command;
        let mut created = false;
        let mut dissolved = 0;
        match &request.action {
            GroupCommand::List => {
                let window = context(&model, &request)?;
                self.workspace_group_result = Some(result(&model, window));
                return Ok(());
            }
            GroupCommand::Create {
                name,
                workspaces,
                external_id,
            } => {
                window = context(&model, &request)?;
                if workspaces.is_empty() {
                    return Err(DesktopError::Failure(
                        "group create requires explicit existing workspaces".into(),
                    ));
                }
                if let Some(existing) = model.workspace_groups.iter().find(|g| {
                    g.window_id == window && external_id.is_some() && g.external_id == *external_id
                }) {
                    // Existing identity wins before resolving stale initial member
                    // handles; never allocate or replay the original create.
                    let mut value = result(&model, window);
                    value["group"] = row(&model, existing.id);
                    value["created"] = json!(false);
                    value["changed"] = json!(false);
                    self.workspace_group_result = Some(value);
                    return Ok(());
                }
                let members = workspaces
                    .iter()
                    .map(|handle| {
                        model
                            .resolve_order_workspace(handle, Some(window))
                            .map(|(_, id)| id)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                group_id = Some(WorkspaceGroupId(self.model.next_identifier()));
                created = true;
                command = GroupAction::Create {
                    window_id: window,
                    name: name.clone(),
                    workspace_ids: members,
                    external_id: external_id.clone(),
                };
            }
            GroupCommand::Remove { workspace } => {
                let constraint = if explicit.is_some() || workspace.is_index()? {
                    Some(context(&model, &request)?)
                } else {
                    None
                };
                let (owner, id) = model.resolve_order_workspace(workspace, constraint)?;
                window = owner;
                group_id = model
                    .workspace_groups
                    .iter()
                    .find(|g| g.workspace_ids.contains(&id))
                    .map(|g| g.id);
                command = GroupAction::Remove { workspace_id: id };
            }
            other => {
                let handle = match other {
                    GroupCommand::Rename { group, .. }
                    | GroupCommand::SetCollapsed { group, .. }
                    | GroupCommand::Add { group, .. }
                    | GroupCommand::SetAnchor { group, .. }
                    | GroupCommand::Move { group, .. }
                    | GroupCommand::Ungroup { group } => group,
                    _ => unreachable!(),
                };
                let id = resolve_group(handle)?;
                group_id = Some(id);
                let group = model
                    .workspace_groups
                    .iter()
                    .find(|g| g.id == id)
                    .expect("resolved group");
                window = group.window_id;
                command = match other {
                    GroupCommand::Rename { name, .. } => GroupAction::Rename {
                        group_id: id,
                        name: name.clone(),
                    },
                    GroupCommand::SetCollapsed { collapsed, .. } => GroupAction::SetCollapsed {
                        group_id: id,
                        collapsed: *collapsed,
                    },
                    GroupCommand::Add { workspace, .. } => GroupAction::Add {
                        group_id: id,
                        workspace_id: model.resolve_order_workspace(workspace, Some(window))?.1,
                    },
                    GroupCommand::SetAnchor { workspace, .. } => GroupAction::SetAnchor {
                        group_id: id,
                        workspace_id: model.resolve_order_workspace(workspace, Some(window))?.1,
                    },
                    GroupCommand::Move { index, .. } => GroupAction::Move {
                        group_id: id,
                        index: *index,
                    },
                    GroupCommand::Ungroup { .. } => {
                        dissolved = group.workspace_ids.len();
                        GroupAction::Ungroup { group_id: id }
                    }
                    _ => unreachable!(),
                };
            }
        }
        self.action(Action::WorkspaceGroup { command })?;
        let after = self.model.snapshot();
        let mut value = result(&after, window);
        value["group"] = group_id
            .filter(|id| after.workspace_groups.iter().any(|g| g.id == *id))
            .map(|id| row(&after, id))
            .unwrap_or(Value::Null);
        value["created"] = json!(created);
        value["changed"] = json!(
            model.workspace_groups != after.workspace_groups || model.windows != after.windows
        );
        if dissolved > 0 {
            value["outcome"] = json!("dissolved");
            value["affected_workspaces"] = json!(dissolved);
            value["workspaces_closed"] = json!(0);
        }
        self.workspace_group_result = Some(value);
        Ok(())
    }
}
