//! Shared workspace actions. This crate performs no PTY, UI, filesystem, or network I/O.

use serde::{Deserialize, Serialize};
use thiserror::Error;
mod surfaces;
pub mod targets;
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

pub const MODEL_VERSION: u32 = 3;
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

    fn split(&mut self, id: PaneId, axis: Axis, new_pane: Pane) {
        match self {
            Self::Leaf { pane } if pane.id == id => {
                *self = Self::Split {
                    axis,
                    first: Box::new(Self::Leaf { pane: pane.clone() }),
                    second: Box::new(Self::Leaf { pane: new_pane }),
                };
            }
            Self::Split { first, second, .. } => {
                if first.depth_of(id, 0).is_some() {
                    first.split(id, axis, new_pane);
                } else {
                    second.split(id, axis, new_pane);
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
            focused_workspace: None,
            workspaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    CreateBrowser {
        pane_id: PaneId,
        url: String,
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
    RenameWorkspace {
        workspace_id: WorkspaceId,
        name: String,
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
    restored_panes: std::collections::HashMap<PaneId, PaneId>,
    restored_workspaces: std::collections::HashMap<WorkspaceId, WorkspaceId>,
}

impl Default for WorkspaceStore {
    fn default() -> Self {
        Self {
            state: Snapshot::default(),
            next_id: 1,
            pending_restores: Vec::new(),
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
        let result = candidate.apply_inner(action)?;
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
            Action::CreateWorkspace { name } => {
                let name = valid_name(name)?;
                if self.state.workspaces.len() == MAX_WORKSPACES {
                    return Err(ActionError::Capacity {
                        resource: "workspaces",
                        limit: MAX_WORKSPACES,
                    });
                }
                let ids = self.reserve(4)?;
                let workspace_id = WorkspaceId(ids);
                let pane = Pane {
                    id: PaneId(ids + 1),
                    selected_surface: SurfaceId(ids + 3),
                    surfaces: vec![Surface::terminal(SurfaceId(ids + 3), SessionId(ids + 2))],
                };
                effects.push(Effect::StartSession {
                    pane_id: pane.id,
                    session_id: pane.session_id().expect("new terminal pane"),
                    cwd: None,
                });
                self.state.workspaces.push(Workspace {
                    id: workspace_id,
                    name,
                    focused_pane: pane.id,
                    root: PaneTree::Leaf { pane },
                });
                self.state.focused_workspace = Some(workspace_id);
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
                self.state.workspaces[index].focused_pane = pane_id;
                self.state.focused_workspace = Some(self.state.workspaces[index].id);
            }
            Action::SplitPane { pane_id, axis } => {
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
                let cwd = workspace
                    .root
                    .panes()
                    .into_iter()
                    .find(|p| p.id == pane_id)
                    .expect("validated pane")
                    .selected()
                    .cwd
                    .clone();
                let ids = self.reserve(3)?;
                let mut surface = Surface::terminal(SurfaceId(ids + 2), SessionId(ids + 1));
                surface.cwd = cwd.clone();
                let pane = Pane {
                    id: PaneId(ids),
                    selected_surface: SurfaceId(ids + 2),
                    surfaces: vec![surface],
                };
                effects.push(Effect::StartSession {
                    pane_id: pane.id,
                    session_id: pane.session_id().expect("new terminal pane"),
                    cwd,
                });
                let workspace = &mut self.state.workspaces[index];
                workspace.focused_pane = pane.id;
                workspace.root.split(pane_id, axis, pane);
                self.state.focused_workspace = Some(workspace.id);
            }
            Action::ClosePane { pane_id } => self.close_pane(pane_id, &mut effects)?,
            action => self.surface_action(action, &mut effects)?,
        }
        self.state.revision = revision;
        Ok(Transition { revision, effects })
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
