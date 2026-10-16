//! Session transactions and bounded receipts. Filesystem work belongs to `persistence`.
use crate::persistence::{PersistenceError, PersistenceOutcome, PersistenceWorker};
use crate::{DesktopError, Session, SessionError, SessionId, Worker, WorkspaceStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use zmux_workspace_core::{Action, RestoreLimits, SessionDocument};

const RECEIPTS: usize = 32;
const RESTORE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionCommand {
    Export {
        path: PathBuf,
        #[serde(default)]
        force: bool,
    },
    Restore {
        #[serde(default)]
        path: Option<PathBuf>,
    },
    Save,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Completed,
    Failed,
}
#[derive(Clone, Debug, Serialize)]
pub struct SessionOperation {
    pub operation_id: u64,
    pub kind: String,
    pub state: OperationState,
    pub error: Option<String>,
    pub revision: Option<u64>,
}
#[derive(Clone, Default, Serialize)]
pub struct PersistenceSnapshot {
    pub enabled: bool,
    pub startup_pending: bool,
    pub restoring: bool,
    pub autosave_enabled: bool,
    pub last_saved_revision: Option<u64>,
    pub error: Option<String>,
    pub operations: Vec<SessionOperation>,
}
#[derive(Default)]
struct IoOperation {
    reservation: bool,
    starter: bool,
    receipt: Option<u64>,
    startup: bool,
    save_revision: Option<u64>,
    authoritative: bool,
    checkpoint: Option<CheckpointOperation>,
    completion: Option<CompletionOperation>,
}
struct CheckpointOperation {
    seal: crate::zellij_coordinator::CheckpointSeal,
    document: SessionDocument,
}
struct CompletionOperation {
    seal: crate::zellij_coordinator::CompletionSeal,
    document: SessionDocument,
}
pub(crate) struct SessionPersistence {
    io: PersistenceWorker,
    path: PathBuf,
    pub snapshot: PersistenceSnapshot,
    next_receipt: u64,
    pending: HashMap<u64, IoOperation>,
    committed: Option<(u64, SessionDocument)>,
    saved_document: Option<SessionDocument>,
    autosave_inflight: bool,
    checkpoint_inflight: Option<u64>,
    checkpoint_lineage: bool,
    allocation_end: u64,
    reservation_pending: bool,
    allocation_failed: bool,
}
impl SessionPersistence {
    pub fn start(directory: PathBuf) -> Result<Self, DesktopError> {
        let io = PersistenceWorker::start(
            directory.clone(),
            RestoreLimits {
                terminals: crate::MAX_TERMINALS,
                browsers: crate::MAX_BROWSERS,
                files: crate::MAX_FILE_SURFACES,
            },
        )
        .map_err(failure)?;
        let path = directory.join("session.json");
        let id = io.submit_load(path.clone()).map_err(failure)?;
        Ok(Self {
            io,
            path,
            snapshot: PersistenceSnapshot {
                enabled: true,
                startup_pending: true,
                autosave_enabled: true,
                ..Default::default()
            },
            next_receipt: 1,
            pending: HashMap::from([(
                id.0,
                IoOperation {
                    receipt: None,
                    startup: true,
                    save_revision: None,
                    authoritative: false,
                    ..Default::default()
                },
            )]),
            committed: None,
            saved_document: None,
            autosave_inflight: false,
            checkpoint_inflight: None,
            checkpoint_lineage: false,
            allocation_end: 0,
            reservation_pending: false,
            allocation_failed: false,
        })
    }
    fn receipt(&mut self, kind: &str) -> Result<u64, DesktopError> {
        if self.snapshot.operations.len() == RECEIPTS {
            let index = self
                .snapshot
                .operations
                .iter()
                .position(|op| !matches!(op.state, OperationState::Pending))
                .ok_or_else(|| failure("session receipt queue is full"))?;
            self.snapshot.operations.remove(index);
        }
        let id = self.next_receipt;
        self.next_receipt = id
            .checked_add(1)
            .ok_or_else(|| failure("session operation IDs exhausted"))?;
        self.snapshot.operations.push(SessionOperation {
            operation_id: id,
            kind: kind.into(),
            state: OperationState::Pending,
            error: None,
            revision: None,
        });
        Ok(id)
    }
    fn finish(&mut self, id: Option<u64>, result: Result<u64, String>) {
        if let Some(op) = self
            .snapshot
            .operations
            .iter_mut()
            .find(|op| Some(op.operation_id) == id)
        {
            match result {
                Ok(revision) => {
                    op.state = OperationState::Completed;
                    op.revision = Some(revision);
                }
                Err(error) => {
                    op.state = OperationState::Failed;
                    op.error = Some(error);
                }
            }
        }
    }
    fn latch(&mut self, error: String) {
        self.snapshot.autosave_enabled = false;
        self.snapshot.error = Some(error);
    }
}
pub(crate) struct PendingRestore {
    pub store: WorkspaceStore,
    allocation_end: u64,
    ids: Vec<SessionId>,
    outstanding: Vec<SessionId>,
    receipt: Option<u64>,
    startup: bool,
    deadline: Instant,
}
fn failure(error: impl ToString) -> DesktopError {
    DesktopError::Failure(error.to_string())
}
impl Worker {
    pub(crate) fn session_transition_pending(&self) -> bool {
        self.restoring.is_some()
            || self.persistence.as_ref().is_some_and(|p| {
                p.snapshot.startup_pending
                    || p.pending
                        .values()
                        .any(|o| o.save_revision.is_none() && !o.reservation)
            })
    }
    fn request_allocator(&mut self, starter: bool) -> Result<(), DesktopError> {
        let namespace = self.model.snapshot().identity_namespace;
        let minimum = self.model.next_identifier();
        let p = self.persistence.as_mut().expect("enabled");
        let id = p.io.submit_reserve(namespace, minimum).map_err(failure)?;
        p.pending.insert(
            id.0,
            IoOperation {
                reservation: true,
                starter,
                ..Default::default()
            },
        );
        p.reservation_pending = true;
        if starter {
            p.snapshot.startup_pending = true;
        }
        Ok(())
    }
    fn prepare_starter(&mut self) {
        if self.shared.stop.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        if let Err(error) = self.request_allocator(true) {
            let p = self.persistence.as_mut().expect("enabled");
            p.snapshot.startup_pending = false;
            p.allocation_failed = true;
            p.latch(error.to_string());
        }
    }
    pub(crate) fn ensure_reserved(&self, candidate: &WorkspaceStore) -> Result<(), DesktopError> {
        if self
            .persistence
            .as_ref()
            .is_some_and(|p| candidate.next_identifier() > p.allocation_end)
        {
            return Err(failure(
                "identifier reservation is pending or unavailable; retry after persistence recovers",
            ));
        }
        Ok(())
    }
    pub(crate) fn starter_workspace(&mut self) {
        if let Err(error) = self.action(Action::CreateWorkspace {
            name: "Terminal".into(),
        }) {
            self.last_error = Some(error.to_string());
        }
    }
    pub(crate) fn launch_model(&self) -> &WorkspaceStore {
        self.restoring
            .as_ref()
            .map(|r| &r.store)
            .unwrap_or(&self.model)
    }
    pub(crate) fn launch_model_mut(&mut self) -> &mut WorkspaceStore {
        self.restoring
            .as_mut()
            .map(|r| &mut r.store)
            .unwrap_or(&mut self.model)
    }
    fn stable_document(&self) -> Result<(u64, SessionDocument), DesktopError> {
        if self.pending_launches != 0
            || self.session_transition_pending()
            || self
                .model
                .snapshot()
                .closed_items
                .iter()
                .any(|item| item.restoring)
        {
            return Err(failure(
                "session topology is still starting; retry after launches complete",
            ));
        }
        let captured = self.model_with_session_metadata();
        Ok((
            captured.snapshot().revision,
            captured.session_document().map_err(failure)?,
        ))
    }
    pub(crate) fn zellij_start_admission(&self) -> Result<(), DesktopError> {
        let p = self
            .persistence
            .as_ref()
            .ok_or_else(|| failure("local Zellij Start requires session persistence"))?;
        if !p.snapshot.autosave_enabled
            || p.checkpoint_inflight.is_some()
            || self.session_transition_pending()
            || self
                .model
                .snapshot()
                .closed_items
                .iter()
                .any(|item| item.restoring)
        {
            return Err(failure(
                "local Zellij checkpoint is unavailable during persistence or restore transition",
            ));
        }
        Ok(())
    }
    /// Pending ordinary launches are canonical only inside this explicit lineage.
    /// Unknown, provisional and mismatched generations always fail closed.
    fn checkpoint_pending_safe(
        &self,
        model: &WorkspaceStore,
        excluded: Option<(SessionId, u64)>,
    ) -> bool {
        use crate::zellij_coordinator::LaunchKind;
        use std::sync::atomic::Ordering;
        if self.session_transition_pending()
            || self.launch_certificates.len() != self.pending_launches
        {
            return false;
        }
        let snapshot = model.snapshot();
        if snapshot.closed_items.iter().any(|item| item.restoring) {
            return false;
        }
        self.launch_certificates.iter().all(|(key, certificate)| {
            if Some(*key) == excluded {
                return true;
            }
            if certificate.namespace != snapshot.identity_namespace || certificate.session != key.0
            {
                return false;
            }
            let surface = snapshot
                .workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .flat_map(|p| &p.surfaces)
                .find(|surface| surface.session_id == Some(key.0));
            let Some(surface) = surface else {
                return certificate.cancellation.load(Ordering::Acquire);
            };
            if snapshot.object_uuid(
                zmux_workspace_core::targets::TargetKind::Surface,
                surface.id.0,
            ) != certificate.surface
                || self
                    .slots
                    .get(&key.0)
                    .is_none_or(|slot| slot.launch_id != key.1)
            {
                return false;
            }
            match &certificate.kind {
                LaunchKind::Ordinary => {
                    surface.remote.is_none()
                        && surface.local_tmux.is_none()
                        && surface.local_zellij.is_none()
                }
                LaunchKind::Ssh => {
                    surface.remote.is_some()
                        && surface.local_tmux.is_none()
                        && surface.local_zellij.is_none()
                }
                LaunchKind::LocalTmux => {
                    surface.remote.is_none()
                        && surface.local_tmux.is_some()
                        && surface.local_zellij.is_none()
                }
                LaunchKind::LocalZellij(Some(identity)) => {
                    surface.remote.is_none()
                        && surface.local_tmux.is_none()
                        && surface.local_zellij.as_ref() == Some(identity)
                }
                LaunchKind::LocalZellij(None) => false,
            }
        })
    }
    fn checkpoint_document(&self) -> Result<(u64, SessionDocument), DesktopError> {
        if let Ok(document) = self.stable_document() {
            return Ok(document);
        }
        if !self
            .persistence
            .as_ref()
            .is_some_and(|p| p.checkpoint_lineage)
            || !self.checkpoint_pending_safe(&self.model, None)
        {
            return Err(failure("session checkpoint topology is provisional"));
        }
        let captured = self.model_with_session_metadata();
        let document = captured.session_document().map_err(failure)?;
        crate::persistence::validate_document_size(&document).map_err(failure)?;
        Ok((captured.snapshot().revision, document))
    }
    pub(crate) fn refresh_checkpoint_baseline(&mut self) {
        if let Ok((revision, document)) = self.checkpoint_document() {
            let strict = self.pending_launches == 0 && !self.session_transition_pending();
            if let Some(p) = &mut self.persistence {
                p.committed = Some((revision, document));
                if strict && p.checkpoint_inflight.is_none() {
                    p.checkpoint_lineage = false;
                }
            }
        }
    }
    pub(crate) fn zellij_owner_reserved(
        &mut self,
        correlation: crate::local_zellij::Correlation,
        owner_uuid: uuid::Uuid,
    ) -> Result<(), DesktopError> {
        use crate::zellij_coordinator::{CheckpointSeal, LaunchKind};
        use std::sync::atomic::Ordering;
        self.zellij_start_admission()?;
        let job = self
            .local_zellij
            .jobs
            .get(&correlation.operation_id)
            .ok_or_else(|| failure("unknown local Zellij reservation"))?;
        if job.correlation != correlation
            || job.installed
            || job.checkpoint.is_some()
            || job.cancel.load(Ordering::Acquire)
            || Instant::now() >= job.deadline
            || self.shared.stop.load(Ordering::Acquire)
        {
            return Err(failure("stale or cancelled local Zellij reservation"));
        }
        let identity = zmux_workspace_core::LocalZellijIdentity {
            profile_id: correlation.profile_id,
            owner_uuid,
        };
        identity.validate().map_err(failure)?;
        if !self
            .launch_certificates
            .get(&(job.session, job.launch_id))
            .is_some_and(|certificate| {
                certificate.namespace == correlation.identity_namespace
                    && certificate.surface == correlation.surface_id
                    && certificate.session == job.session
                    && matches!(certificate.kind, LaunchKind::LocalZellij(None))
                    && std::sync::Arc::ptr_eq(&certificate.cancellation, &job.cancel)
            })
        {
            return Err(failure(
                "local Zellij reservation launch certificate changed",
            ));
        }
        let ticket = job
            .reservation
            .as_ref()
            .ok_or_else(|| failure("local Zellij reservation already consumed"))?;
        let prepared = self
            .model
            .prepare_local_zellij_surface(ticket, identity.clone(), job.focus)
            .map_err(failure)?;
        self.profiles.validate_model(&prepared.store().snapshot())?;
        self.validate_todo_candidate(prepared.store())?;
        if !self.checkpoint_pending_safe(prepared.store(), Some((job.session, job.launch_id))) {
            return Err(failure(
                "local Zellij checkpoint includes an unknown pending launch",
            ));
        }
        let captured = self.capture_session_metadata(prepared.store().clone());
        let document = captured.session_document().map_err(failure)?;
        crate::persistence::validate_document_size(&document).map_err(failure)?;
        let revision = captured.snapshot().revision;
        let mut seal = CheckpointSeal {
            correlation: correlation.clone(),
            session: job.session,
            surface: job.surface,
            launch_id: job.launch_id,
            identity: identity.clone(),
            revision,
            io_id: 0,
        };
        let p = self.persistence.as_mut().expect("admitted persistence");
        let receipt = p.receipt("local_zellij_checkpoint")?;
        let io_id = match p.io.submit_save(p.path.clone(), document.clone(), true) {
            Ok(id) => id.0,
            Err(error) => {
                p.finish(Some(receipt), Err(error.to_string()));
                return Err(failure(error));
            }
        };
        seal.io_id = io_id;
        // Every fallible operation precedes acceptance. This serialized install
        // consumes the prepared ticket without replaying a Start/Attach effect.
        self.model = prepared.into_store();
        let job = self
            .local_zellij
            .jobs
            .get_mut(&correlation.operation_id)
            .expect("validated job");
        job.reservation = None;
        job.identity = Some(identity.clone());
        job.installed = true;
        job.checkpoint = Some(io_id);
        self.slots.insert(
            job.session,
            crate::Slot {
                cancellation: job.cancel.clone(),
                launch_id: job.launch_id,
                session: None,
                connection: None,
                cached: None,
                starting: true,
                failure: None,
                resize: None,
                last_notice: 0,
            },
        );
        self.launch_certificates
            .get_mut(&(job.session, job.launch_id))
            .expect("validated launch certificate")
            .kind = LaunchKind::LocalZellij(Some(identity));
        if let Some(operation) = self
            .local_zellij
            .receipts
            .iter_mut()
            .find(|operation| operation.operation_id == correlation.operation_id)
        {
            operation.owner_uuid = Some(owner_uuid);
        }
        p.committed = Some((revision, document.clone()));
        p.checkpoint_inflight = Some(io_id);
        p.checkpoint_lineage = true;
        p.pending.insert(
            io_id,
            IoOperation {
                receipt: Some(receipt),
                save_revision: Some(revision),
                authoritative: true,
                checkpoint: Some(CheckpointOperation { seal, document }),
                ..Default::default()
            },
        );
        Ok(())
    }
    pub(crate) fn zellij_completion_admission_available(&self) -> bool {
        !self.session_transition_pending()
            && !self
                .model
                .snapshot()
                .closed_items
                .iter()
                .any(|item| item.restoring)
            && self
                .persistence
                .as_ref()
                .is_none_or(|p| p.checkpoint_inflight.is_none())
    }
    pub(crate) fn zellij_workload_finished(
        &mut self,
        pending: crate::zellij_coordinator::PendingCompletion,
    ) -> Result<(), DesktopError> {
        use crate::zellij_coordinator::{CheckpointSeal, CompletionSeal};
        use std::sync::atomic::Ordering;
        let p = self.persistence.as_ref().ok_or_else(|| {
            failure("completed local Zellij history requires session persistence")
        })?;
        if !p.snapshot.autosave_enabled || !self.zellij_completion_admission_available() {
            return Err(failure("completed workload persistence is unavailable"));
        }
        let id = pending.correlation.operation_id;
        let job = self
            .zellij_consumer(id)
            .ok_or_else(|| failure("completed workload consumer disappeared"))?;
        if job.correlation != pending.correlation
            || !self.zellij_job_target_matches(job)
            || job.cancel.load(Ordering::Acquire)
            || Instant::now() >= pending.deadline
            || self.shared.stop.load(Ordering::Acquire)
        {
            return Err(failure(
                "completed workload target or authorization expired",
            ));
        }
        let identity = job
            .identity
            .clone()
            .ok_or_else(|| failure("completed workload has no bound identity"))?;
        let history = zmux_workspace_core::LocalZellijCompletion {
            receipt: pending.receipt.clone(),
            viewport: pending.viewport,
        };
        let candidate = self
            .model
            .prepare_local_zellij_completion(job.surface, job.session, &identity, history)
            .map_err(failure)?;
        self.profiles.validate_model(&candidate.snapshot())?;
        self.validate_todo_candidate(&candidate)?;
        if !self.checkpoint_pending_safe(&candidate, None) {
            return Err(failure(
                "completed workload checkpoint includes provisional launches",
            ));
        }
        let captured = self.capture_session_metadata(candidate.clone());
        let document = captured.session_document().map_err(failure)?;
        crate::persistence::validate_document_size(&document).map_err(failure)?;
        let revision = captured.snapshot().revision;
        let mut seal = CompletionSeal {
            target: CheckpointSeal {
                correlation: job.correlation.clone(),
                session: job.session,
                surface: job.surface,
                launch_id: job.launch_id,
                identity,
                revision,
                io_id: 0,
            },
            deadline: pending.deadline,
            generation: pending.receipt.owner_generation(),
            digest: pending.receipt.digest().map_err(failure)?,
        };
        let p = self.persistence.as_mut().expect("admitted persistence");
        let receipt = p.receipt("local_zellij_completion")?;
        let io_id = match p.io.submit_save(p.path.clone(), document.clone(), true) {
            Ok(id) => id.0,
            Err(error) => {
                p.finish(Some(receipt), Err(error.to_string()));
                return Err(failure(error));
            }
        };
        seal.target.io_id = io_id;
        self.model = candidate;
        p.committed = Some((revision, document.clone()));
        p.checkpoint_inflight = Some(io_id);
        p.checkpoint_lineage = true;
        p.pending.insert(
            io_id,
            IoOperation {
                receipt: Some(receipt),
                save_revision: Some(revision),
                authoritative: true,
                completion: Some(CompletionOperation { seal, document }),
                ..Default::default()
            },
        );
        self.zellij_consumer_mut(id)
            .expect("validated consumer")
            .checkpoint = Some(io_id);
        Ok(())
    }
    pub(crate) fn session_command(&mut self, action: SessionCommand) -> Result<(), DesktopError> {
        if self.persistence.is_none() {
            return Err(failure("session persistence is disabled for this desktop"));
        }
        if self.session_transition_pending() {
            return Err(failure("another session restore is pending"));
        }
        if matches!(&action, SessionCommand::Restore { .. })
            && self
                .persistence
                .as_ref()
                .is_some_and(|p| p.reservation_pending)
        {
            return Err(failure("identifier reservation is pending; retry restore"));
        }
        if matches!(&action, SessionCommand::Restore { .. })
            && (!self.model.snapshot().workspaces.is_empty()
                || self.pending_launches != 0
                || !self.slots.is_empty())
        {
            return Err(failure(
                "restore requires an empty desktop; close every workspace first",
            ));
        }
        let document = if matches!(&action, SessionCommand::Restore { .. }) {
            None
        } else {
            Some(self.stable_document()?)
        };
        let p = self.persistence.as_mut().expect("checked");
        let kind = match &action {
            SessionCommand::Export { .. } => "export",
            SessionCommand::Restore { .. } => "restore",
            SessionCommand::Save => "save",
        };
        let receipt = p.receipt(kind)?;
        let (request, save_revision, authoritative) = match action {
            SessionCommand::Restore { path } => (
                p.io.submit_load(path.unwrap_or_else(|| p.path.clone())),
                None,
                false,
            ),
            SessionCommand::Export { path, force } => {
                if path == p.path {
                    p.finish(
                        Some(receipt),
                        Err("use session save for the recovery file".into()),
                    );
                    return Err(failure("use session save for the recovery file"));
                }
                let (revision, document) = document.expect("save document");
                (
                    p.io.submit_save(path, document, force),
                    Some(revision),
                    false,
                )
            }
            SessionCommand::Save => {
                if !p.snapshot.autosave_enabled {
                    p.finish(Some(receipt), Err("recovery file is protected after a persistence failure; export to another path".into()));
                    return Err(failure(
                        "recovery file is protected after a persistence failure; export to another path",
                    ));
                }
                let (revision, document) = document.expect("save document");
                (
                    p.io.submit_save(p.path.clone(), document, true),
                    Some(revision),
                    true,
                )
            }
        };
        match request {
            Ok(id) => {
                p.pending.insert(
                    id.0,
                    IoOperation {
                        receipt: Some(receipt),
                        startup: false,
                        save_revision,
                        authoritative,
                        ..Default::default()
                    },
                );
                self.session_operation_id = Some(receipt);
                Ok(())
            }
            Err(error) => {
                p.finish(Some(receipt), Err(error.to_string()));
                Err(failure(error))
            }
        }
    }
    pub(crate) fn poll_sessions(&mut self) -> bool {
        let mut dirty = false;
        loop {
            let completion = match self.persistence.as_ref().map(|p| p.io.try_completion()) {
                Some(Ok(Some(c))) => c,
                Some(Err(error)) => {
                    let failed: Vec<_> = self
                        .persistence
                        .as_ref()
                        .into_iter()
                        .flat_map(|p| p.pending.values())
                        .filter_map(|op| {
                            op.checkpoint
                                .as_ref()
                                .map(|checkpoint| checkpoint.seal.correlation.operation_id)
                                .or_else(|| {
                                    op.completion.as_ref().map(|checkpoint| {
                                        checkpoint.seal.target.correlation.operation_id
                                    })
                                })
                        })
                        .collect();
                    for id in failed {
                        self.fail_zellij(
                            id,
                            format!("local Zellij checkpoint completion unavailable: {error}"),
                        );
                    }
                    if let Some(p) = &mut self.persistence {
                        let message = error.to_string();
                        dirty |= p.snapshot.error.as_ref() != Some(&message);
                        p.latch(message);
                    }
                    break;
                }
                _ => break,
            };
            dirty = true;
            let op = self
                .persistence
                .as_mut()
                .expect("enabled")
                .pending
                .remove(&completion.operation_id.0);
            let Some(op) = op else {
                continue;
            };
            if (op.checkpoint.is_some() || op.completion.is_some())
                && !matches!(&completion.result, Ok(PersistenceOutcome::Saved) | Err(_))
            {
                self.load_failed(
                    op,
                    "unexpected completion for local Zellij authoritative checkpoint".into(),
                );
                continue;
            }
            match completion.result {
                Ok(PersistenceOutcome::Profiles(_)) => {
                    self.last_error =
                        Some("unexpected profile completion on session worker".into());
                }
                Ok(PersistenceOutcome::Reserved(reservation)) => {
                    let result = self.model.advance_identifier_floor(reservation.start);
                    let p = self.persistence.as_mut().expect("enabled");
                    p.reservation_pending = false;
                    match result {
                        Ok(()) => {
                            p.allocation_end = reservation.end;
                            if op.starter {
                                p.snapshot.startup_pending = false;
                                self.starter_workspace();
                            }
                        }
                        Err(error) => {
                            p.allocation_failed = true;
                            p.snapshot.startup_pending = false;
                            p.latch(error.to_string());
                        }
                    }
                }
                Err(error) if op.reservation => {
                    let p = self.persistence.as_mut().expect("enabled");
                    p.reservation_pending = false;
                    p.allocation_failed = true;
                    p.snapshot.startup_pending = false;
                    p.latch(format!("reserve durable identifiers: {error}"));
                }
                Ok(PersistenceOutcome::Saved) => {
                    let (reservation, completed) = {
                        let p = self.persistence.as_mut().expect("enabled");
                        p.finish(op.receipt, Ok(op.save_revision.unwrap_or(0)));
                        if op.authoritative {
                            p.snapshot.last_saved_revision = op.save_revision;
                            p.autosave_inflight = false;
                        }
                        if p.checkpoint_inflight == Some(completion.operation_id.0) {
                            p.checkpoint_inflight = None;
                            if let Some(checkpoint) = op.checkpoint {
                                p.saved_document = Some(checkpoint.document);
                                (Some(checkpoint.seal), None)
                            } else if let Some(checkpoint) = op.completion {
                                p.saved_document = Some(checkpoint.document);
                                (None, Some(checkpoint.seal))
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        }
                    };
                    if let Some(seal) = reservation {
                        self.authorize_zellij_checkpoint(seal);
                    }
                    if let Some(seal) = completed {
                        self.authorize_zellij_completion(seal);
                    }
                }
                Ok(PersistenceOutcome::Loaded(loaded)) => {
                    let owned = self.pending_launches
                        + self.retired.len()
                        + self
                            .slots
                            .values()
                            .filter(|slot| slot.session.is_some())
                            .count();
                    if owned + loaded.session_ids.len() > crate::MAX_TERMINALS {
                        self.load_failed(op, "restore exceeds terminal capacity while prior sessions are cleaning up; retry".into());
                        continue;
                    }
                    let prepared = WorkspaceStore::prepare_session_restore(
                        loaded.document,
                        &loaded.session_ids,
                        loaded.window_id,
                    );
                    match prepared {
                        Ok(mut prepared) => {
                            if let Err(error) = self.validate_launch_effects(&prepared.effects) {
                                self.load_failed(op, error.to_string());
                                continue;
                            }
                            if let Err(error) =
                                self.profiles.validate_model(&prepared.store.snapshot())
                            {
                                self.load_failed(op, error.to_string());
                                continue;
                            }
                            if let Err(error) = self.validate_restored_todos(&prepared.store) {
                                self.load_failed(op, error.to_string());
                                continue;
                            }
                            if let Err(error) = prepared
                                .store
                                .advance_identifier_floor(loaded.allocation_start)
                                .and_then(|()| {
                                    prepared
                                        .store
                                        .advance_restore_revision(self.model.snapshot().revision)
                                })
                            {
                                self.load_failed(op, error.to_string());
                                continue;
                            }
                            self.restoring = Some(PendingRestore {
                                store: prepared.store,
                                allocation_end: loaded.allocation_end,
                                ids: loaded.session_ids.clone(),
                                outstanding: loaded.session_ids,
                                receipt: op.receipt,
                                startup: op.startup,
                                deadline: Instant::now() + RESTORE_TIMEOUT,
                            });
                            self.persistence
                                .as_mut()
                                .expect("enabled")
                                .snapshot
                                .restoring = true;
                            self.effects(prepared.effects);
                            self.commit_session_restore();
                        }
                        Err(error) => self.load_failed(op, error.to_string()),
                    }
                }
                Err(PersistenceError::NotFound) if op.startup => {
                    self.prepare_starter();
                }
                Err(error) => self.load_failed(op, error.to_string()),
            }
        }
        if self
            .restoring
            .as_ref()
            .is_some_and(|r| Instant::now() >= r.deadline)
        {
            self.fail_session_restore("session restore launch deadline elapsed".into());
            dirty = true;
        }
        // Store only a fully committed topology. PTY frames and transient undo-close
        // candidates never become an authoritative shutdown/autosave snapshot.
        if let Ok((revision, document)) = self.checkpoint_document()
            && let Some(p) = &mut self.persistence
        {
            p.committed = Some((revision, document.clone()));
            if p.snapshot.autosave_enabled
                && !p.autosave_inflight
                && p.checkpoint_inflight.is_none()
                && p.saved_document.as_ref() != Some(&document)
            {
                match p.io.submit_save(p.path.clone(), document.clone(), true) {
                    Ok(id) => {
                        p.saved_document = Some(document);
                        p.autosave_inflight = true;
                        p.pending.insert(
                            id.0,
                            IoOperation {
                                receipt: None,
                                startup: false,
                                save_revision: Some(revision),
                                authoritative: true,
                                ..Default::default()
                            },
                        );
                    }
                    Err(PersistenceError::QueueFull) => {}
                    Err(error) => {
                        p.latch(error.to_string());
                        dirty = true;
                    }
                }
            }
        }
        if !self.session_transition_pending()
            && self.persistence.as_ref().is_some_and(|p| {
                !p.reservation_pending
                    && !p.allocation_failed
                    && p.allocation_end
                        .saturating_sub(self.model.next_identifier())
                        < 128
            })
            && let Err(error) = self.request_allocator(false)
        {
            let p = self.persistence.as_mut().expect("enabled");
            p.allocation_failed = true;
            p.latch(error.to_string());
            dirty = true;
        }
        dirty
    }
    fn load_failed(&mut self, op: IoOperation, error: String) {
        if let Some(completion) = &op.completion {
            self.fail_zellij(
                completion.seal.target.correlation.operation_id,
                format!("completed workload checkpoint failed: {error}"),
            );
        }
        if let Some(checkpoint) = &op.checkpoint {
            self.fail_zellij(
                checkpoint.seal.correlation.operation_id,
                format!("local Zellij checkpoint failed: {error}"),
            );
        }
        let p = self.persistence.as_mut().expect("enabled");
        p.finish(op.receipt, Err(error.clone()));
        if op.startup || op.authoritative {
            p.latch(error.clone());
        } else {
            p.snapshot.error = Some(error.clone());
        }
        if op.authoritative {
            p.autosave_inflight = false;
        }
        if op.checkpoint.is_some() || op.completion.is_some() {
            p.checkpoint_inflight = None;
        }
        if op.startup {
            self.prepare_starter();
        }
        self.last_error = Some(error);
    }
    pub(crate) fn complete_session_restore(
        &mut self,
        id: SessionId,
        result: &Result<Session, SessionError>,
    ) -> bool {
        let Some(restore) = &mut self.restoring else {
            return false;
        };
        if !restore.ids.contains(&id) {
            return false;
        }
        restore.outstanding.retain(|pending| *pending != id);
        if let Err(error) = result {
            self.fail_session_restore(format!("session restore launch failed: {error}"));
        }
        true
    }
    pub(crate) fn commit_session_restore(&mut self) {
        if !self
            .restoring
            .as_ref()
            .is_some_and(|r| r.outstanding.is_empty())
        {
            return;
        }
        let restore = self.restoring.take().expect("ready");
        self.model = restore.store;
        self.metadata = crate::metadata::MetadataStore::default();
        let p = self.persistence.as_mut().expect("enabled");
        p.snapshot.restoring = false;
        p.snapshot.startup_pending = false;
        p.allocation_end = restore.allocation_end;
        p.finish(restore.receipt, Ok(self.model.snapshot().revision));
    }
    pub(crate) fn fail_session_restore(&mut self, error: String) {
        let Some(restore) = self.restoring.take() else {
            return;
        };
        for id in restore.ids {
            if let Some(mut slot) = self.slots.remove(&id)
                && let Some(session) = slot.session.take()
            {
                self.retire_owned(session, id, slot.launch_id);
            }
        }
        let p = self.persistence.as_mut().expect("enabled");
        p.snapshot.restoring = false;
        p.snapshot.startup_pending = false;
        p.finish(restore.receipt, Err(error.clone()));
        if restore.startup {
            p.latch(error.clone());
            self.prepare_starter();
        } else {
            p.snapshot.error = Some(error.clone());
        }
        self.last_error = Some(error);
    }
    pub(crate) fn prepare_shutdown_persistence(&mut self) {
        if self.restoring.is_some() {
            self.fail_session_restore("session restore cancelled by shutdown".into());
        }
        if let Ok((revision, document)) = self.checkpoint_document()
            && let Some(p) = &mut self.persistence
        {
            p.committed = Some((revision, document));
        }
    }
    pub(crate) fn poll_shutdown_persistence(
        &mut self,
        submitted: &mut bool,
        deadline: Instant,
    ) -> bool {
        let Some(p) = &mut self.persistence else {
            return true;
        };
        // Drain only ready receipts, leaving the executor available for native
        // download cleanup acknowledgments during a slow filesystem operation.
        while !p.pending.is_empty() {
            match p.io.try_completion() {
                Ok(Some(completion)) => {
                    let Some(op) = p.pending.remove(&completion.operation_id.0) else {
                        continue;
                    };
                    match completion.result {
                        Ok(PersistenceOutcome::Saved) => {
                            p.finish(op.receipt, Ok(op.save_revision.unwrap_or(0)));
                            if op.authoritative {
                                p.snapshot.last_saved_revision = op.save_revision;
                            }
                        }
                        Ok(PersistenceOutcome::Reserved(_) | PersistenceOutcome::Profiles(_)) => (),
                        Ok(PersistenceOutcome::Loaded(_)) => {
                            p.finish(op.receipt, Err("restore cancelled by shutdown".into()))
                        }
                        Err(error) => {
                            p.finish(op.receipt, Err(error.to_string()));
                            if op.authoritative {
                                let message = format!("session shutdown save failed: {error}");
                                self.cleanup_error = Some(message.clone());
                                p.latch(message);
                            }
                        }
                    }
                }
                Ok(None) => return false,
                Err(error) => {
                    let message = format!("session shutdown completion unavailable: {error}");
                    self.cleanup_error = Some(message.clone());
                    p.latch(message);
                    let receipts: Vec<_> =
                        p.pending.drain().filter_map(|(_, op)| op.receipt).collect();
                    for receipt in receipts {
                        p.finish(Some(receipt), Err(error.to_string()));
                    }
                    break;
                }
            }
        }
        if !*submitted {
            *submitted = true;
            if Instant::now() >= deadline && p.snapshot.autosave_enabled {
                let message =
                    "session final flush deadline elapsed before submission; outcome is unknown"
                        .to_string();
                self.cleanup_error = Some(message.clone());
                p.latch(message);
            }
            if p.snapshot.autosave_enabled
                && let Some((revision, document)) = p.committed.clone()
            {
                match p.io.submit_save(p.path.clone(), document, true) {
                    Ok(id) => {
                        p.pending.insert(
                            id.0,
                            IoOperation {
                                receipt: None,
                                save_revision: Some(revision),
                                authoritative: true,
                                ..Default::default()
                            },
                        );
                        return false;
                    }
                    Err(error) => {
                        let message = format!("session final save was not accepted: {error}");
                        self.cleanup_error = Some(message.clone());
                        p.latch(message);
                    }
                }
            }
        }
        p.io.request_shutdown();
        if !p.io.is_finished() {
            return false;
        }
        if let Err(error) = p.io.join_until(Instant::now()) {
            self.cleanup_error = Some(error.to_string());
            p.latch(error.to_string());
        }
        p.snapshot.startup_pending = false;
        p.snapshot.restoring = false;
        p.reservation_pending = false;
        true
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
