//! Session transactions and bounded receipts. Filesystem work belongs to `persistence`.
use crate::persistence::{PersistenceError, PersistenceOutcome, PersistenceWorker};
use crate::{DesktopError, Session, SessionError, SessionId, Worker, WorkspaceStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    thread,
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
        if self.pending_launches != 0 || self.session_transition_pending() {
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
            match completion.result {
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
                    let p = self.persistence.as_mut().expect("enabled");
                    p.finish(op.receipt, Ok(op.save_revision.unwrap_or(0)));
                    if op.authoritative {
                        p.snapshot.last_saved_revision = op.save_revision;
                        p.autosave_inflight = false;
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
                    );
                    match prepared {
                        Ok(mut prepared) => {
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
        if let Ok((revision, document)) = self.stable_document()
            && let Some(p) = &mut self.persistence
        {
            p.committed = Some((revision, document.clone()));
            if p.snapshot.autosave_enabled
                && !p.autosave_inflight
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
                self.retire(session);
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
    pub(crate) fn shutdown_persistence(&mut self, deadline: Instant) {
        // The command loop has ended; bounded filesystem waits are cleanup work.
        if self.restoring.is_some() {
            self.fail_session_restore("session restore cancelled by shutdown".into());
        }
        if let Ok((revision, document)) = self.stable_document()
            && let Some(p) = &mut self.persistence
        {
            p.committed = Some((revision, document));
        }
        let Some(p) = &mut self.persistence else {
            return;
        };
        // Finish older writes before the final snapshot, including explicit exports
        // while recovery autosaving is latched off after a corrupt startup file.
        if let Err(error) = p.drain_shutdown(deadline) {
            self.cleanup_error = Some(error);
        }
        if p.snapshot.autosave_enabled
            && Instant::now() < deadline
            && let Some((revision, document)) = p.committed.clone()
        {
            match p.io.submit_save(p.path.clone(), document, true) {
                Ok(id) => {
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
                    if let Err(error) = p.drain_shutdown(deadline) {
                        self.cleanup_error = Some(error);
                    }
                }
                Err(error) => {
                    let message = format!("session final save was not accepted: {error}");
                    p.latch(message.clone());
                    self.cleanup_error = Some(message);
                }
            }
        } else if p.snapshot.autosave_enabled && p.committed.is_some() && Instant::now() >= deadline
        {
            let error = "session final flush deadline elapsed before submission".to_string();
            p.latch(error.clone());
            self.cleanup_error = Some(error);
        }
        if !p.pending.is_empty() {
            let error = "session shutdown deadline elapsed before completion was confirmed; outcome is unknown".to_string();
            let receipts: Vec<_> = p.pending.drain().filter_map(|(_, op)| op.receipt).collect();
            for receipt in receipts {
                p.finish(Some(receipt), Err(error.clone()));
            }
            p.latch(error.clone());
            self.cleanup_error = Some(error);
        }
        if let Err(error) = p.io.join_until(deadline) {
            self.cleanup_error = Some(error.to_string());
        }
        p.snapshot.startup_pending = false;
        p.snapshot.restoring = false;
        p.reservation_pending = false;
    }
}
impl SessionPersistence {
    fn drain_shutdown(&mut self, deadline: Instant) -> Result<(), String> {
        let mut authoritative_failure = None;
        while !self.pending.is_empty() && Instant::now() < deadline {
            match self.io.try_completion() {
                Ok(Some(completion)) => {
                    let Some(op) = self.pending.remove(&completion.operation_id.0) else {
                        continue;
                    };
                    match completion.result {
                        Ok(PersistenceOutcome::Saved) => {
                            self.finish(op.receipt, Ok(op.save_revision.unwrap_or(0)));
                            if op.authoritative {
                                self.snapshot.last_saved_revision = op.save_revision;
                            }
                        }
                        Ok(PersistenceOutcome::Reserved(_)) => {}
                        Ok(PersistenceOutcome::Loaded(_)) => {
                            self.finish(op.receipt, Err("restore cancelled by shutdown".into()))
                        }
                        Err(error) => {
                            self.finish(op.receipt, Err(error.to_string()));
                            if op.authoritative {
                                let message = format!("session shutdown save failed: {error}");
                                self.latch(message.clone());
                                authoritative_failure = Some(message);
                            }
                        }
                    }
                }
                Ok(None) => thread::sleep(Duration::from_millis(2)),
                Err(error) => {
                    let message = format!("session shutdown completion unavailable: {error}");
                    self.latch(message.clone());
                    return Err(message);
                }
            }
        }
        authoritative_failure.map_or(Ok(()), Err)
    }
}
