//! Canonical workspace checklist values. No I/O or random identity allocation.
use crate::{ActionError, ClosedContent, WorkspaceId, WorkspaceStore};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const MAX_TODO_ENTRIES: usize = 50;
pub const MAX_TODO_TEXT_BYTES: usize = 2048;
pub const MAX_RETAINED_TODO_BYTES: usize = 512 * 1024;
pub const MAX_UNDO_TODO_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TodoState {
    #[default]
    Pending,
    InProgress,
    Completed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoOrigin {
    #[default]
    User,
    Agent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoEntry {
    pub id: Uuid,
    pub text: String,
    pub state: TodoState,
    pub origin: TodoOrigin,
}
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TodoError(pub &'static str);
pub fn validate_text(text: &str) -> Result<(), TodoError> {
    if text.trim().is_empty()
        || text.len() > MAX_TODO_TEXT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(TodoError(
            "todo text must contain 1 to 2048 bytes of nonwhitespace content without controls except newline/tab",
        ));
    }
    Ok(())
}
pub fn encoded_bound(items: &[TodoEntry]) -> usize {
    items.iter().map(|item| 192 + 2 * item.text.len()).sum()
}
pub fn validate_lists<'a>(
    lists: impl IntoIterator<Item = &'a [TodoEntry]>,
) -> Result<(), TodoError> {
    let mut ids = HashSet::new();
    let mut bytes = 0usize;
    for items in lists {
        if items.len() > MAX_TODO_ENTRIES {
            return Err(TodoError("workspace todo capacity reached"));
        }
        for item in items {
            if item.id.is_nil() || !ids.insert(item.id) {
                return Err(TodoError("todo IDs must be nonzero and unique"));
            }
            validate_text(&item.text)?;
        }
        bytes += encoded_bound(items);
        if bytes > MAX_RETAINED_TODO_BYTES {
            return Err(TodoError("aggregate todo byte capacity reached"));
        }
    }
    Ok(())
}
impl WorkspaceStore {
    /// The serialized runtime owner plans combined metadata capacity before this
    /// pure transaction. All direct callers still receive full model validation.
    pub fn replace_workspace_todos(
        &mut self,
        workspace: WorkspaceId,
        items: Vec<TodoEntry>,
    ) -> Result<(), ActionError> {
        let mut candidate = self.clone();
        let index = candidate
            .state
            .workspaces
            .iter()
            .position(|w| w.id == workspace)
            .ok_or(ActionError::UnknownWorkspace(workspace))?;
        let surfaces: Vec<_> = candidate.state.workspaces[index]
            .root
            .panes()
            .iter()
            .flat_map(|pane| pane.surfaces.iter().map(|surface| surface.id))
            .collect();
        candidate.ensure_settled(&surfaces)?;
        candidate.state.workspaces[index].todos = items;
        candidate.validate_todos()?;
        // Todo data has its own metadata publication, not topology identity.
        // Keep selector/topology revisions stable so native rows are not rebuilt.
        *self = candidate;
        Ok(())
    }
    pub(crate) fn validate_todos(&self) -> Result<(), ActionError> {
        validate_lists(
            self.state
                .workspaces
                .iter()
                .map(|workspace| workspace.todos.as_slice()),
        )
        .map_err(|error| ActionError::InvalidOperation(error.to_string()))?;
        for item in &self.state.closed_items {
            validate_lists([item.saved_todos()])
                .map_err(|error| ActionError::InvalidOperation(error.to_string()))?;
        }
        if self
            .state
            .closed_items
            .iter()
            .map(|item| encoded_bound(item.saved_todos()))
            .sum::<usize>()
            > MAX_UNDO_TODO_BYTES
        {
            return Err(ActionError::InvalidOperation(
                "undo todo byte capacity reached".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn trim_todo_history(&mut self) -> Result<(), ActionError> {
        while self
            .state
            .closed_items
            .iter()
            .map(|item| encoded_bound(item.saved_todos()))
            .sum::<usize>()
            > MAX_UNDO_TODO_BYTES
        {
            let newest = self.state.closed_items.last().map(|item| item.id);
            let index = self
                .state
                .closed_items
                .iter()
                .position(|item| !item.restoring && Some(item.id) != newest)
                .ok_or_else(|| {
                    ActionError::InvalidOperation(
                        "todo recovery history is busy restoring; retry".into(),
                    )
                })?;
            self.state.closed_items.remove(index);
        }
        self.prune_restore_anchors();
        Ok(())
    }
}
impl crate::ClosedItem {
    pub(crate) fn saved_todos(&self) -> &[TodoEntry] {
        match &self.content {
            ClosedContent::Workspace { workspace } => &workspace.todos,
            _ => &self.workspace_todos,
        }
    }
}
