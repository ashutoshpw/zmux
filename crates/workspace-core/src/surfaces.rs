//! Terminal tabs and bounded undo-close descriptors. No process objects or terminal input live here.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    Terminal,
    Browser,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Surface {
    pub id: SurfaceId,
    pub kind: SurfaceKind,
    pub session_id: Option<SessionId>,
    pub url: Option<String>,
    pub navigation: u64,
    pub title: String,
    pub cwd: Option<String>,
    pub remote: Option<zmux_remote_runtime::SshConnectionConfig>,
}
impl Surface {
    pub(crate) fn terminal(id: SurfaceId, session_id: SessionId) -> Self {
        Self {
            id,
            kind: SurfaceKind::Terminal,
            session_id: Some(session_id),
            url: None,
            navigation: 0,
            title: "Terminal".into(),
            cwd: None,
            remote: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedTab {
    pub surface: Surface,
    pub index: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClosedContent {
    Tabs {
        pane_id: PaneId,
        tabs: Vec<ClosedTab>,
    },
    Pane {
        pane: Pane,
    },
    Workspace {
        workspace: Workspace,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedItem {
    pub id: u64,
    pub title: String,
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub restoring: bool,
    pub content: ClosedContent,
}
#[derive(Clone, Debug)]
pub(crate) struct PendingRestore {
    history_id: u64,
    surfaces: Vec<SurfaceId>,
    waiting: Vec<SessionId>,
    prior_focus: Option<(WorkspaceId, PaneId, SurfaceId)>,
}

impl WorkspaceStore {
    pub(crate) fn surface_action(
        &mut self,
        action: Action,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        match &action {
            Action::NavigateBrowser { surface_id, url }
            | Action::ObserveBrowser {
                surface_id, url, ..
            } => {
                validate_browser_url(url)?;
                let (wi, pane, index) = self.location(*surface_id)?;
                let surface = &mut self.state.workspaces[wi]
                    .root
                    .pane_mut(pane)
                    .unwrap()
                    .surfaces[index];
                if surface.kind != SurfaceKind::Browser {
                    return Err(ActionError::InvalidOperation(
                        "surface is not a browser".into(),
                    ));
                }
                if let Action::ObserveBrowser { title, .. } = &action {
                    let title: String = title
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(MAX_NAME_CHARS)
                        .collect();
                    if !title.is_empty() {
                        surface.title = title;
                    }
                } else {
                    surface.navigation = surface
                        .navigation
                        .checked_add(1)
                        .ok_or(ActionError::Exhausted)?;
                }
                surface.url = Some(url.clone());
                return Ok(());
            }
            _ => {}
        }
        match action {
            Action::CreateBrowser { pane_id, url } => {
                validate_browser_url(&url)?;
                let wi = self.workspace_index(pane_id)?;
                let pane = self.pane(pane_id)?;
                Self::check_tab_capacity(pane.surfaces.len() + 1)?;
                let id = SurfaceId(self.reserve(1)?);
                let surface = Surface {
                    id,
                    kind: SurfaceKind::Browser,
                    session_id: None,
                    url: Some(url),
                    navigation: 0,
                    title: "Browser".into(),
                    cwd: None,
                    remote: None,
                };
                let pane = self.state.workspaces[wi].root.pane_mut(pane_id).unwrap();
                pane.selected_surface = id;
                pane.surfaces.push(surface);
                self.focus(wi, pane_id);
            }
            Action::NavigateBrowser { surface_id, url }
            | Action::ObserveBrowser {
                surface_id,
                url,
                title: _,
            } => {
                // Observation is handled separately below to keep navigation generations distinct.
                let _ = (surface_id, url);
                unreachable!("browser navigation dispatched before generic actions")
            }
            Action::CreateSurface { pane_id } => {
                let wi = self.workspace_index(pane_id)?;
                let pane = self.pane(pane_id)?;
                Self::check_tab_capacity(pane.surfaces.len() + 1)?;
                let cwd = pane
                    .selected()
                    .cwd
                    .clone()
                    .filter(|_| pane.selected().remote.is_none());
                let remote = pane
                    .selected()
                    .remote
                    .as_ref()
                    .map(|remote| remote.for_restore());
                let ids = self.reserve(2)?;
                let mut surface = Surface::terminal(SurfaceId(ids), SessionId(ids + 1));
                surface.cwd = cwd;
                surface.remote = remote;
                Self::start(pane_id, &surface, effects);
                let pane = self.state.workspaces[wi]
                    .root
                    .pane_mut(pane_id)
                    .expect("validated pane");
                let index = pane
                    .surfaces
                    .iter()
                    .position(|s| s.id == pane.selected_surface)
                    .unwrap()
                    + 1;
                pane.selected_surface = surface.id;
                pane.surfaces.insert(index, surface);
                self.focus(wi, pane_id);
            }
            Action::SelectSurface { surface_id } => {
                let (wi, pane_id, _) = self.location(surface_id)?;
                self.state.workspaces[wi]
                    .root
                    .pane_mut(pane_id)
                    .unwrap()
                    .selected_surface = surface_id;
                self.focus(wi, pane_id);
            }
            Action::RenameSurface { surface_id, name } => {
                let name = valid_name(name)?;
                let (wi, pane_id, index) = self.location(surface_id)?;
                self.state.workspaces[wi]
                    .root
                    .pane_mut(pane_id)
                    .unwrap()
                    .surfaces[index]
                    .title = name;
            }
            Action::CloseSurface { surface_id } => {
                let (wi, pane_id, index) = self.location(surface_id)?;
                self.ensure_settled(&[surface_id])?;
                let surface = self.pane(pane_id)?.surfaces[index].clone();
                self.remember(
                    wi,
                    surface.title.clone(),
                    ClosedContent::Tabs {
                        pane_id,
                        tabs: vec![ClosedTab { surface, index }],
                    },
                )?;
                self.remove_surface(surface_id, effects)?;
            }
            Action::CloseOtherSurfaces { surface_id } => {
                let (wi, pane_id, _) = self.location(surface_id)?;
                let pane = self.pane(pane_id)?;
                let ids: Vec<_> = pane.surfaces.iter().map(|s| s.id).collect();
                self.ensure_settled(&ids)?;
                let tabs: Vec<_> = pane
                    .surfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.id != surface_id)
                    .map(|(index, surface)| ClosedTab {
                        surface: surface.clone(),
                        index,
                    })
                    .collect();
                if !tabs.is_empty() {
                    self.remember(
                        wi,
                        "Other terminal tabs".into(),
                        ClosedContent::Tabs { pane_id, tabs },
                    )?;
                    for id in ids.into_iter().filter(|id| *id != surface_id) {
                        self.remove_surface(id, effects)?;
                    }
                }
                self.state.workspaces[wi]
                    .root
                    .pane_mut(pane_id)
                    .unwrap()
                    .selected_surface = surface_id;
                self.focus(wi, pane_id);
            }
            Action::ReorderSurface { surface_id, index } => {
                let (_, pane_id, _) = self.location(surface_id)?;
                self.move_surface(surface_id, pane_id, index)?;
            }
            Action::MoveSurface {
                surface_id,
                pane_id,
                index,
            } => self.move_surface(surface_id, pane_id, index)?,
            Action::CloseWorkspace { workspace_id } => {
                let wi = self
                    .state
                    .workspaces
                    .iter()
                    .position(|w| w.id == workspace_id)
                    .ok_or(ActionError::UnknownWorkspace(workspace_id))?;
                let workspace = self.state.workspaces[wi].clone();
                let ids: Vec<_> = workspace
                    .root
                    .panes()
                    .iter()
                    .flat_map(|p| p.surfaces.iter().map(|s| s.id))
                    .collect();
                self.ensure_settled(&ids)?;
                self.remember(
                    wi,
                    workspace.name.clone(),
                    ClosedContent::Workspace { workspace },
                )?;
                for id in ids {
                    self.remove_surface(id, effects)?;
                }
            }
            Action::ReopenClosed { closed_id } => self.reopen(closed_id, effects)?,
            _ => unreachable!("other actions handled by root dispatcher"),
        }
        Ok(())
    }

    pub(crate) fn close_pane(
        &mut self,
        pane_id: PaneId,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        let wi = self.workspace_index(pane_id)?;
        let pane = self.pane(pane_id)?.clone();
        let ids: Vec<_> = pane.surfaces.iter().map(|s| s.id).collect();
        self.ensure_settled(&ids)?;
        self.remember(wi, "Terminal pane".into(), ClosedContent::Pane { pane })?;
        for id in ids {
            self.remove_surface(id, effects)?;
        }
        Ok(())
    }

    fn pane(&self, id: PaneId) -> Result<&Pane, ActionError> {
        self.state
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .find(|p| p.id == id)
            .ok_or(ActionError::UnknownPane(id))
    }
    fn location(&self, id: SurfaceId) -> Result<(usize, PaneId, usize), ActionError> {
        for (wi, workspace) in self.state.workspaces.iter().enumerate() {
            for pane in workspace.root.panes() {
                if let Some(index) = pane.surfaces.iter().position(|s| s.id == id) {
                    return Ok((wi, pane.id, index));
                }
            }
        }
        Err(ActionError::UnknownSurface(id))
    }
    fn focus(&mut self, wi: usize, pane_id: PaneId) {
        self.state.workspaces[wi].focused_pane = pane_id;
        self.state.focused_workspace = Some(self.state.workspaces[wi].id);
    }
    fn focused(&self) -> Option<(WorkspaceId, PaneId, SurfaceId)> {
        let workspace = self
            .state
            .workspaces
            .iter()
            .find(|w| Some(w.id) == self.state.focused_workspace)?;
        let pane = workspace
            .root
            .panes()
            .into_iter()
            .find(|p| p.id == workspace.focused_pane)?;
        Some((workspace.id, pane.id, pane.selected_surface))
    }
    fn ensure_settled(&self, ids: &[SurfaceId]) -> Result<(), ActionError> {
        if self
            .pending_restores
            .iter()
            .any(|r| r.surfaces.iter().any(|id| ids.contains(id)))
        {
            return Err(ActionError::InvalidOperation(
                "terminal restoration is in progress; retry after it finishes".into(),
            ));
        }
        Ok(())
    }
    fn check_tab_capacity(count: usize) -> Result<(), ActionError> {
        if count > MAX_SURFACES_PER_PANE {
            return Err(ActionError::Capacity {
                resource: "tabs per pane",
                limit: MAX_SURFACES_PER_PANE,
            });
        }
        Ok(())
    }
    fn check_workspace_capacity(&self) -> Result<(), ActionError> {
        if self.state.workspaces.len() >= MAX_WORKSPACES {
            return Err(ActionError::Capacity {
                resource: "workspaces",
                limit: MAX_WORKSPACES,
            });
        }
        Ok(())
    }
    fn start(pane_id: PaneId, surface: &Surface, effects: &mut Vec<Effect>) {
        if let Some(session_id) = surface.session_id {
            effects.push(Effect::StartSession {
                pane_id,
                session_id,
                cwd: surface.cwd.clone(),
                remote: surface.remote.clone(),
            });
        }
    }
    fn remove_surface(
        &mut self,
        id: SurfaceId,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        let (wi, pane_id, index) = self.location(id)?;
        let pane = self.state.workspaces[wi].root.pane_mut(pane_id).unwrap();
        let surface = pane.surfaces.remove(index);
        if let Some(session_id) = surface.session_id {
            effects.push(Effect::StopSession { session_id });
        }
        if !pane.surfaces.is_empty() {
            if pane.selected_surface == id {
                pane.selected_surface = pane.surfaces[index.min(pane.surfaces.len() - 1)].id;
            }
        } else {
            match self.state.workspaces[wi].root.clone().without(pane_id) {
                Some(root) => {
                    let workspace = &mut self.state.workspaces[wi];
                    if workspace.focused_pane == pane_id {
                        workspace.focused_pane = root.panes()[0].id;
                    }
                    workspace.root = root;
                }
                None => {
                    let workspace = self.state.workspaces.remove(wi);
                    if self.state.focused_workspace == Some(workspace.id) {
                        self.state.focused_workspace = self.state.workspaces.last().map(|w| w.id);
                    }
                }
            }
        }
        Ok(())
    }
    fn move_surface(
        &mut self,
        id: SurfaceId,
        destination: PaneId,
        index: usize,
    ) -> Result<(), ActionError> {
        let (wi, source, old_index) = self.location(id)?;
        self.ensure_settled(&[id])?;
        let dest = self.pane(destination)?;
        let limit = dest.surfaces.len() - usize::from(source == destination);
        if index > limit {
            return Err(ActionError::InvalidOperation(format!(
                "tab index must be 0–{limit}"
            )));
        }
        if source != destination {
            Self::check_tab_capacity(dest.surfaces.len() + 1)?;
        }
        if source == destination && limit == 0 {
            return Ok(());
        }
        let surface = self.state.workspaces[wi]
            .root
            .pane_mut(source)
            .unwrap()
            .surfaces[old_index]
            .clone();
        // Reuse topology cleanup without emitting a stop: moving retains the live session.
        self.remove_surface(id, &mut Vec::new())?;
        let dest_wi = self.workspace_index(destination)?;
        let pane = self.state.workspaces[dest_wi]
            .root
            .pane_mut(destination)
            .unwrap();
        pane.selected_surface = id;
        pane.surfaces.insert(index, surface);
        self.focus(dest_wi, destination);
        Ok(())
    }
    fn remember(
        &mut self,
        wi: usize,
        title: String,
        content: ClosedContent,
    ) -> Result<(), ActionError> {
        let id = self.reserve(1)?;
        let workspace = &self.state.workspaces[wi];
        let item = ClosedItem {
            id,
            title,
            workspace_id: workspace.id,
            workspace_name: workspace.name.clone(),
            restoring: false,
            content,
        };
        if self.state.closed_items.len() >= MAX_CLOSED_ITEMS {
            let index = self
                .state
                .closed_items
                .iter()
                .position(|item| !item.restoring)
                .ok_or_else(|| {
                    ActionError::InvalidOperation(
                        "closed-item history is busy restoring; retry".into(),
                    )
                })?;
            self.state.closed_items.remove(index);
        }
        self.state.closed_items.push(item);
        self.prune_restore_anchors();
        Ok(())
    }

    fn fresh_surface(
        &mut self,
        old: &Surface,
        pane_id: PaneId,
        effects: &mut Vec<Effect>,
    ) -> Result<Surface, ActionError> {
        let ids = self.reserve(2)?;
        let surface = Surface {
            id: SurfaceId(ids),
            kind: old.kind.clone(),
            session_id: old.session_id.map(|_| SessionId(ids + 1)),
            url: old.url.clone(),
            navigation: 0,
            title: old.title.clone(),
            cwd: old.cwd.clone().filter(|_| old.remote.is_none()),
            remote: old.remote.as_ref().map(|remote| remote.for_restore()),
        };
        Self::start(pane_id, &surface, effects);
        Ok(surface)
    }
    fn fresh_tree(
        &mut self,
        old: &PaneTree,
        effects: &mut Vec<Effect>,
    ) -> Result<PaneTree, ActionError> {
        Ok(match old {
            PaneTree::Leaf { pane } => {
                let pane_id = PaneId(self.reserve(1)?);
                self.restored_panes.insert(pane.id, pane_id);
                let mut surfaces = Vec::new();
                let selected_index = pane
                    .surfaces
                    .iter()
                    .position(|s| s.id == pane.selected_surface)
                    .unwrap();
                for old in &pane.surfaces {
                    surfaces.push(self.fresh_surface(old, pane_id, effects)?);
                }
                PaneTree::Leaf {
                    pane: Pane {
                        id: pane_id,
                        selected_surface: surfaces[selected_index].id,
                        surfaces,
                    },
                }
            }
            PaneTree::Split {
                axis,
                first,
                second,
            } => PaneTree::Split {
                axis: *axis,
                first: Box::new(self.fresh_tree(first, effects)?),
                second: Box::new(self.fresh_tree(second, effects)?),
            },
        })
    }
    fn reopen(&mut self, id: Option<u64>, effects: &mut Vec<Effect>) -> Result<(), ActionError> {
        let index = match id {
            Some(id) => self.state.closed_items.iter().position(|r| r.id == id),
            None => self.state.closed_items.len().checked_sub(1),
        }
        .ok_or_else(|| ActionError::InvalidOperation("no matching recently closed item".into()))?;
        if self.state.closed_items[index].restoring {
            return Err(ActionError::InvalidOperation(
                "this item is already restoring".into(),
            ));
        }
        let record = self.state.closed_items[index].clone();
        let prior_focus = self.focused();
        let prior_surfaces: Vec<_> = self
            .state
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .map(|s| s.id)
            .collect();
        match &record.content {
            ClosedContent::Tabs { pane_id, tabs } => {
                let original_pane = *pane_id;
                let recovered_pane = self
                    .restored_panes
                    .get(pane_id)
                    .copied()
                    .unwrap_or(*pane_id);
                let recovered_workspace = self
                    .restored_workspaces
                    .get(&record.workspace_id)
                    .copied()
                    .unwrap_or(record.workspace_id);
                let destination = self
                    .pane(*pane_id)
                    .or_else(|_| self.pane(recovered_pane))
                    .ok()
                    .map(|p| p.id)
                    .or_else(|| {
                        self.state
                            .workspaces
                            .iter()
                            .find(|w| w.id == recovered_workspace)
                            .map(|w| w.focused_pane)
                    });
                let pane_id = if let Some(id) = destination {
                    Self::check_tab_capacity(self.pane(id)?.surfaces.len() + tabs.len())?;
                    id
                } else {
                    self.check_workspace_capacity()?;
                    let ids = self.reserve(2)?;
                    let pane_id = PaneId(ids + 1);
                    self.restored_panes.insert(original_pane, pane_id);
                    self.restored_workspaces
                        .insert(record.workspace_id, WorkspaceId(ids));
                    self.state.workspaces.push(Workspace {
                        id: WorkspaceId(ids),
                        name: record.workspace_name.clone(),
                        focused_pane: pane_id,
                        root: PaneTree::Leaf {
                            pane: Pane {
                                id: pane_id,
                                selected_surface: SurfaceId(0),
                                surfaces: Vec::new(),
                            },
                        },
                    });
                    pane_id
                };
                for tab in tabs {
                    let surface = self.fresh_surface(&tab.surface, pane_id, effects)?;
                    let wi = self.workspace_index(pane_id)?;
                    let pane = self.state.workspaces[wi].root.pane_mut(pane_id).unwrap();
                    pane.selected_surface = surface.id;
                    pane.surfaces
                        .insert(tab.index.min(pane.surfaces.len()), surface);
                }
                let wi = self.workspace_index(pane_id)?;
                self.focus(wi, pane_id);
            }
            ClosedContent::Pane { pane } => {
                // A removed split has no live anchor in this first slice. Restore its grouped
                // tabs in a new workspace rather than displacing a surviving split or session.
                self.restore_workspace(
                    &record.workspace_name,
                    &PaneTree::Leaf { pane: pane.clone() },
                    0,
                    effects,
                )?;
            }
            ClosedContent::Workspace { workspace } => {
                let selected = workspace
                    .root
                    .panes()
                    .iter()
                    .position(|p| p.id == workspace.focused_pane)
                    .unwrap_or(0);
                self.restore_workspace(&workspace.name, &workspace.root, selected, effects)?;
            }
        }
        if let Some(id) = self.state.focused_workspace {
            self.restored_workspaces.insert(record.workspace_id, id);
        }
        let waiting: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::StartSession { session_id, .. } => Some(*session_id),
                _ => None,
            })
            .collect();
        let surfaces = self
            .state
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .filter(|s| !prior_surfaces.contains(&s.id))
            .map(|s| s.id)
            .collect();
        if waiting.is_empty() {
            self.state.closed_items.remove(index);
            self.prune_restore_anchors();
            return Ok(());
        }
        self.pending_restores.push(PendingRestore {
            history_id: record.id,
            surfaces,
            waiting,
            prior_focus,
        });
        self.state.closed_items[index].restoring = true;
        Ok(())
    }
    fn restore_workspace(
        &mut self,
        name: &str,
        old: &PaneTree,
        selected: usize,
        effects: &mut Vec<Effect>,
    ) -> Result<(), ActionError> {
        self.check_workspace_capacity()?;
        let id = WorkspaceId(self.reserve(1)?);
        let root = self.fresh_tree(old, effects)?;
        let focused_pane = root.panes()[selected].id;
        self.state.workspaces.push(Workspace {
            id,
            name: name.into(),
            focused_pane,
            root,
        });
        self.state.focused_workspace = Some(id);
        Ok(())
    }

    /// Called only by the coordinator after a real PTY launch resolves. A failed grouped
    /// restore removes every provisional surface and leaves the descriptor available to retry.
    pub fn finish_restore(&mut self, session: SessionId, success: bool) -> Vec<Effect> {
        let Some(index) = self
            .pending_restores
            .iter()
            .position(|r| r.waiting.contains(&session))
        else {
            return Vec::new();
        };
        let mut restore = self.pending_restores.remove(index);
        let mut effects = Vec::new();
        if success {
            restore.waiting.retain(|id| *id != session);
            if restore.waiting.is_empty() {
                self.state
                    .closed_items
                    .retain(|r| r.id != restore.history_id);
            } else {
                self.pending_restores.push(restore);
            }
        } else {
            let focus_was_restored = self
                .focused()
                .is_some_and(|(_, _, id)| restore.surfaces.contains(&id));
            for id in restore.surfaces {
                if self.location(id).is_ok() {
                    self.remove_surface(id, &mut effects)
                        .expect("located surface");
                }
            }
            if let Some(item) = self
                .state
                .closed_items
                .iter_mut()
                .find(|r| r.id == restore.history_id)
            {
                item.restoring = false;
            }
            if focus_was_restored
                && let Some((_, _, surface)) = restore.prior_focus
                && let Ok((wi, pane, _)) = self.location(surface)
            {
                self.state.workspaces[wi]
                    .root
                    .pane_mut(pane)
                    .unwrap()
                    .selected_surface = surface;
                self.focus(wi, pane);
            }
        }
        self.prune_restore_anchors();
        self.state.revision += 1;
        effects
    }

    fn prune_restore_anchors(&mut self) {
        let workspaces: Vec<_> = self
            .state
            .closed_items
            .iter()
            .map(|r| r.workspace_id)
            .collect();
        let panes: Vec<_> = self
            .state
            .closed_items
            .iter()
            .flat_map(|r| match &r.content {
                ClosedContent::Tabs { pane_id, .. } => vec![*pane_id],
                ClosedContent::Pane { pane } => vec![pane.id],
                ClosedContent::Workspace { workspace } => {
                    workspace.root.panes().iter().map(|p| p.id).collect()
                }
            })
            .collect();
        self.restored_workspaces
            .retain(|old, _| workspaces.contains(old));
        self.restored_panes.retain(|old, _| panes.contains(old));
    }

    /// Refresh restore metadata from the owning runtime. Never stores terminal input.
    pub fn set_session_cwd(&mut self, session_id: SessionId, cwd: String) {
        if !cwd.starts_with('/') || cwd.len() > 4096 || cwd.chars().any(char::is_control) {
            return;
        }
        for workspace in &mut self.state.workspaces {
            let ids: Vec<_> = workspace.root.panes().iter().map(|p| p.id).collect();
            for id in ids {
                for surface in &mut workspace.root.pane_mut(id).unwrap().surfaces {
                    if surface.session_id == Some(session_id) {
                        surface.cwd = Some(cwd);
                        return;
                    }
                }
            }
        }
    }
}

/// Browser content cannot select local files, scripts or privileged URL handlers.
fn validate_browser_url(url: &str) -> Result<(), ActionError> {
    if url == "about:blank" {
        return Ok(());
    }
    let authority = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    if url.len() > 8192
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
        || authority.is_none_or(|rest| {
            let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
            host.is_empty() || host.contains('@') || host.contains('\\')
        })
    {
        return Err(ActionError::InvalidOperation(
            "browser URL must be http://, https:// or about:blank without credentials".into(),
        ));
    }
    Ok(())
}
