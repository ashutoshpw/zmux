//! Serialized local-Zellij operation ownership. Filesystem/process/observer I/O
//! remains on the existing S01 and foreground launch workers.
use crate::{
    Completion, DesktopError, SessionError, SessionId, Slot, Worker,
    local_zellij::{self, CheckpointSaved, Correlation, LaunchEvent, LaunchOperation},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_workspace_core::{
    LocalZellijIdentity, LocalZellijReservation, PaneId, SurfaceId, targets::TargetKind,
};
const MAX_OPERATIONS: usize = 8;
const MAX_RECEIPTS: usize = 32;
const STARTUP: Duration = Duration::from_secs(10);
fn default_focus() -> bool {
    true
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Start {
        profile_id: Uuid,
        pane_id: PaneId,
        name: String,
        cwd: PathBuf,
        #[serde(default)]
        command: Option<String>,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    Attach {
        profile_id: Uuid,
        owner_uuid: Uuid,
        pane_id: PaneId,
        #[serde(default = "default_focus")]
        focus: bool,
    },
    Cancel {
        operation_id: u64,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub epoch: Uuid,
    pub operation_id: u64,
    pub state: String,
    pub profile_id: Uuid,
    pub owner_uuid: Option<Uuid>,
    pub surface: Uuid,
    pub checkpoint_revision: Option<u64>,
    pub frontend_pid: Option<u32>,
    pub error: Option<String>,
    pub final_frame_received: bool,
    pub monitor_joined: bool,
    pub namespace_cleaned: bool,
    pub session_joined: bool,
    pub frontend_exit_code: Option<u32>,
}
pub(crate) enum LaunchKind {
    Ordinary,
    Ssh,
    LocalTmux,
    LocalZellij(Option<LocalZellijIdentity>),
}
pub(crate) struct LaunchCertificate {
    pub namespace: Uuid,
    pub surface: Uuid,
    pub session: SessionId,
    pub kind: LaunchKind,
    pub cancellation: Arc<AtomicBool>,
}
pub(crate) struct Job {
    pub correlation: Correlation,
    pub session: SessionId,
    pub surface: SurfaceId,
    pub launch_id: u64,
    pub cancel: Arc<AtomicBool>,
    pub deadline: Instant,
    pub reservation: Option<LocalZellijReservation>,
    pub identity: Option<LocalZellijIdentity>,
    pub installed: bool,
    pub checkpoint: Option<u64>,
    pub focus: bool,
    pub(crate) authorization: SyncSender<CheckpointSaved>,
}
#[derive(Clone)]
pub(crate) struct CheckpointSeal {
    pub correlation: Correlation,
    pub session: SessionId,
    pub surface: SurfaceId,
    pub launch_id: u64,
    pub identity: LocalZellijIdentity,
    pub revision: u64,
    pub io_id: u64,
}
#[derive(Clone)]
pub(crate) struct ZellijTag {
    pub correlation: Correlation,
    pub session: SessionId,
    pub launch_id: u64,
}
pub(crate) struct ZellijCompletion {
    pub tag: ZellijTag,
    pub observer: Option<local_zellij::ObserverHandle>,
    pub owner_uuid: Option<Uuid>,
    pub frontend_pid: Option<u32>,
}
pub(crate) struct Runtime {
    pub configuration: Option<local_zellij::Configuration>,
    pub epoch: Uuid,
    pub jobs: HashMap<u64, Job>,
    pub receipts: Vec<Receipt>,
    next: u64,
    events_tx: SyncSender<LaunchEvent>,
    events_rx: Receiver<LaunchEvent>,
    observations: HashMap<u64, local_zellij::ObserverHandle>,
    tags: HashMap<(SessionId, u64), ZellijTag>,
}
impl Runtime {
    pub fn new(configuration: Option<local_zellij::Configuration>) -> Self {
        let (events_tx, events_rx) = mpsc::sync_channel(MAX_OPERATIONS);
        Self {
            epoch: Uuid::new_v4(),
            configuration,
            jobs: HashMap::new(),
            receipts: Vec::new(),
            next: 1,
            events_tx,
            events_rx,
            observations: HashMap::new(),
            tags: HashMap::new(),
        }
    }
    fn receipt_mut(&mut self, id: u64) -> Option<&mut Receipt> {
        self.receipts.iter_mut().find(|r| r.operation_id == id)
    }
    pub(crate) fn cancel_all(&self) {
        for job in self.jobs.values() {
            job.cancel.store(true, Ordering::Release);
        }
    }
}
impl Worker {
    pub(crate) fn zellij_profile(
        &self,
        id: Uuid,
    ) -> Result<(local_zellij::Installation, local_zellij::Profile), DesktopError> {
        let configuration = self
            .local_zellij
            .configuration
            .as_ref()
            .ok_or_else(|| failure("local Zellij installation is not configured"))?;
        let profile = configuration
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| failure("local Zellij installation profile is unavailable"))?;
        Ok((configuration.installation.clone(), profile.clone()))
    }
    pub(crate) fn local_zellij_command(&mut self, request: Request) -> Result<(), DesktopError> {
        if let Request::Cancel { operation_id } = request {
            let job = self
                .local_zellij
                .jobs
                .get(&operation_id)
                .ok_or_else(|| failure("local Zellij operation is not pending"))?;
            job.cancel.store(true, Ordering::Release);
            return Ok(());
        }
        let (profile_id, pane_id, operation, focus) = match request {
            Request::Start {
                profile_id,
                pane_id,
                name,
                cwd,
                command,
                focus,
            } => {
                self.zellij_start_admission()?;
                (
                    profile_id,
                    pane_id,
                    LaunchOperation::Start { name, cwd, command },
                    focus,
                )
            }
            Request::Attach {
                profile_id,
                owner_uuid,
                pane_id,
                focus,
            } => {
                let identity = LocalZellijIdentity {
                    profile_id,
                    owner_uuid,
                };
                identity.validate().map_err(failure)?;
                (
                    profile_id,
                    pane_id,
                    LaunchOperation::Attach { identity },
                    focus,
                )
            }
            Request::Cancel { .. } => unreachable!(),
        };
        self.zellij_profile(profile_id)?;
        if self.session_transition_pending() {
            return Err(failure(
                "local Zellij launch cannot overlap session restore",
            ));
        }
        self.zellij_capacity()?;
        let mut candidate = self.model.clone();
        let reservation = candidate
            .reserve_local_zellij_surface(pane_id)
            .map_err(failure)?;
        self.ensure_reserved(&candidate)?;
        let session = reservation.session_id();
        let surface = reservation.surface_id();
        let identity = match &operation {
            LaunchOperation::Attach { identity } => Some(identity.clone()),
            _ => None,
        };
        // Burn allocator IDs before the worker can spawn. The attach candidate is
        // fully validated now and published only after worker admission succeeds.
        self.model = candidate;
        let prepared = if let Some(identity) = &identity {
            match self
                .model
                .prepare_local_zellij_surface(&reservation, identity.clone(), focus)
            {
                Ok(prepared) => {
                    if let Err(error) = self.validate_todo_candidate(prepared.store()) {
                        self.model
                            .cancel_local_zellij_reservation(reservation)
                            .map_err(failure)?;
                        return Err(error);
                    }
                    Some(prepared)
                }
                Err(error) => {
                    self.model
                        .cancel_local_zellij_reservation(reservation)
                        .map_err(failure)?;
                    return Err(failure(error));
                }
            }
        } else {
            None
        };
        let spawned =
            self.spawn_zellij_job(profile_id, session, surface, pane_id, operation, focus);
        match spawned {
            Ok(id) => {
                if let Some(prepared) = prepared {
                    self.model = prepared.into_store();
                } else {
                    let job = self.local_zellij.jobs.get_mut(&id).expect("admitted job");
                    job.installed = false;
                    job.reservation = Some(reservation);
                }
                Ok(())
            }
            Err(error) => {
                self.model
                    .cancel_local_zellij_reservation(reservation)
                    .map_err(failure)?;
                Err(error)
            }
        }
    }
    fn zellij_capacity(&self) -> Result<(), DesktopError> {
        if self.local_zellij.jobs.len() >= MAX_OPERATIONS
            || self.pending_launches
                + self.retired.len()
                + self.local_tmux.reserved.len()
                + self
                    .slots
                    .values()
                    .filter(|slot| slot.session.is_some())
                    .count()
                >= crate::MAX_TERMINALS
        {
            return Err(failure(
                "local Zellij launch exceeds retained terminal capacity",
            ));
        }
        Ok(())
    }
    pub(crate) fn launch_zellij_attach(
        &mut self,
        pane: PaneId,
        session: SessionId,
        identity: LocalZellijIdentity,
    ) {
        let surface = self
            .launch_model()
            .snapshot()
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|s| s.session_id == Some(session))
            .map(|s| s.id);
        let result = surface
            .ok_or_else(|| failure("local Zellij attach target disappeared"))
            .and_then(|surface| {
                self.spawn_zellij_job(
                    identity.profile_id,
                    session,
                    surface,
                    pane,
                    LaunchOperation::Attach {
                        identity: identity.clone(),
                    },
                    true,
                )
                .map(|_| ())
            });
        if let Err(error) = result {
            self.slots.insert(
                session,
                Slot {
                    cancellation: Arc::new(AtomicBool::new(true)),
                    launch_id: self.next_launch_id,
                    session: None,
                    connection: None,
                    cached: None,
                    starting: false,
                    failure: Some(error.to_string()),
                    resize: None,
                    last_notice: 0,
                },
            );
            let result = Err(SessionError::Failure(error.to_string()));
            if !self.complete_session_restore(session, &result) {
                let cleanup = self.model.finish_restore(session, false);
                self.effects(cleanup);
            }
            self.last_error = Some(error.to_string());
        }
    }
    fn spawn_zellij_job(
        &mut self,
        profile_id: Uuid,
        session: SessionId,
        surface: SurfaceId,
        pane: PaneId,
        operation: LaunchOperation,
        focus: bool,
    ) -> Result<u64, DesktopError> {
        let identity = match &operation {
            LaunchOperation::Attach { identity } => Some(identity.clone()),
            LaunchOperation::Start { .. } => None,
        };
        let (installation, profile) = self.zellij_profile(profile_id)?;
        if self.local_zellij.receipts.len() == MAX_RECEIPTS {
            let index = self
                .local_zellij
                .receipts
                .iter()
                .position(|receipt| {
                    receipt.session_joined
                        && !self.local_zellij.jobs.contains_key(&receipt.operation_id)
                })
                .ok_or_else(|| failure("local Zellij receipt queue is full"))?;
            let old = self.local_zellij.receipts.remove(index);
            self.local_zellij.observations.remove(&old.operation_id);
            self.local_zellij
                .tags
                .retain(|_, tag| tag.correlation.operation_id != old.operation_id);
        }
        let id = self.local_zellij.next;
        let next = id
            .checked_add(1)
            .ok_or_else(|| failure("local Zellij operation IDs exhausted"))?;
        let launch_id = self.next_launch_id;
        let next_launch = launch_id
            .checked_add(1)
            .ok_or_else(|| failure("launch generation exhausted"))?;
        let model = self.launch_model().snapshot();
        let correlation = Correlation {
            installation_epoch: installation.epoch,
            profile_id,
            operation_id: id,
            identity_namespace: model.identity_namespace,
            surface_id: model.object_uuid(TargetKind::Surface, surface.0),
            session_id: Uuid::new_v5(
                &model.identity_namespace,
                format!("session:{}", session.0).as_bytes(),
            ),
            launch_generation: launch_id,
        };
        correlation.validate().map_err(failure)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + STARTUP;
        let (authorization, receiver) = mpsc::sync_channel(1);
        let events = self.local_zellij.events_tx.clone();
        let sender = self.completion_tx.clone();
        let workspace = model
            .workspaces
            .iter()
            .find(|w| w.root.panes().iter().any(|p| p.id == pane))
            .ok_or_else(|| failure("local Zellij pane disappeared"))?;
        let environment = vec![
            (
                "ZMUX_WORKSPACE_ID".into(),
                model
                    .object_uuid(TargetKind::Workspace, workspace.id.0)
                    .to_string(),
            ),
            ("ZMUX_SURFACE_ID".into(), correlation.surface_id.to_string()),
            ("ZMUX_TAB_ID".into(), correlation.surface_id.to_string()),
        ];
        let installed = matches!(&operation, LaunchOperation::Attach { .. });
        let launch = local_zellij::LaunchRequest {
            correlation: correlation.clone(),
            installation,
            profile,
            operation,
            startup_deadline: deadline,
            cancellation: cancel.clone(),
            engine: self.terminal_engine,
            environment,
        };
        let tag = ZellijTag {
            correlation: correlation.clone(),
            session,
            launch_id,
        };
        let completion_tag = tag.clone();
        let thread = thread::Builder::new()
            .name(format!("zmux-zellij-{id}"))
            .spawn(move || {
                let (result, cleanup, metadata) =
                    match local_zellij::launch_client(launch, events, receiver) {
                        Ok(ready) => (
                            Ok(ready.session),
                            None,
                            ZellijCompletion {
                                tag: completion_tag,
                                observer: Some(ready.observer),
                                owner_uuid: Some(ready.record.id),
                                frontend_pid: Some(ready.frontend.pid),
                            },
                        ),
                        Err(failure) => (
                            Err(failure.error),
                            failure.cleanup,
                            ZellijCompletion {
                                tag: completion_tag,
                                observer: failure.observer,
                                owner_uuid: None,
                                frontend_pid: None,
                            },
                        ),
                    };
                if let Err(error) = sender.send(Completion {
                    launch_id,
                    id: session,
                    result,
                    connection: None,
                    cleanup,
                    zellij: Some(metadata),
                }) {
                    drop(error.0);
                }
            })
            .map_err(failure)?;
        self.local_zellij.tags.insert((session, launch_id), tag);
        self.local_zellij.next = next;
        self.next_launch_id = next_launch;
        self.launch_certificates.insert(
            (session, launch_id),
            LaunchCertificate {
                namespace: correlation.identity_namespace,
                surface: correlation.surface_id,
                session,
                kind: LaunchKind::LocalZellij(identity.clone()),
                cancellation: cancel.clone(),
            },
        );
        self.pending_launches += 1;
        self.launch_workers.push(thread);
        if installed {
            self.slots.insert(
                session,
                Slot {
                    cancellation: cancel.clone(),
                    launch_id,
                    session: None,
                    connection: None,
                    cached: None,
                    starting: true,
                    failure: None,
                    resize: None,
                    last_notice: 0,
                },
            );
        }
        self.local_zellij.jobs.insert(
            id,
            Job {
                correlation: correlation.clone(),
                session,
                surface,
                launch_id,
                cancel,
                deadline,
                reservation: None,
                identity: identity.clone(),
                installed,
                checkpoint: None,
                focus,
                authorization,
            },
        );
        self.local_zellij.receipts.push(Receipt {
            epoch: self.local_zellij.epoch,
            operation_id: id,
            state: "starting".into(),
            profile_id,
            owner_uuid: identity.map(|i| i.owner_uuid),
            surface: correlation.surface_id,
            checkpoint_revision: None,
            frontend_pid: None,
            error: None,
            final_frame_received: false,
            monitor_joined: false,
            namespace_cleaned: false,
            session_joined: false,
            frontend_exit_code: None,
        });
        Ok(id)
    }
    pub(crate) fn poll_zellij(&mut self) -> bool {
        let mut dirty = false;
        for _ in 0..MAX_OPERATIONS {
            match self.local_zellij.events_rx.try_recv() {
                Ok(LaunchEvent::OwnerReserved {
                    correlation,
                    owner_uuid,
                }) => {
                    dirty = true;
                    if let Err(error) = self.zellij_owner_reserved(correlation.clone(), owner_uuid)
                    {
                        self.fail_zellij(correlation.operation_id, error.to_string());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.local_zellij.cancel_all();
                    break;
                }
            }
        }
        let invalid: Vec<_> = self
            .local_zellij
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.cancel.load(Ordering::Acquire)
                    || Instant::now() >= job.deadline
                    || self.shared.stop.load(Ordering::Acquire)
                    || (job.installed && !self.zellij_job_target_matches(job))
                    || (job.reservation.is_some()
                        && job.correlation.identity_namespace
                            != self.model.snapshot().identity_namespace)
            })
            .filter(|(id, _)| {
                self.local_zellij
                    .receipts
                    .iter()
                    .any(|r| r.operation_id == **id && r.state != "canceling")
            })
            .map(|(id, _)| *id)
            .collect();
        for id in invalid {
            self.fail_zellij(
                id,
                "local Zellij launch cancelled, expired or its target changed".into(),
            );
            dirty = true;
        }
        dirty |= self.poll_zellij_observations();
        dirty
    }
    pub(crate) fn poll_zellij_observations(&mut self) -> bool {
        let observations: Vec<_> = self
            .local_zellij
            .observations
            .iter()
            .map(|(id, handle)| (*id, handle.snapshot()))
            .collect();
        let mut dirty = false;
        for (id, observation) in observations {
            let observation = match observation {
                Ok(observation) => observation,
                Err(error) => {
                    self.last_error = Some(error.to_string());
                    continue;
                }
            };
            if let Some(receipt) = self.local_zellij.receipt_mut(id) {
                dirty |= receipt.final_frame_received != observation.final_receipt.is_some()
                    || receipt.monitor_joined != observation.monitor_joined
                    || receipt.namespace_cleaned != observation.namespace_cleaned;
                receipt.final_frame_received = observation.final_receipt.is_some();
                receipt.monitor_joined = observation.monitor_joined;
                receipt.namespace_cleaned = observation.namespace_cleaned;
                if let Some(error) = &observation.error {
                    dirty |= receipt.error.as_ref() != Some(error);
                    receipt.error = Some(error.clone());
                }
            }
            if observation.error.is_some() {
                let target = self
                    .local_zellij
                    .tags
                    .iter()
                    .find(|(_, tag)| tag.correlation.operation_id == id)
                    .map(|(key, _)| *key);
                if let Some((session_id, launch_id)) = target
                    && self
                        .slots
                        .get(&session_id)
                        .is_some_and(|slot| slot.launch_id == launch_id)
                {
                    let slot = self.slots.get_mut(&session_id).expect("matched slot");
                    slot.cancellation.store(true, Ordering::Release);
                    slot.starting = false;
                    slot.failure = observation.error.clone();
                    if let Some(session) = slot.session.take() {
                        self.retire_owned(session, session_id, launch_id);
                        dirty = true;
                    }
                }
            }
        }
        dirty
    }
    pub(crate) fn zellij_retirement_tag(
        &self,
        session: SessionId,
        launch_id: u64,
    ) -> Option<ZellijTag> {
        self.local_zellij.tags.get(&(session, launch_id)).cloned()
    }
    pub(crate) fn zellij_session_joined(
        &mut self,
        tag: &ZellijTag,
        exit_code: Option<u32>,
        result: &Result<(), SessionError>,
    ) {
        if !self
            .local_zellij
            .tags
            .get(&(tag.session, tag.launch_id))
            .is_some_and(|current| current.correlation == tag.correlation)
        {
            return;
        }
        let observed = self
            .local_zellij
            .observations
            .get(&tag.correlation.operation_id)
            .map(|handle| handle.snapshot());
        if let Some(receipt) = self.local_zellij.receipt_mut(tag.correlation.operation_id) {
            receipt.session_joined = result.is_ok();
            receipt.frontend_exit_code = exit_code;
            if let Err(error) = result {
                receipt.state = "cleanup_failed".into();
                receipt.error = Some(error.to_string());
                self.cleanup_error = receipt.error.clone();
                return;
            }
            let observation = match observed {
                Some(Ok(observation)) => observation,
                Some(Err(error)) => {
                    receipt.state = "cleanup_unknown".into();
                    receipt.error = Some(error.to_string());
                    self.cleanup_error = receipt.error.clone();
                    return;
                }
                None => {
                    receipt.state = "cleanup_unknown".into();
                    receipt.error =
                        Some("local Zellij final observer evidence is unavailable".into());
                    self.cleanup_error = receipt.error.clone();
                    return;
                }
            };
            receipt.final_frame_received = observation.final_receipt.is_some();
            receipt.monitor_joined = observation.monitor_joined;
            receipt.namespace_cleaned = observation.namespace_cleaned;
            if let Some(error) = observation.error {
                receipt.error = Some(error);
            }
            let Some(final_receipt) = observation.final_receipt else {
                receipt.state = "cleanup_unknown".into();
                receipt.error.get_or_insert_with(|| "local Zellij final observer receipt is missing; preservation outcome is unknown".into());
                self.cleanup_error = receipt.error.clone();
                return;
            };
            let complete = observation.monitor_joined
                && observation.namespace_cleaned
                && exit_code == Some(0)
                && final_receipt.workers_joined
                && final_receipt.original_owner_preserved
                && final_receipt.transport.errors.is_empty()
                && receipt.error.is_none();
            receipt.state = if complete {
                "frontend_joined"
            } else {
                "cleanup_failed"
            }
            .into();
            if !complete {
                receipt.error.get_or_insert_with(|| {
                    "local Zellij final cleanup evidence was incomplete or unsuccessful".into()
                });
                self.cleanup_error = receipt.error.clone();
            }
        }
    }
    pub(crate) fn fail_zellij(&mut self, id: u64, error: String) {
        if let Some(job) = self.local_zellij.jobs.get(&id) {
            job.cancel.store(true, Ordering::Release);
        }
        if let Some(receipt) = self.local_zellij.receipt_mut(id) {
            receipt.state = "canceling".into();
            receipt.error = Some(error);
        }
    }
    pub(crate) fn zellij_job_target_matches(&self, job: &Job) -> bool {
        let model = self.launch_model().snapshot();
        model.identity_namespace == job.correlation.identity_namespace
            && self
                .slots
                .get(&job.session)
                .is_some_and(|slot| slot.launch_id == job.launch_id)
            && model
                .workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .flat_map(|p| &p.surfaces)
                .any(|surface| {
                    surface.id == job.surface
                        && surface.session_id == Some(job.session)
                        && surface.local_zellij == job.identity
                })
    }
    pub(crate) fn authorize_zellij_checkpoint(&mut self, seal: CheckpointSeal) {
        let valid = !self.shared.stop.load(Ordering::Acquire)
            && self
                .local_zellij
                .jobs
                .get(&seal.correlation.operation_id)
                .is_some_and(|job| {
                    job.correlation == seal.correlation
                        && job.session == seal.session
                        && job.surface == seal.surface
                        && job.launch_id == seal.launch_id
                        && job.identity.as_ref() == Some(&seal.identity)
                        && job.checkpoint == Some(seal.io_id)
                        && Instant::now() < job.deadline
                        && !job.cancel.load(Ordering::Acquire)
                        && self.zellij_job_target_matches(job)
                });
        if !valid {
            self.fail_zellij(
                seal.correlation.operation_id,
                "local Zellij checkpoint completed after authorization was revoked".into(),
            );
            return;
        }
        let id = seal.correlation.operation_id;
        let job = self
            .local_zellij
            .jobs
            .get_mut(&id)
            .expect("checked operation");
        job.checkpoint = None;
        match job.authorization.try_send(CheckpointSaved {
            correlation: seal.correlation,
            owner_uuid: seal.identity.owner_uuid,
            document_revision: seal.revision,
        }) {
            Ok(()) => {
                if let Some(receipt) = self.local_zellij.receipt_mut(id) {
                    receipt.state = "reservation_committed".into();
                    receipt.checkpoint_revision = Some(seal.revision);
                }
            }
            Err(error) => self.fail_zellij(
                id,
                format!("local Zellij reservation authorization was not delivered: {error}"),
            ),
        }
    }
    pub(crate) fn complete_zellij(&mut self, completion: &mut Completion) {
        let Some(metadata) = &completion.zellij else {
            return;
        };
        let matched = self
            .local_zellij
            .jobs
            .get(&metadata.tag.correlation.operation_id)
            .is_some_and(|job| {
                job.correlation == metadata.tag.correlation
                    && job.session == completion.id
                    && job.launch_id == completion.launch_id
            });
        let authorized = matched
            && self
                .local_zellij
                .jobs
                .get(&metadata.tag.correlation.operation_id)
                .is_some_and(|job| {
                    !self.shared.stop.load(Ordering::Acquire)
                        && !job.cancel.load(Ordering::Acquire)
                        && Instant::now() < job.deadline
                        && self.zellij_job_target_matches(job)
                });
        if completion.result.is_ok() && !authorized {
            let rejected = std::mem::replace(
                &mut completion.result,
                Err(SessionError::Failure(
                    "local Zellij launch completion arrived after ownership was revoked".into(),
                )),
            );
            if let Ok(session) = rejected {
                completion.cleanup = Some(session);
            }
        }
        if !matched {
            return;
        }
        let job = self
            .local_zellij
            .jobs
            .remove(&metadata.tag.correlation.operation_id)
            .expect("matched job");
        if let Some(ticket) = job.reservation
            && ticket.identity_namespace() == self.model.snapshot().identity_namespace
            && let Err(error) = self.model.cancel_local_zellij_reservation(ticket)
        {
            self.last_error = Some(format!("cancel local Zellij reservation: {error}"));
        }
        if let Some(observer) = &metadata.observer {
            self.local_zellij
                .observations
                .insert(metadata.tag.correlation.operation_id, observer.clone());
        }
        let error = completion.result.as_ref().err().map(ToString::to_string);
        if let Some(receipt) = self
            .local_zellij
            .receipt_mut(metadata.tag.correlation.operation_id)
        {
            receipt.state = if error.is_none() {
                "transport_connected"
            } else {
                "failed"
            }
            .into();
            receipt.error = error.clone();
            receipt.frontend_pid = metadata.frontend_pid;
            receipt.owner_uuid = metadata.owner_uuid.or(receipt.owner_uuid);
            if completion.result.is_err() && completion.cleanup.is_none() {
                receipt.session_joined = true;
            }
        }
        if let Some(error) = error {
            self.last_error = Some(error);
        }
    }
}
fn failure(error: impl ToString) -> DesktopError {
    DesktopError::Failure(error.to_string())
}
