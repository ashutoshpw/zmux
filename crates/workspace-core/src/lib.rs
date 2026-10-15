//! Shared workspace actions. This crate performs no PTY, UI, filesystem, or network I/O.

use serde::{Deserialize, Serialize};
use thiserror::Error;
mod navigation;
mod restore;
mod surfaces;
mod windows;
pub use restore::{
    MAX_NEXT_IDENTIFIER, PreparedSessionRestore, RestoreError, RestoreLimits,
    SESSION_DOCUMENT_VERSION, SavedContent, SavedPane, SavedSshConnection, SavedSurface, SavedTree,
    SavedWindowLayout, SavedWorkspace, SessionDocument, ValidatedSessionDocument,
};
pub use windows::{MAX_WINDOWS, Window};
pub mod targets;
pub use navigation::{Direction, FocusHistoryDirection, MAX_FOCUS_HISTORY, Navigation};
use surfaces::PendingRestore;
pub use surfaces::{ClosedContent, ClosedItem, Surface, SurfaceKind};

pub const MAX_WORKSPACES: usize = 32;
pub const MAX_PANES_PER_WORKSPACE: usize = 64;
pub const MAX_SPLIT_DEPTH: usize = 16;
pub const MAX_NAME_CHARS: usize = 128;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}
identifier!(WorkspaceId);
identifier!(PaneId);
identifier!(SessionId);
identifier!(SurfaceId);
identifier!(WindowId);

pub const MODEL_VERSION: u32 = 6;
pub const MAX_SURFACES_PER_PANE: usize = 32;
pub const MAX_CLOSED_ITEMS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Pane {
    pub id: PaneId,
    pub selected_surface: SurfaceId,
    pub surfaces: Vec<Surface>,
}

impl Pane {
    pub fn selected(&self) -> &Surface {
        self.surfaces
            .iter()
            .find(|s| s.id == self.selected_surface)
            .expect("selected surface exists")
    }
    pub fn session_id(&self) -> Option<SessionId> {
        self.selected().session_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneTree {
    Leaf {
        pane: Pane,
    },
    Split {
        axis: Axis,
        first: Box<PaneTree>,
        second: Box<PaneTree>,
    },
}

impl PaneTree {
    pub fn panes(&self) -> Vec<&Pane> {
        let mut panes = Vec::new();
        self.collect(&mut panes);
        panes
    }

    fn collect<'a>(&'a self, panes: &mut Vec<&'a Pane>) {
        match self {
            Self::Leaf { pane } => panes.push(pane),
            Self::Split { first, second, .. } => {
                first.collect(panes);
                second.collect(panes);
            }
        }
    }

    fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            Self::Leaf { pane } => (pane.id == id).then_some(pane),
            Self::Split { first, second, .. } => first.pane_mut(id).or_else(|| second.pane_mut(id)),
        }
    }

    fn depth_of(&self, id: PaneId, depth: usize) -> Option<usize> {
        match self {
            Self::Leaf { pane } => (pane.id == id).then_some(depth),
            Self::Split { first, second, .. } => first
                .depth_of(id, depth + 1)
                .or_else(|| second.depth_of(id, depth + 1)),
        }
    }

    fn split(&mut self, id: PaneId, axis: Axis, new_pane: Pane, before: bool) {
        match self {
            Self::Leaf { pane } if pane.id == id => {
                let original = Box::new(Self::Leaf { pane: pane.clone() });
                let added = Box::new(Self::Leaf { pane: new_pane });
                let (first, second) = if before {
                    (added, original)
                } else {
                    (original, added)
                };
                *self = Self::Split {
                    axis,
                    first,
                    second,
                };
            }
            Self::Split { first, second, .. } => {
                if first.depth_of(id, 0).is_some() {
                    first.split(id, axis, new_pane, before);
                } else {
                    second.split(id, axis, new_pane, before);
                }
            }
            Self::Leaf { .. } => unreachable!("pane validated before mutation"),
        }
    }

    fn without(self, id: PaneId) -> Option<Self> {
        match self {
            Self::Leaf { ref pane } if pane.id == id => None,
            Self::Leaf { .. } => Some(self),
            Self::Split {
                axis,
                first,
                second,
            } => match (first.without(id), second.without(id)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(tree), None) | (None, Some(tree)) => Some(tree),
                (None, None) => None,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub focused_pane: PaneId,
    pub root: PaneTree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// Persist with the model: object UUIDs derive from this namespace and never from display order.
    pub identity_namespace: uuid::Uuid,
    pub model_version: u32,
    pub revision: u64,
    pub closed_items: Vec<ClosedItem>,
    pub windows: Vec<Window>,
    pub focused_window: Option<WindowId>,
    /// Compatibility projection of the focused window's selected workspace.
    pub focused_workspace: Option<WorkspaceId>,
    pub workspaces: Vec<Workspace>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            identity_namespace: uuid::Uuid::nil(),
            model_version: MODEL_VERSION,
            revision: 0,
            closed_items: Vec::new(),
            windows: Vec::new(),
            focused_window: None,
            focused_workspace: None,
            workspaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    CreateWindow {
        #[serde(default = "default_focus")]
        focus: bool,
    },
    FocusWindow {
        window_id: WindowId,
    },
    CloseWindow {
        window_id: WindowId,
    },
    MoveWorkspaceToWindow {
        workspace_id: WorkspaceId,
        destination_window_id: WindowId,
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        focus: bool,
    },
    CreateWorkspaceInWindow {
        window_id: WindowId,
        name: String,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    NavigateFocusInWindow {
        window_id: WindowId,
        direction: FocusHistoryDirection,
    },
    ReopenClosedInWindow {
        window_id: WindowId,
        #[serde(default)]
        closed_id: Option<u64>,
    },
    CreateLocalTmuxSurface {
        pane_id: PaneId,
        identity: zmux_remote_runtime::local_multiplexer::LocalTmuxIdentity,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    CreateBrowser {
        #[serde(default)]
        profile_id: Option<uuid::Uuid>,
        pane_id: PaneId,
        url: String,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    NavigateBrowser {
        surface_id: SurfaceId,
        url: String,
    },
    ObserveBrowser {
        surface_id: SurfaceId,
        url: String,
        title: String,
    },
    CreateSurface {
        pane_id: PaneId,
    },
    SelectSurface {
        surface_id: SurfaceId,
    },
    CloseSurface {
        surface_id: SurfaceId,
    },
    RenameSurface {
        surface_id: SurfaceId,
        name: String,
    },
    ReorderSurface {
        surface_id: SurfaceId,
        index: usize,
    },
    MoveSurface {
        surface_id: SurfaceId,
        pane_id: PaneId,
        index: usize,
    },
    CloseOtherSurfaces {
        surface_id: SurfaceId,
    },
    CloseWorkspace {
        workspace_id: WorkspaceId,
    },
    ReopenClosed {
        #[serde(default)]
        closed_id: Option<u64>,
    },
    CreateWorkspace {
        name: String,
    },
    CreateSshWorkspace {
        name: String,
        connection: zmux_remote_runtime::SshConnectionConfig,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    RenameWorkspace {
        workspace_id: WorkspaceId,
        name: String,
    },
    SelectWorkspace {
        workspace_id: WorkspaceId,
    },
    NavigateWorkspace {
        workspace_id: WorkspaceId,
        direction: Navigation,
    },
    NavigateSurface {
        surface_id: SurfaceId,
        direction: Navigation,
    },
    NavigatePane {
        pane_id: PaneId,
        direction: Navigation,
    },
    FocusPaneDirection {
        pane_id: PaneId,
        direction: Direction,
    },
    NavigateFocus {
        direction: FocusHistoryDirection,
    },
    SplitPaneDirectional {
        pane_id: PaneId,
        direction: Direction,
        #[serde(default)]
        focus: bool,
    },
    FocusPane {
        pane_id: PaneId,
    },
    SplitPane {
        pane_id: PaneId,
        axis: Axis,
    },
    ClosePane {
        pane_id: PaneId,
    },
}

/// Effects are applied by the session runtime, never by the UI model itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    StartSession {
        pane_id: PaneId,
        session_id: SessionId,
        cwd: Option<String>,
        remote: Option<zmux_remote_runtime::SshConnectionConfig>,
        local_tmux: Option<Box<zmux_remote_runtime::local_multiplexer::LocalTmuxIdentity>>,
    },
    StopSession {
        session_id: SessionId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Transition {
    pub revision: u64,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("name must contain 1–{MAX_NAME_CHARS} characters and no control characters")]
    InvalidName,
    #[error("workspace {0:?} does not exist")]
    UnknownWorkspace(WorkspaceId),
    #[error("pane {0:?} does not exist")]
    UnknownPane(PaneId),
    #[error("surface {0:?} does not exist")]
    UnknownSurface(SurfaceId),
    #[error("{0}")]
    InvalidOperation(String),
    #[error("{resource} limit reached ({limit})")]
    Capacity {
        resource: &'static str,
        limit: usize,
    },
    #[error("workspace identifier or revision space exhausted")]
    Exhausted,
}

#[derive(Clone, Debug)]
pub struct WorkspaceStore {
    state: Snapshot,
    next_id: u64,
    pending_restores: Vec<PendingRestore>,
    focus_history: std::collections::HashMap<WindowId, navigation::FocusHistory>,
    restore_placements: std::collections::HashMap<u64, Vec<surfaces::SplitAnchor>>,
    restored_panes: std::collections::HashMap<PaneId, PaneId>,
    restored_workspaces: std::collections::HashMap<WorkspaceId, WorkspaceId>,
}

impl Default for WorkspaceStore {
    fn default() -> Self {
        Self {
            state: Snapshot::default(),
            next_id: 1,
            pending_restores: Vec::new(),
            focus_history: Default::default(),
            restore_placements: Default::default(),
            restored_panes: Default::default(),
            restored_workspaces: Default::default(),
        }
    }
}

impl WorkspaceStore {
    /// The application injects an OS-random namespace once; the core performs no I/O.
    pub fn with_identity_namespace(namespace: uuid::Uuid) -> Self {
        Self {
            state: Snapshot {
                identity_namespace: namespace,
                ..Snapshot::default()
            },
            ..Self::default()
        }
    }

    /// This snapshot is for infrequent workspace changes, not terminal output frames.
    pub fn snapshot(&self) -> Snapshot {
        self.state.clone()
    }

    /// Rejected actions do not change state, identifiers, or the revision.
    pub fn apply(&mut self, action: Action) -> Result<Transition, ActionError> {
        let mut candidate = self.clone();
        let prior = candidate.focused();
        let prior_window = candidate.state.focused_window;
        let is_history_navigation = matches!(
            action,
            Action::NavigateFocus { .. } | Action::NavigateFocusInWindow { .. }
        );
        let record_current = !matches!(action, Action::MoveWorkspaceToWindow { focus: false, .. });
        let result = candidate.apply_inner(action)?;
        candidate.check_window_layout()?;
        candidate.record_focus(prior, prior_window, is_history_navigation, record_current);
        *self = candidate;
        Ok(result)
    }

    fn apply_inner(&mut self, action: Action) -> Result<Transition, ActionError> {
        let revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(ActionError::Exhausted)?;
        let mut effects = Vec::new();
        match action {
            Action::CreateWindow { focus } => self.create_window(focus, &mut effects)?,
            Action::FocusWindow { window_id } => self.focus_window(window_id)?,
            Action::CloseWindow { window_id } => self.close_window(window_id, &mut effects)?,
            Action::MoveWorkspaceToWindow {
                workspace_id,
                destination_window_id,
                index,
                focus,
            } => {
                self.move_workspace_to_window(workspace_id, destination_window_id, index, focus)?;
            }
            Action::CreateWorkspaceInWindow {
                window_id,
                name,
                focus,
            } => {
                let prior = self.state.focused_window;
                self.focus_window(window_id)?;
                self.create_workspace(name, None, focus, &mut effects)?;
                if !focus {
                    self.state.focused_window = prior;
                    self.project_window_focus();
                }
            }
            Action::NavigateFocusInWindow {
                window_id,
                direction,
            } => {
                self.focus_window(window_id)?;
                self.navigate_focus(direction)?;
            }
            Action::ReopenClosedInWindow {
                window_id,
                closed_id,
            } => {
                self.focus_window(window_id)?;
                self.surface_action(Action::ReopenClosed { closed_id }, &mut effects)?;
            }
            Action::CreateWorkspace { name } => {
                self.create_workspace(name, None, true, &mut effects)?
            }
            Action::CreateSshWorkspace {
                name,
                connection,
                focus,
            } => {
                connection.validate().map_err(|e| {
                    ActionError::InvalidOperation(format!("invalid SSH connection: {e}"))
                })?;
                self.create_workspace(name, Some(connection), focus, &mut effects)?;
            }
            Action::RenameWorkspace { workspace_id, name } => {
                let name = valid_name(name)?;
                let workspace = self
                    .state
                    .workspaces
                    .iter_mut()
                    .find(|w| w.id == workspace_id)
                    .ok_or(ActionError::UnknownWorkspace(workspace_id))?;
                workspace.name = name;
            }
            Action::FocusPane { pane_id } => {
                let index = self.workspace_index(pane_id)?;
                self.focus(index, pane_id);
            }
            Action::SplitPane { pane_id, axis } => {
                self.split_pane(pane_id, axis, false, true, &mut effects)?;
            }
            Action::SplitPaneDirectional {
                pane_id,
                direction,
                focus,
            } => {
                self.split_pane(
                    pane_id,
                    direction.axis(),
                    direction.before(),
                    focus,
                    &mut effects,
                )?;
            }
            Action::SelectWorkspace { workspace_id } => self.select_workspace(workspace_id)?,
            Action::NavigateWorkspace {
                workspace_id,
                direction,
            } => self.navigate_workspace(workspace_id, direction)?,
            Action::NavigateSurface {
                surface_id,
                direction,
            } => self.navigate_surface(surface_id, direction)?,
            Action::NavigatePane { pane_id, direction } => {
                self.navigate_pane(pane_id, direction)?
            }
            Action::FocusPaneDirection { pane_id, direction } => {
                self.focus_pane_direction(pane_id, direction)?
            }
            Action::NavigateFocus { direction } => self.navigate_focus(direction)?,
            Action::ClosePane { pane_id } => self.close_pane(pane_id, &mut effects)?,
            action => self.surface_action(action, &mut effects)?,
        }
        self.state.revision = revision;
        Ok(Transition { revision, effects })
    }

    fn split_pane(
        &mut self,
        pane_id: PaneId,
        axis: Axis,
        before: bool,
        focus: bool,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        let index = self.workspace_index(pane_id)?;
        let workspace = &self.state.workspaces[index];
        if workspace.root.panes().len() == MAX_PANES_PER_WORKSPACE {
            return Err(ActionError::Capacity {
                resource: "panes",
                limit: MAX_PANES_PER_WORKSPACE,
            });
        }
        if workspace.root.depth_of(pane_id, 0).expect("validated pane") >= MAX_SPLIT_DEPTH {
            return Err(ActionError::Capacity {
                resource: "split depth",
                limit: MAX_SPLIT_DEPTH,
            });
        }
        let selected = workspace
            .root
            .panes()
            .into_iter()
            .find(|p| p.id == pane_id)
            .expect("validated pane")
            .selected();
        let cwd = selected.cwd.clone().filter(|_| selected.remote.is_none());
        let remote = selected.remote.as_ref().map(|remote| remote.for_restore());
        let ids = self.reserve(3)?;
        let mut surface = Surface::terminal(SurfaceId(ids + 2), SessionId(ids + 1));
        surface.cwd = cwd.clone();
        surface.remote = remote.clone();
        let pane = Pane {
            id: PaneId(ids),
            selected_surface: SurfaceId(ids + 2),
            surfaces: vec![surface],
        };
        effects.push(Effect::StartSession {
            pane_id: pane.id,
            session_id: pane.session_id().expect("new terminal pane"),
            cwd,
            remote,
            local_tmux: None,
        });
        let workspace = &mut self.state.workspaces[index];
        let added_id = pane.id;
        workspace.root.split(pane_id, axis, pane, before);
        if focus {
            self.focus(index, added_id);
        }
        Ok(())
    }

    fn create_workspace(
        &mut self,
        name: String,
        remote: Option<zmux_remote_runtime::SshConnectionConfig>,
        focus: bool,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        let name = valid_name(name)?;
        if self.state.workspaces.len() == MAX_WORKSPACES {
            return Err(ActionError::Capacity {
                resource: "workspaces",
                limit: MAX_WORKSPACES,
            });
        }
        let ids = self.reserve(4)?;
        let workspace_id = WorkspaceId(ids);
        let mut surface = Surface::terminal(SurfaceId(ids + 3), SessionId(ids + 2));
        if let Some(connection) = &remote {
            surface.title = format!("ssh {}", connection.destination)
                .chars()
                .take(MAX_NAME_CHARS)
                .collect();
        }
        surface.remote = remote.clone();
        let pane = Pane {
            id: PaneId(ids + 1),
            selected_surface: surface.id,
            surfaces: vec![surface],
        };
        effects.push(Effect::StartSession {
            pane_id: pane.id,
            session_id: SessionId(ids + 2),
            cwd: None,
            remote,
            local_tmux: None,
        });
        self.state.workspaces.push(Workspace {
            id: workspace_id,
            name,
            focused_pane: pane.id,
            root: PaneTree::Leaf { pane },
        });
        self.own_workspace(workspace_id, focus)?;
        Ok(())
    }

    fn workspace_index(&self, pane_id: PaneId) -> Result<usize, ActionError> {
        self.state
            .workspaces
            .iter()
            .position(|w| w.root.depth_of(pane_id, 0).is_some())
            .ok_or(ActionError::UnknownPane(pane_id))
    }

    fn reserve(&mut self, count: u64) -> Result<u64, ActionError> {
        let next = self
            .next_id
            .checked_add(count)
            .ok_or(ActionError::Exhausted)?;
        let start = self.next_id;
        self.next_id = next;
        Ok(start)
    }
}

fn valid_name(name: String) -> Result<String, ActionError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ActionError::InvalidName);
    }
    Ok(name.to_owned())
}

fn default_focus() -> bool {
    true
}
