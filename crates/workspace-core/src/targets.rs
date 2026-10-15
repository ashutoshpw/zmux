//! Stable object addressing and ownership checks. Resolution uses one immutable model revision.
use crate::{
    Action, Axis, Direction, Navigation, PaneId, Snapshot, SurfaceId, WindowId, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Window,
    Workspace,
    Pane,
    Surface,
}
impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Window => "window",
            Self::Workspace => "workspace",
            Self::Pane => "pane",
            Self::Surface => "surface",
        })
    }
}

/// Numeric forms retain legacy stable-ID semantics. Explicit `index:N` is one-based.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetRef(String);
impl FromStr for TargetRef {
    type Err = TargetError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let target = Self(value.into());
        target.selector()?;
        Ok(target)
    }
}
impl fmt::Display for TargetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl TargetRef {
    fn selector(&self) -> Result<Selector, TargetError> {
        if self.0.len() > 128 {
            return Err(TargetError::InvalidTarget);
        }
        if let Some((prefix, id)) = self.0.split_once(':') {
            let id = positive(id)?;
            return match prefix {
                "window" => Ok(Selector::Id(Some(TargetKind::Window), id)),
                "workspace" => Ok(Selector::Id(Some(TargetKind::Workspace), id)),
                "pane" => Ok(Selector::Id(Some(TargetKind::Pane), id)),
                "surface" | "tab" => Ok(Selector::Id(Some(TargetKind::Surface), id)),
                "index" => Ok(Selector::Index(id)),
                _ => Err(TargetError::InvalidTarget),
            };
        }
        if let Ok(uuid) = Uuid::parse_str(&self.0)
            && uuid.hyphenated().to_string().eq_ignore_ascii_case(&self.0)
        {
            return Ok(Selector::Uuid(uuid));
        }
        Ok(Selector::Id(None, positive(&self.0)?))
    }
    pub fn is_index(&self) -> Result<bool, TargetError> {
        Ok(matches!(self.selector()?, Selector::Index(_)))
    }
}
fn positive(value: &str) -> Result<u64, TargetError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(TargetError::InvalidTarget);
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or(TargetError::InvalidTarget)
}
enum Selector {
    Id(Option<TargetKind>, u64),
    Uuid(Uuid),
    Index(u64),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TargetError {
    #[error("workspace model changed: expected revision {expected}, current revision {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error("target must be a UUID, positive ID, workspace:N/pane:N/surface:N/tab:N, or index:N")]
    InvalidTarget,
    #[error("expected a {expected} target, got {actual}")]
    WrongKind {
        expected: TargetKind,
        actual: TargetKind,
    },
    #[error("{kind} target {target} no longer exists")]
    NotFound { kind: TargetKind, target: String },
    #[error("{kind} target belongs to a different workspace or pane")]
    ScopeMismatch { kind: TargetKind },
    #[error("no focused {0} is available")]
    NoFocus(TargetKind),
    #[error("local tmux caller {0}; supply an explicit target")]
    LocalTmuxCaller(&'static str),
}
impl TargetError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::StaleRevision { .. } => "stale_revision",
            Self::InvalidTarget => "invalid_target",
            Self::WrongKind { .. } => "wrong_target_kind",
            Self::NotFound { .. } => "target_not_found",
            Self::ScopeMismatch { .. } => "target_scope_mismatch",
            Self::NoFocus(_) => "no_focused_target",
            Self::LocalTmuxCaller(_) => "local_tmux_caller_unavailable",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectIdentity {
    pub kind: TargetKind,
    pub id: u64,
    pub uuid: Uuid,
    pub reference: String,
    pub index: usize,
    pub workspace_id: WorkspaceId,
    pub window_id: WindowId,
    pub pane_id: Option<PaneId>,
}
impl Snapshot {
    pub fn resolve_window(&self, handle: &TargetRef) -> Result<WindowId, TargetError> {
        let selector = handle.selector()?;
        if let Selector::Id(Some(actual), _) = selector
            && actual != TargetKind::Window
        {
            return Err(TargetError::WrongKind {
                expected: TargetKind::Window,
                actual,
            });
        }
        self.windows
            .iter()
            .enumerate()
            .find(|(index, window)| match selector {
                Selector::Id(_, id) => window.id.0 == id,
                Selector::Uuid(uuid) => self.object_uuid(TargetKind::Window, window.id.0) == uuid,
                Selector::Index(id) => u64::try_from(index + 1).ok() == Some(id),
            })
            .map(|(_, window)| window.id)
            .ok_or_else(|| TargetError::NotFound {
                kind: TargetKind::Window,
                target: handle.to_string(),
            })
    }
    /// Workspace position selectors for ordering use canonical window membership,
    /// never the flat object catalog. Other commands retain their existing grammar.
    pub fn resolve_order_workspace(
        &self,
        handle: &TargetRef,
        window: Option<WindowId>,
    ) -> Result<(WindowId, WorkspaceId), TargetError> {
        let workspace = if let Selector::Index(index) = handle.selector()? {
            let window = window.ok_or(TargetError::NoFocus(TargetKind::Window))?;
            self.windows
                .iter()
                .find(|w| w.id == window)
                .and_then(|w| {
                    w.workspace_ids
                        .get(usize::try_from(index - 1).unwrap_or(usize::MAX))
                })
                .copied()
                .ok_or_else(|| TargetError::NotFound {
                    kind: TargetKind::Workspace,
                    target: handle.to_string(),
                })?
        } else {
            self.resolve_handle(handle, TargetKind::Workspace, None, None)?
                .workspace_id
        };
        let owner = self
            .windows
            .iter()
            .find(|w| w.workspace_ids.contains(&workspace))
            .ok_or_else(|| TargetError::NotFound {
                kind: TargetKind::Workspace,
                target: handle.to_string(),
            })?
            .id;
        if window.is_some_and(|window| window != owner) {
            return Err(TargetError::ScopeMismatch {
                kind: TargetKind::Window,
            });
        }
        Ok((owner, workspace))
    }
    pub fn object_uuid(&self, kind: TargetKind, id: u64) -> Uuid {
        Uuid::new_v5(&self.identity_namespace, format!("{kind}:{id}").as_bytes())
    }
    pub fn target_catalog(&self) -> Vec<ObjectIdentity> {
        let mut result = Vec::new();
        for (wi, workspace) in self.workspaces.iter().enumerate() {
            result.push(self.identity(
                TargetKind::Workspace,
                workspace.id.0,
                wi + 1,
                workspace.id,
                None,
            ));
            for (pi, pane) in workspace.root.panes().into_iter().enumerate() {
                result.push(self.identity(
                    TargetKind::Pane,
                    pane.id.0,
                    pi + 1,
                    workspace.id,
                    Some(pane.id),
                ));
                for (si, surface) in pane.surfaces.iter().enumerate() {
                    result.push(self.identity(
                        TargetKind::Surface,
                        surface.id.0,
                        si + 1,
                        workspace.id,
                        Some(pane.id),
                    ));
                }
            }
        }
        result
    }
    fn identity(
        &self,
        kind: TargetKind,
        id: u64,
        index: usize,
        workspace_id: WorkspaceId,
        pane_id: Option<PaneId>,
    ) -> ObjectIdentity {
        ObjectIdentity {
            kind,
            id,
            uuid: self.object_uuid(kind, id),
            reference: format!("{kind}:{id}"),
            index,
            workspace_id,
            window_id: self
                .windows
                .iter()
                .find(|window| window.workspace_ids.contains(&workspace_id))
                .expect("canonical workspace owner")
                .id,
            pane_id,
        }
    }
    fn resolve_handle(
        &self,
        handle: &TargetRef,
        kind: TargetKind,
        workspace: Option<WorkspaceId>,
        pane: Option<PaneId>,
    ) -> Result<ObjectIdentity, TargetError> {
        let catalog = self.target_catalog();
        let selector = handle.selector()?;
        if let Selector::Id(Some(actual), _) = selector
            && actual != kind
        {
            return Err(TargetError::WrongKind {
                expected: kind,
                actual,
            });
        }
        let found = match selector {
            Selector::Uuid(uuid) => catalog.iter().find(|o| o.uuid == uuid),
            Selector::Id(_, id) => catalog.iter().find(|o| o.id == id),
            Selector::Index(index) => catalog
                .iter()
                .filter(|o| {
                    o.kind == kind
                        && workspace.is_none_or(|w| o.workspace_id == w)
                        && pane.is_none_or(|p| o.pane_id == Some(p))
                })
                .nth(usize::try_from(index - 1).unwrap_or(usize::MAX)),
        }
        .ok_or_else(|| TargetError::NotFound {
            kind,
            target: handle.to_string(),
        })?;
        if found.kind != kind {
            return Err(TargetError::WrongKind {
                expected: kind,
                actual: found.kind,
            });
        }
        if workspace.is_some_and(|w| found.workspace_id != w)
            || pane.is_some_and(|p| found.pane_id != Some(p))
        {
            return Err(TargetError::ScopeMismatch { kind });
        }
        Ok(found.clone())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetScope {
    pub window: Option<TargetRef>,
    pub workspace: Option<TargetRef>,
    pub pane: Option<TargetRef>,
    pub surface: Option<TargetRef>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallerContext {
    pub window: Option<TargetRef>,
    #[serde(default)]
    pub local_tmux: Option<zmux_remote_runtime::local_multiplexer::LocalTmuxIdentity>,
    pub workspace: Option<TargetRef>,
    pub surface: Option<TargetRef>,
}
impl CallerContext {
    pub fn bind_local_tmux(&self, model: &Snapshot) -> Result<Self, TargetError> {
        let Some(identity) = &self.local_tmux else {
            return Ok(self.clone());
        };
        identity
            .validate()
            .map_err(|_| TargetError::InvalidTarget)?;
        if self.workspace.is_some() || self.surface.is_some() {
            return Err(TargetError::InvalidTarget);
        }
        let mut found = model.workspaces.iter().flat_map(|w| {
            w.root.panes().into_iter().flat_map(move |p| {
                p.surfaces
                    .iter()
                    .filter(move |s| s.local_tmux.as_ref() == Some(identity))
                    .map(move |s| (w.id, s.id))
            })
        });
        let (workspace, surface) = found
            .next()
            .ok_or(TargetError::LocalTmuxCaller("has no live app surface"))?;
        if found.next().is_some() {
            return Err(TargetError::LocalTmuxCaller("has multiple app surfaces"));
        }
        Ok(Self {
            window: None,
            local_tmux: None,
            workspace: Some(
                model
                    .object_uuid(TargetKind::Workspace, workspace.0)
                    .to_string()
                    .parse()?,
            ),
            surface: Some(
                model
                    .object_uuid(TargetKind::Surface, surface.0)
                    .to_string()
                    .parse()?,
            ),
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ResolvedTarget {
    pub window: WindowId,
    pub workspace: WorkspaceId,
    pub pane: PaneId,
    pub surface: SurfaceId,
}
impl TargetScope {
    /// Window roots are valid even with no selected workspace/pane/surface.
    pub fn resolve_window(
        &self,
        model: &Snapshot,
        caller: &CallerContext,
    ) -> Result<WindowId, TargetError> {
        if self.workspace.is_some() || self.pane.is_some() || self.surface.is_some() {
            return self.resolve(model, caller).map(|target| target.window);
        }
        if let Some(window) = &self.window {
            return model.resolve_window(window);
        }
        let caller = caller.bind_local_tmux(model)?;
        if caller.surface.is_some() || caller.workspace.is_some() {
            return self.resolve(model, &caller).map(|target| target.window);
        }
        if let Some(window) = &caller.window
            && let Ok(window) = model.resolve_window(window)
        {
            return Ok(window);
        }
        model
            .focused_window
            .ok_or(TargetError::NoFocus(TargetKind::Window))
    }
    pub fn resolve(
        &self,
        model: &Snapshot,
        caller: &CallerContext,
    ) -> Result<ResolvedTarget, TargetError> {
        let explicit = self.workspace.is_some() || self.pane.is_some() || self.surface.is_some();
        let mut constrained_window = self
            .window
            .as_ref()
            .map(|handle| model.resolve_window(handle))
            .transpose()?;
        let caller = if explicit {
            caller.clone()
        } else {
            caller.bind_local_tmux(model)?
        };
        if constrained_window.is_none()
            && !explicit
            && caller.workspace.is_none()
            && caller.surface.is_none()
        {
            constrained_window = caller
                .window
                .as_ref()
                .and_then(|handle| model.resolve_window(handle).ok());
        }
        let default_workspace = constrained_window
            .and_then(|window| {
                model
                    .windows
                    .iter()
                    .find(|w| w.id == window)
                    .and_then(|w| w.selected_workspace)
            })
            .or_else(|| {
                constrained_window
                    .is_none()
                    .then_some(model.focused_workspace)
                    .flatten()
            });
        let workspace_ref = self.workspace.as_ref().or_else(|| {
            (!explicit && caller.surface.is_none())
                .then_some(caller.workspace.as_ref())
                .flatten()
        });
        let surface_ref = self
            .surface
            .as_ref()
            .or_else(|| (!explicit).then_some(caller.surface.as_ref()).flatten());
        let mut workspace = workspace_ref
            .map(|r| {
                model
                    .resolve_handle(r, TargetKind::Workspace, None, None)
                    .map(|o| o.workspace_id)
            })
            .transpose()?;
        let mut pane = None;
        if let Some(handle) = &self.pane {
            let enclosing = if handle.is_index()? {
                workspace.or(default_workspace)
            } else {
                workspace
            };
            let object = model.resolve_handle(handle, TargetKind::Pane, enclosing, None)?;
            workspace = Some(object.workspace_id);
            pane = object.pane_id;
        }
        let mut surface = None;
        if let Some(handle) = surface_ref {
            let mut enclosing_pane = pane;
            let mut enclosing_workspace = workspace;
            if handle.is_index()? {
                enclosing_workspace = workspace.or(default_workspace);
                enclosing_pane = pane.or_else(|| {
                    model
                        .workspaces
                        .iter()
                        .find(|w| Some(w.id) == enclosing_workspace)
                        .map(|w| w.focused_pane)
                });
            }
            let object = model.resolve_handle(
                handle,
                TargetKind::Surface,
                enclosing_workspace,
                enclosing_pane,
            )?;
            workspace = Some(object.workspace_id);
            pane = object.pane_id;
            surface = Some(SurfaceId(object.id));
        }
        let workspace = workspace
            .or_else(|| {
                constrained_window.and_then(|window| {
                    model
                        .windows
                        .iter()
                        .find(|w| w.id == window)
                        .and_then(|w| w.selected_workspace)
                })
            })
            .or_else(|| {
                constrained_window
                    .is_none()
                    .then_some(model.focused_workspace)
                    .flatten()
            })
            .ok_or(TargetError::NoFocus(TargetKind::Workspace))?;
        let owner = model
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .ok_or(TargetError::NoFocus(TargetKind::Workspace))?;
        let pane = pane.unwrap_or(owner.focused_pane);
        let pane_object = owner
            .root
            .panes()
            .into_iter()
            .find(|p| p.id == pane)
            .ok_or(TargetError::NoFocus(TargetKind::Pane))?;
        let window = model
            .windows
            .iter()
            .find(|window| window.workspace_ids.contains(&workspace))
            .ok_or(TargetError::NoFocus(TargetKind::Window))?
            .id;
        if constrained_window.is_some_and(|expected| expected != window) {
            return Err(TargetError::ScopeMismatch {
                kind: TargetKind::Window,
            });
        }
        Ok(ResolvedTarget {
            window,
            workspace,
            pane,
            surface: surface.unwrap_or(pane_object.selected_surface),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetAction {
    MoveWorkspaceToWindow {
        destination: TargetRef,
        #[serde(default)]
        focus: bool,
    },
    RenameWorkspace {
        name: String,
    },
    CloseWorkspace,
    SelectWorkspace,
    NavigateWorkspace {
        direction: Navigation,
    },
    NavigateSurface {
        direction: Navigation,
    },
    NavigatePane {
        direction: Navigation,
    },
    FocusPaneDirection {
        direction: Direction,
    },
    SplitPaneDirectional {
        direction: Direction,
        #[serde(default)]
        focus: bool,
    },
    FocusPane,
    SplitPane {
        axis: Axis,
    },
    ClosePane,
    CreateSurface,
    SelectSurface,
    CloseSurface,
    RenameSurface {
        name: String,
    },
    ReorderSurface {
        index: usize,
        #[serde(default)]
        focus: bool,
    },
    MoveSurface {
        destination: TargetRef,
        index: usize,
    },
    CloseOtherSurfaces,
    CreateBrowser {
        #[serde(default)]
        profile: Option<String>,
        url: String,
        #[serde(default = "crate::default_focus")]
        focus: bool,
    },
    NavigateBrowser {
        url: String,
    },
}
impl TargetAction {
    pub fn resolve(
        self,
        model: &Snapshot,
        scope: &TargetScope,
        caller: &CallerContext,
    ) -> Result<Action, TargetError> {
        let target = scope.resolve(model, caller)?;
        Ok(match self {
            Self::MoveWorkspaceToWindow { destination, focus } => Action::MoveWorkspaceToWindow {
                workspace_id: target.workspace,
                destination_window_id: model.resolve_window(&destination)?,
                index: None,
                focus,
            },
            Self::RenameWorkspace { name } => Action::RenameWorkspace {
                workspace_id: target.workspace,
                name,
            },
            Self::CloseWorkspace => Action::CloseWorkspace {
                workspace_id: target.workspace,
            },
            Self::SelectWorkspace => Action::SelectWorkspace {
                workspace_id: target.workspace,
            },
            Self::NavigateWorkspace { direction } => Action::NavigateWorkspace {
                workspace_id: target.workspace,
                direction,
            },
            Self::NavigateSurface { direction } => Action::NavigateSurface {
                surface_id: target.surface,
                direction,
            },
            Self::NavigatePane { direction } => Action::NavigatePane {
                pane_id: target.pane,
                direction,
            },
            Self::FocusPaneDirection { direction } => Action::FocusPaneDirection {
                pane_id: target.pane,
                direction,
            },
            Self::SplitPaneDirectional { direction, focus } => Action::SplitPaneDirectional {
                pane_id: target.pane,
                direction,
                focus,
            },
            Self::FocusPane => Action::FocusPane {
                pane_id: target.pane,
            },
            Self::SplitPane { axis } => Action::SplitPane {
                pane_id: target.pane,
                axis,
            },
            Self::ClosePane => Action::ClosePane {
                pane_id: target.pane,
            },
            Self::CreateSurface => Action::CreateSurface {
                pane_id: target.pane,
            },
            Self::SelectSurface => Action::SelectSurface {
                surface_id: target.surface,
            },
            Self::CloseSurface => Action::CloseSurface {
                surface_id: target.surface,
            },
            Self::RenameSurface { name } => Action::RenameSurface {
                surface_id: target.surface,
                name,
            },
            Self::ReorderSurface { index, focus } => Action::ReorderSurface {
                surface_id: target.surface,
                index,
                focus,
            },
            Self::MoveSurface { destination, index } => {
                let workspace = if destination.is_index()? {
                    Some(format!("workspace:{}", target.workspace.0).parse()?)
                } else {
                    None
                };
                let destination = TargetScope {
                    workspace,
                    pane: Some(destination),
                    ..TargetScope::default()
                }
                .resolve(model, &CallerContext::default())?;
                Action::MoveSurface {
                    surface_id: target.surface,
                    pane_id: destination.pane,
                    index,
                }
            }
            Self::CloseOtherSurfaces => Action::CloseOtherSurfaces {
                surface_id: target.surface,
            },
            Self::CreateBrowser {
                url,
                focus,
                profile,
            } => Action::CreateBrowser {
                profile_id: profile
                    .map(|value| {
                        uuid::Uuid::parse_str(&value).map_err(|_| TargetError::InvalidTarget)
                    })
                    .transpose()?,
                pane_id: target.pane,
                url,
                focus,
            },
            Self::NavigateBrowser { url } => Action::NavigateBrowser {
                surface_id: target.surface,
                url,
            },
        })
    }
}
