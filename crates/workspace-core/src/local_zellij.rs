//! Attach-only recovery values and allocator-backed provisional surface identities.
use crate::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_LOCAL_ZELLIJ_RESERVATIONS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalZellijIdentity {
    pub profile_id: Uuid,
    pub owner_uuid: Uuid,
}
impl LocalZellijIdentity {
    pub fn validate(&self) -> Result<(), ActionError> {
        if self.profile_id.is_nil() || self.owner_uuid.is_nil() {
            return Err(ActionError::InvalidOperation(
                "local Zellij profile and owner UUIDs must be nonzero".into(),
            ));
        }
        Ok(())
    }
}

/// Not serialized or constructible from a public action. Cancellation burns its IDs.
#[derive(Debug)]
pub struct LocalZellijReservation {
    namespace: Uuid,
    workspace: WorkspaceId,
    pane: PaneId,
    surface: SurfaceId,
    session: SessionId,
}
impl LocalZellijReservation {
    pub fn surface_id(&self) -> SurfaceId {
        self.surface
    }
    pub fn session_id(&self) -> SessionId {
        self.session
    }
    pub fn identity_namespace(&self) -> Uuid {
        self.namespace
    }
}

/// Prepared before submitting any checkpoint. On the serialized owner, installing
/// this exact candidate after queue acceptance is an infallible assignment.
#[derive(Debug)]
pub struct PreparedLocalZellijSurface {
    store: WorkspaceStore,
}
impl PreparedLocalZellijSurface {
    pub fn store(&self) -> &WorkspaceStore {
        &self.store
    }
    pub fn into_store(self) -> WorkspaceStore {
        self.store
    }
}

impl WorkspaceStore {
    /// The runtime first checks this candidate's allocator against its durable
    /// reserved block, then commits the allocator advance before any CLI spawn.
    /// Reserve on the live owner (or immediately install the checked clone), never
    /// spawn from an uncommitted allocator clone. No surface or effect is created.
    pub fn reserve_local_zellij_surface(
        &mut self,
        pane: PaneId,
    ) -> Result<LocalZellijReservation, ActionError> {
        if self.state.identity_namespace.is_nil() {
            return Err(ActionError::InvalidOperation(
                "local Zellij reservation requires a nonzero identity namespace".into(),
            ));
        }
        let wi = self.workspace_index(pane)?;
        let target = self.pane(pane)?;
        let ids: Vec<_> = target.surfaces.iter().map(|s| s.id).collect();
        self.ensure_settled(&ids)?;
        Self::check_tab_capacity(target.surfaces.len() + 1)?;
        if self.local_zellij_reservations.len() >= MAX_LOCAL_ZELLIJ_RESERVATIONS {
            return Err(ActionError::Capacity {
                resource: "local Zellij reservations",
                limit: MAX_LOCAL_ZELLIJ_RESERVATIONS,
            });
        }
        let workspace = self.state.workspaces[wi].id;
        let first = self.reserve(2)?;
        let ticket = LocalZellijReservation {
            namespace: self.state.identity_namespace,
            workspace,
            pane,
            surface: SurfaceId(first),
            session: SessionId(first + 1),
        };
        self.local_zellij_reservations
            .insert(ticket.surface, (workspace, pane, ticket.session));
        Ok(ticket)
    }
    fn check_local_zellij_reservation(
        &self,
        ticket: &LocalZellijReservation,
    ) -> Result<(), ActionError> {
        if ticket.namespace != self.state.identity_namespace
            || self.local_zellij_reservations.get(&ticket.surface)
                != Some(&(ticket.workspace, ticket.pane, ticket.session))
        {
            return Err(ActionError::InvalidOperation(
                "local Zellij reservation is stale or belongs to another owner".into(),
            ));
        }
        Ok(())
    }
    pub fn cancel_local_zellij_reservation(
        &mut self,
        ticket: LocalZellijReservation,
    ) -> Result<(), ActionError> {
        self.check_local_zellij_reservation(&ticket)?;
        self.local_zellij_reservations.remove(&ticket.surface);
        Ok(())
    }
    /// Plans against the current canonical owner; it neither publishes a surface
    /// nor starts another client. All failure is before checkpoint submission.
    pub fn prepare_local_zellij_surface(
        &self,
        ticket: &LocalZellijReservation,
        identity: LocalZellijIdentity,
        focus: bool,
    ) -> Result<PreparedLocalZellijSurface, ActionError> {
        self.check_local_zellij_reservation(ticket)?;
        identity.validate()?;
        let mut candidate = self.clone();
        let wi = candidate.workspace_index(ticket.pane)?;
        if candidate.state.workspaces[wi].id != ticket.workspace {
            return Err(ActionError::UnknownWorkspace(ticket.workspace));
        }
        let pane = candidate.pane(ticket.pane)?;
        candidate.ensure_settled(&pane.surfaces.iter().map(|s| s.id).collect::<Vec<_>>())?;
        Self::check_tab_capacity(pane.surfaces.len() + 1)?;
        let prior = candidate.focused();
        let prior_window = candidate.state.focused_window;
        let mut surface = Surface::terminal(ticket.surface, ticket.session);
        surface.local_zellij = Some(identity);
        surface.title = "Persistent terminal".into();
        let pane = candidate.state.workspaces[wi]
            .root
            .pane_mut(ticket.pane)
            .expect("checked pane");
        if focus {
            pane.selected_surface = ticket.surface;
        }
        pane.surfaces.push(surface);
        if focus {
            candidate.focus(wi, ticket.pane);
        }
        candidate.state.revision = candidate
            .state
            .revision
            .checked_add(1)
            .ok_or(ActionError::Exhausted)?;
        candidate.local_zellij_reservations.remove(&ticket.surface);
        candidate.check_window_layout()?;
        candidate.validate_terminal_descriptors()?;
        candidate.validate_todos()?;
        candidate.record_focus(prior, prior_window, false, focus);
        Ok(PreparedLocalZellijSurface { store: candidate })
    }
    pub(crate) fn validate_terminal_descriptors(&self) -> Result<(), ActionError> {
        for workspace in &self.state.workspaces {
            for pane in workspace.root.panes() {
                for surface in &pane.surfaces {
                    surface.validate_terminal_descriptor()?;
                }
            }
        }
        Ok(())
    }
}
impl Surface {
    pub(crate) fn validate_terminal_descriptor(&self) -> Result<(), ActionError> {
        let count = usize::from(self.remote.is_some())
            + usize::from(self.local_tmux.is_some())
            + usize::from(self.local_zellij.is_some());
        if count > 1 || (self.kind == SurfaceKind::Browser && count != 0) {
            return Err(ActionError::InvalidOperation("terminal cannot combine SSH, local tmux or local Zellij identities, and browsers cannot carry terminal identity".into()));
        }
        if let Some(identity) = &self.local_zellij {
            identity.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_rejects_mixed_or_browser_terminal_identity_instead_of_choosing_one() {
        for browser in [false, true] {
            let mut store = WorkspaceStore::with_identity_namespace(Uuid::from_u128(99));
            store
                .apply(Action::CreateWorkspace {
                    name: "invalid".into(),
                })
                .unwrap();
            let pane = store.state.workspaces[0].focused_pane;
            let surface = &mut store.state.workspaces[0]
                .root
                .pane_mut(pane)
                .unwrap()
                .surfaces[0];
            surface.local_zellij = Some(LocalZellijIdentity {
                profile_id: Uuid::from_u128(1),
                owner_uuid: Uuid::from_u128(2),
            });
            if browser {
                surface.kind = SurfaceKind::Browser;
            } else {
                surface.remote = Some(
                    serde_json::from_value(serde_json::json!({"destination":"host"})).unwrap(),
                );
            }
            assert!(
                store
                    .session_document()
                    .unwrap_err()
                    .to_string()
                    .contains("cannot combine"),
                "mixed identities must never be silently discarded during capture"
            );
        }
    }
}
