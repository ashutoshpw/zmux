//! Serialized request handling, durable state transitions, and due-job dispatch.

use super::{
    DeliveryPhase, DispatchCheckpoint, DispatchReceipt, EditPatch, JobState,
    MAX_DISPATCH_LATENESS_SECONDS, MutationKind, ReceiptState, RequestReceipt, ScheduledJob,
    SchedulerAction, SchedulerError, SchedulerErrorCode, SchedulerReply, SchedulerRequest,
    SchedulerResponse, SchedulerResult, SchedulerStatus, ServiceMode, StoreHealth, TargetIdentity,
    store::{Store, StoreError, receipt_expiry},
    store_state::{StoreState, StoredReceipt},
    tmux::{DispatchResult, Tmux, TmuxError},
};
use nix::unistd::geteuid;
use sha2::{Digest, Sha256};
use std::{env, path::PathBuf};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

#[derive(Clone)]
pub struct SchedulerService {
    store: Store,
    tmux: Tmux,
}

impl SchedulerService {
    pub fn open(root: PathBuf) -> Result<Self, SchedulerError> {
        Self::open_with_tmux(
            root,
            env::var_os("ZMUX_SCHEDULER_TMUX_SOCKET").map(PathBuf::from),
            env::var_os("ZMUX_SCHEDULER_TMUX_BIN").map(PathBuf::from),
        )
    }

    /// Open a scheduler with an explicitly configured tmux socket and binary.
    /// Unspecified values use the platform's default socket and PATH lookup.
    pub fn open_with_tmux(
        root: PathBuf,
        socket: Option<PathBuf>,
        binary: Option<PathBuf>,
    ) -> Result<Self, SchedulerError> {
        let tmux = Tmux::from_config(socket, binary).map_err(map_tmux_error)?;
        Self::open_with_client(root, tmux)
    }

    fn open_with_client(root: PathBuf, tmux: Tmux) -> Result<Self, SchedulerError> {
        let store = Store::open(root).map_err(map_store_error)?;
        Ok(Self { store, tmux })
    }

    pub fn open_from_environment() -> Result<Self, SchedulerError> {
        Self::open(default_root()?)
    }

    pub fn store_root(&self) -> &std::path::Path {
        self.store.root()
    }

    /// Process one validated versioned request. Setup operations are reserved
    /// for the managed-SSH transport and never execute inside this daemon.
    pub fn handle(&self, request: SchedulerRequest) -> SchedulerResponse {
        if let Err(error) = request.validate() {
            return SchedulerResponse::error(
                request.request_id,
                SchedulerError::new(error.error_code(), error.to_string()),
            );
        }
        match request.action.clone() {
            SchedulerAction::SetupCheck | SchedulerAction::SetupInstall => response_error(
                request.request_id,
                SchedulerErrorCode::InvalidRequest,
                "setup actions are handled by the managed-SSH transport",
            ),
            SchedulerAction::Status { receipt_request_id } => {
                if let Some(receipt_request_id) = receipt_request_id {
                    return self.request_status(request.request_id, receipt_request_id);
                }
                self.status(&request)
            }
            SchedulerAction::Targets => self.targets(&request),
            SchedulerAction::Create {
                target_id,
                deadline,
                message,
                submit,
            } => self.create(&request, target_id, deadline, message, submit),
            SchedulerAction::List { limit } => self.list(&request, limit.unwrap_or(100)),
            SchedulerAction::Show { job_id } => self.show(&request, job_id),
            SchedulerAction::Edit {
                job_id,
                expected_version,
                patch,
            } => self.edit(&request, job_id, expected_version, patch),
            SchedulerAction::Cancel {
                job_id,
                expected_version,
            } => self.cancel(&request, job_id, expected_version),
            SchedulerAction::History { limit } => self.history(&request, limit.unwrap_or(100)),
        }
    }

    /// Called once when a foreground service starts. Pending work already past
    /// its deadline becomes missed, and claims interrupted by a prior process
    /// become outcome_unknown. Neither case is replayed.
    pub fn recover_on_start(&self) -> Result<(), SchedulerError> {
        let now = OffsetDateTime::now_utc();
        let now_nanos = now.unix_timestamp_nanos();
        let updated = format_utc(now);
        self.store
            .transact(|state| {
                let mut changed = false;
                for job in &mut state.jobs {
                    match job.state {
                        JobState::Dispatching => {
                            let mut receipt = job.dispatch.clone().unwrap_or(DispatchReceipt {
                                text: DeliveryPhase::Unknown,
                                submit: DeliveryPhase::Unknown,
                                last_durable_phase: DispatchCheckpoint::Claimed,
                            });
                            if receipt.text == DeliveryPhase::NotStarted {
                                receipt.text = DeliveryPhase::Unknown;
                            }
                            if job.submit && receipt.submit == DeliveryPhase::NotStarted {
                                receipt.submit = DeliveryPhase::Unknown;
                            }
                            job.dispatch = Some(receipt);
                            job.state = JobState::OutcomeUnknown;
                            job.updated_at_utc = updated.clone();
                            changed = true;
                        }
                        JobState::Pending if deadline_nanos(job) <= now_nanos => {
                            job.state = JobState::Missed;
                            job.updated_at_utc = updated.clone();
                            changed = true;
                        }
                        _ => {}
                    }
                }
                Ok(((), changed))
            })
            .map_err(map_store_error)
    }

    /// Claim and dispatch at most one due job. The claim is synced before the
    /// first tmux side effect, so a process restart cannot replay uncertain input.
    pub fn dispatch_due_once(&self) -> Result<bool, SchedulerError> {
        let claimed = self
            .store
            .transact(|state| {
                // Sample only after the store lock is held. A stale pre-lock
                // timestamp could claim a job after its dispatch grace elapsed.
                let now = OffsetDateTime::now_utc();
                let now_nanos = now.unix_timestamp_nanos();
                let updated = format_utc(now);
                let mut changed = false;
                for job in &mut state.jobs {
                    if job.state == JobState::Pending
                        && deadline_nanos(job) <= now_nanos
                        && is_past_dispatch_grace(deadline_nanos(job), now_nanos)
                    {
                        job.state = JobState::Missed;
                        job.updated_at_utc = updated.clone();
                        changed = true;
                    }
                }
                let candidate = state
                    .jobs
                    .iter()
                    .enumerate()
                    .filter(|(_, job)| {
                        job.state == JobState::Pending && deadline_nanos(job) <= now_nanos
                    })
                    .min_by_key(|(_, job)| deadline_nanos(job))
                    .map(|(index, _)| index);
                let Some(index) = candidate else {
                    return Ok((None, changed));
                };
                let job = &mut state.jobs[index];
                job.state = JobState::Dispatching;
                job.updated_at_utc = updated.clone();
                job.dispatch = Some(DispatchReceipt {
                    text: DeliveryPhase::NotStarted,
                    submit: DeliveryPhase::NotStarted,
                    last_durable_phase: DispatchCheckpoint::Claimed,
                });
                Ok((Some(job.clone()), true))
            })
            .map_err(map_store_error)?;
        let Some(job) = claimed else {
            return Ok(false);
        };
        let mut refreshed_display = None;
        let result = self.tmux.dispatch(
            &job,
            |checkpoint| {
                self.store
                    .transact(|state| {
                        let Some(current) = state.jobs.iter_mut().find(|item| item.id == job.id)
                        else {
                            return Err(StoreError::Corrupt(
                                "claimed scheduler job disappeared before text checkpoint".into(),
                            ));
                        };
                        if current.state != JobState::Dispatching {
                            return Err(StoreError::Corrupt(
                                "claimed scheduler job changed before text checkpoint".into(),
                            ));
                        }
                        current.dispatch = Some(checkpoint.clone());
                        Ok(((), true))
                    })
                    .is_ok()
            },
            &mut refreshed_display,
        );
        self.store
            .transact(|state| {
                let Some(current) = state.jobs.iter_mut().find(|item| item.id == job.id) else {
                    return Err(StoreError::Corrupt(
                        "claimed scheduler job disappeared before completion".into(),
                    ));
                };
                if current.state != JobState::Dispatching {
                    return Err(StoreError::Corrupt(
                        "claimed scheduler job changed state during dispatch".into(),
                    ));
                }
                current.updated_at_utc = format_utc(OffsetDateTime::now_utc());
                if let Some(display) = refreshed_display {
                    current.display = display;
                }
                match result {
                    DispatchResult::Sent(receipt) => {
                        current.state = JobState::Sent;
                        current.dispatch = Some(receipt);
                        current.failure_code = None;
                    }
                    DispatchResult::Failed(receipt, code) => {
                        current.state = JobState::Failed;
                        current.dispatch = Some(receipt);
                        current.failure_code = Some(code);
                    }
                    DispatchResult::Unknown(receipt) => {
                        current.state = JobState::OutcomeUnknown;
                        current.dispatch = Some(receipt);
                        current.failure_code = None;
                    }
                    DispatchResult::Missed(receipt) => {
                        current.state = JobState::Missed;
                        current.dispatch = Some(receipt);
                        current.failure_code = None;
                    }
                }
                Ok(((), true))
            })
            .map_err(map_store_error)?;
        Ok(true)
    }

    fn status(&self, request: &SchedulerRequest) -> SchedulerResponse {
        let state = match self.store.read(Clone::clone) {
            Ok(state) => state,
            Err(error) => return response_store_error(request.request_id, error),
        };
        let remote_uid = geteuid().as_raw();
        let tmux_status = self.tmux.status().ok().flatten();
        let is_systemd = env::var_os("INVOCATION_ID").is_some();
        let status = SchedulerStatus {
            helper_instance_id: state.helper_instance_id,
            remote_uid,
            helper_version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: super::SCHEDULER_PROTOCOL_VERSION,
            service_mode: if is_systemd {
                ServiceMode::SystemdUser
            } else {
                ServiceMode::ManualForeground
            },
            store_health: StoreHealth::Healthy,
            checked_at_utc: format_utc(OffsetDateTime::now_utc()),
            tmux_available: tmux_status.is_some(),
            default_tmux_socket_id: tmux_status.as_ref().map(|value| value.socket_id.clone()),
            server_incarnation: tmux_status.and_then(|value| value.server_incarnation),
            linger_enabled: linger_enabled(),
        };
        SchedulerResponse::success(request, SchedulerReply::Status { status })
    }

    fn targets(&self, request: &SchedulerRequest) -> SchedulerResponse {
        let state = match self.store.read(Clone::clone) {
            Ok(state) => state,
            Err(error) => return response_store_error(request.request_id, error),
        };
        match self
            .tmux
            .targets(state.helper_instance_id, geteuid().as_raw())
        {
            Ok(targets) => SchedulerResponse::success(request, SchedulerReply::Targets { targets }),
            Err(error) => SchedulerResponse::error(request.request_id, map_tmux_error(error)),
        }
    }

    fn list(&self, request: &SchedulerRequest, limit: u16) -> SchedulerResponse {
        let mut jobs = match self.store.read(|state| {
            let mut jobs: Vec<_> = state
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Pending | JobState::Dispatching))
                .cloned()
                .collect();
            jobs.sort_by_key(deadline_nanos);
            jobs.truncate(limit as usize);
            jobs
        }) {
            Ok(jobs) => jobs,
            Err(error) => return response_store_error(request.request_id, error),
        };
        let identities: Vec<_> = jobs.iter().map(|job| job.target.clone()).collect();
        if let Ok(displays) = self.tmux.displays_for(&identities) {
            for (job, display) in jobs.iter_mut().zip(displays) {
                if let Some(display) = display {
                    job.display = display;
                }
            }
        }
        SchedulerResponse::success(request, SchedulerReply::Jobs { jobs })
    }

    fn show(&self, request: &SchedulerRequest, job_id: Uuid) -> SchedulerResponse {
        match self
            .store
            .read(|state| state.jobs.iter().find(|job| job.id == job_id).cloned())
        {
            Ok(Some(mut job)) => {
                if let Ok(display) = self.tmux.verify_target(&job.target) {
                    job.display = display;
                }
                SchedulerResponse::success(request, SchedulerReply::Job { job })
            }
            Ok(None) => response_error(
                request.request_id,
                SchedulerErrorCode::NotFound,
                "scheduled job was not found",
            ),
            Err(error) => response_store_error(request.request_id, error),
        }
    }

    fn history(&self, request: &SchedulerRequest, limit: u16) -> SchedulerResponse {
        match self.store.read(|state| {
            let mut jobs: Vec<_> = state
                .jobs
                .iter()
                .filter(|job| !matches!(job.state, JobState::Pending | JobState::Dispatching))
                .cloned()
                .collect();
            jobs.sort_by(|left, right| right.updated_at_utc.cmp(&left.updated_at_utc));
            let truncated = jobs.len() > limit as usize || state.pruned_before_utc.is_some();
            jobs.truncate(limit as usize);
            (jobs, truncated, state.pruned_before_utc.clone())
        }) {
            Ok((jobs, truncated, pruned_before_utc)) => SchedulerResponse::success(
                request,
                SchedulerReply::History {
                    jobs,
                    truncated,
                    pruned_before_utc,
                },
            ),
            Err(error) => response_store_error(request.request_id, error),
        }
    }

    fn request_status(&self, request: Option<Uuid>, receipt_id: Uuid) -> SchedulerResponse {
        match self
            .store
            .read(|state| Store::receipt(state, receipt_id).map(|entry| entry.receipt.clone()))
        {
            Ok(Some(receipt)) => SchedulerResponse {
                protocol_version: super::SCHEDULER_PROTOCOL_VERSION,
                request_id: request,
                result: SchedulerResult::Success(SchedulerReply::RequestStatus { receipt }),
            },
            Ok(None) => {
                let mut error = SchedulerError::new(
                    SchedulerErrorCode::NotFoundOrExpired,
                    "request receipt was not found or has expired; the original outcome is unknown",
                );
                error.outcome_unknown = true;
                SchedulerResponse::error(request, error)
            }
            Err(error) => response_store_error(request, error),
        }
    }

    fn create(
        &self,
        request: &SchedulerRequest,
        target_id: String,
        deadline: super::DeadlineInput,
        message: String,
        submit: bool,
    ) -> SchedulerResponse {
        self.mutate(request, MutationKind::Create, move |state, accepted_at| {
            let identity = TargetIdentity::from_target_id(&target_id).map_err(|error| {
                SchedulerError::new(SchedulerErrorCode::InvalidTarget, error.to_string())
            })?;
            if identity.helper_instance_id != state.helper_instance_id
                || identity.remote_uid != geteuid().as_raw()
            {
                return Err(SchedulerError::new(
                    SchedulerErrorCode::InvalidTarget,
                    "target token belongs to a different scheduler helper or remote account",
                ));
            }
            let display = self.tmux.verify_target(&identity).map_err(map_tmux_error)?;
            let resolved = deadline
                .resolve(accepted_at)
                .map_err(|error| SchedulerError::new(error.error_code(), error.to_string()))?;
            let active = state
                .jobs
                .iter()
                .filter(|job| matches!(job.state, JobState::Pending | JobState::Dispatching))
                .count();
            if active >= super::MAX_PENDING_JOBS {
                return Err(SchedulerError::new(
                    SchedulerErrorCode::Capacity,
                    "scheduler already has 100 pending or dispatching jobs",
                ));
            }
            let timestamp = format_utc(accepted_at);
            let job = ScheduledJob {
                id: Uuid::new_v4(),
                version: 1,
                state: JobState::Pending,
                message,
                deadline_utc: resolved.deadline_utc,
                deadline_input: resolved.input,
                submit,
                target: identity,
                display,
                created_at_utc: timestamp.clone(),
                updated_at_utc: timestamp,
                dispatch: None,
                failure_code: None,
            };
            state.jobs.push(job.clone());
            Ok((
                SchedulerReply::Mutation {
                    job: job.clone(),
                    receipt: placeholder_receipt(job.id, job.version),
                },
                Some(job.id),
                Some(job.version),
            ))
        })
    }

    fn edit(
        &self,
        request: &SchedulerRequest,
        job_id: Uuid,
        expected_version: u64,
        patch: EditPatch,
    ) -> SchedulerResponse {
        self.mutate(request, MutationKind::Edit, move |state, accepted_at| {
            let Some(index) = state.jobs.iter().position(|job| job.id == job_id) else {
                return Err(SchedulerError::new(
                    SchedulerErrorCode::NotFound,
                    "scheduled job was not found",
                ));
            };
            if state.jobs[index].version != expected_version {
                let mut error = SchedulerError::new(
                    SchedulerErrorCode::StaleVersion,
                    "scheduled job version changed; refresh before editing",
                );
                error.current_version = Some(state.jobs[index].version);
                return Err(error);
            }
            if state.jobs[index].state != JobState::Pending {
                let mut error = SchedulerError::new(
                    SchedulerErrorCode::AlreadyClaimed,
                    "only a pending job can be edited",
                );
                error.current_version = Some(state.jobs[index].version);
                return Err(error);
            }
            let resolved = patch
                .deadline
                .as_ref()
                .map(|deadline| deadline.resolve(accepted_at))
                .transpose()
                .map_err(|error| SchedulerError::new(error.error_code(), error.to_string()))?;
            let current = &mut state.jobs[index];
            if let Some(message) = patch.message {
                current.message = message;
            }
            if let Some(resolved) = resolved {
                current.deadline_utc = resolved.deadline_utc;
                current.deadline_input = resolved.input;
            }
            let next_version = current.version.checked_add(1).ok_or_else(|| {
                SchedulerError::new(
                    SchedulerErrorCode::Capacity,
                    "scheduled job version is exhausted",
                )
            })?;
            current.version = next_version;
            current.updated_at_utc = format_utc(accepted_at);
            let job = current.clone();
            Ok((
                SchedulerReply::Mutation {
                    job: job.clone(),
                    receipt: placeholder_receipt(job.id, job.version),
                },
                Some(job.id),
                Some(job.version),
            ))
        })
    }

    fn cancel(
        &self,
        request: &SchedulerRequest,
        job_id: Uuid,
        expected_version: u64,
    ) -> SchedulerResponse {
        self.mutate(request, MutationKind::Cancel, move |state, accepted_at| {
            let Some(job) = state.jobs.iter_mut().find(|job| job.id == job_id) else {
                return Err(SchedulerError::new(
                    SchedulerErrorCode::NotFound,
                    "scheduled job was not found",
                ));
            };
            if job.version != expected_version {
                let mut error = SchedulerError::new(
                    SchedulerErrorCode::StaleVersion,
                    "scheduled job version changed; refresh before cancelling",
                );
                error.current_version = Some(job.version);
                return Err(error);
            }
            if job.state != JobState::Pending {
                let mut error = SchedulerError::new(
                    SchedulerErrorCode::AlreadyClaimed,
                    "only a pending job can be cancelled",
                );
                error.current_version = Some(job.version);
                return Err(error);
            }
            let next_version = job.version.checked_add(1).ok_or_else(|| {
                SchedulerError::new(
                    SchedulerErrorCode::Capacity,
                    "scheduled job version is exhausted",
                )
            })?;
            job.version = next_version;
            job.state = JobState::Cancelled;
            job.updated_at_utc = format_utc(accepted_at);
            let job = job.clone();
            Ok((
                SchedulerReply::Mutation {
                    job: job.clone(),
                    receipt: placeholder_receipt(job.id, job.version),
                },
                Some(job.id),
                Some(job.version),
            ))
        })
    }

    fn mutate(
        &self,
        request: &SchedulerRequest,
        operation: MutationKind,
        apply: impl FnOnce(
            &mut StoreState,
            OffsetDateTime,
        )
            -> Result<(SchedulerReply, Option<Uuid>, Option<u64>), SchedulerError>,
    ) -> SchedulerResponse {
        let request_id = request.request_id.expect("validated mutation request ID");
        let fingerprint = fingerprint(&request.action);
        let request_clone = request.clone();
        let result = self.store.transact(|state| {
            Store::prune_expired_receipts(state);
            if let Some(existing) = Store::receipt(state, request_id) {
                if existing.fingerprint == fingerprint {
                    return Ok((existing.response.clone(), false));
                }
                return Ok((response_error(
                    Some(request_id),
                    SchedulerErrorCode::RequestIdConflict,
                    "request ID was already used with a different operation payload",
                ), false));
            }
            let additional_capacity_units = match &request.action {
                // A create records one receipt and creates a new pending-job
                // reservation so a later cancel remains available.
                SchedulerAction::Create { .. } => 2,
                SchedulerAction::Edit { .. } => 1,
                SchedulerAction::Cancel {
                    job_id,
                    expected_version,
                } => {
                    let can_consume_reserved_slot = state.jobs.iter().any(|job| {
                        job.id == *job_id
                            && job.state == JobState::Pending
                            && job.version == *expected_version
                            && job.version.checked_add(1).is_some()
                    });
                    usize::from(!can_consume_reserved_slot)
                }
                _ => 1,
            };
            if !Store::receipt_capacity_available(state, additional_capacity_units) {
                return Ok((response_error(
                    Some(request_id),
                    SchedulerErrorCode::Capacity,
                    "receipt capacity is exhausted or reserved for pending-job cancellations; no mutation was applied and this request ID was not recorded. Up to 1024 receipts are retained for 30 days; retry the same request ID after a receipt expires or a pending job is cancelled",
                ), false));
            }
            let accepted_at = OffsetDateTime::now_utc();
            let result = apply(state, accepted_at);
            let requested_job_id = match &request.action {
                SchedulerAction::Edit { job_id, .. } | SchedulerAction::Cancel { job_id, .. } => Some(*job_id),
                _ => None,
            };
            let requested_version = match &request.action {
                SchedulerAction::Edit { expected_version, .. } | SchedulerAction::Cancel { expected_version, .. } => Some(*expected_version),
                _ => None,
            };
            let (receipt_state, job_id, job_version) = match &result {
                Ok((_, job_id, version)) => (ReceiptState::Applied, *job_id, *version),
                Err(error) => (
                    ReceiptState::Rejected,
                    requested_job_id,
                    error.current_version.or(requested_version),
                ),
            };
            let receipt = RequestReceipt {
                request_id,
                operation,
                state: receipt_state,
                job_id,
                job_version,
                received_at_utc: format_utc(accepted_at),
                completed_at_utc: Some(format_utc(OffsetDateTime::now_utc())),
            };
            let response = match result {
                Ok((reply, _, _)) => {
                    let reply = attach_receipt(reply, receipt.clone());
                    SchedulerResponse::success(&request_clone, reply)
                }
                Err(error) => SchedulerResponse::error(Some(request_id), error),
            };
            Store::remember_receipt(
                state,
                StoredReceipt {
                    fingerprint,
                    receipt,
                    response: response.clone(),
                    expires_at_unix_nanos: receipt_expiry(accepted_at),
                },
            );
            Ok((response, true))
        });
        match result {
            Ok(response) => response,
            Err(error) => response_store_error(Some(request_id), error),
        }
    }
}

fn attach_receipt(reply: SchedulerReply, receipt: RequestReceipt) -> SchedulerReply {
    match reply {
        SchedulerReply::Mutation { job, .. } => SchedulerReply::Mutation { job, receipt },
        reply => reply,
    }
}

fn placeholder_receipt(job_id: Uuid, version: u64) -> RequestReceipt {
    RequestReceipt {
        request_id: Uuid::nil(),
        operation: MutationKind::Create,
        state: ReceiptState::Applied,
        job_id: Some(job_id),
        job_version: Some(version),
        received_at_utc: String::new(),
        completed_at_utc: None,
    }
}

fn fingerprint(action: &SchedulerAction) -> String {
    let body = serde_json::to_vec(action).expect("scheduler actions are serializable");
    let digest = Sha256::digest(body);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn deadline_nanos(job: &ScheduledJob) -> i128 {
    OffsetDateTime::parse(&job.deadline_utc, &Rfc3339)
        .map(|deadline| deadline.unix_timestamp_nanos())
        .unwrap_or(i128::MAX)
}

fn is_past_dispatch_grace(deadline_nanos: i128, now_nanos: i128) -> bool {
    now_nanos.saturating_sub(deadline_nanos)
        > i128::from(MAX_DISPATCH_LATENESS_SECONDS) * 1_000_000_000
}

fn format_utc(value: OffsetDateTime) -> String {
    value
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub(super) fn default_root() -> Result<PathBuf, SchedulerError> {
    if let Some(root) = env::var_os("ZMUX_SCHEDULER_ROOT") {
        let root = PathBuf::from(root);
        if root.is_absolute() {
            return Ok(root);
        }
        return Err(SchedulerError::new(
            SchedulerErrorCode::InvalidRequest,
            "ZMUX_SCHEDULER_ROOT must be an absolute path",
        ));
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| SchedulerError::new(SchedulerErrorCode::Internal, "HOME is not set"))?;
    let state_home = match env::var_os("XDG_STATE_HOME") {
        Some(value) => {
            let value = PathBuf::from(value);
            if !value.is_absolute() {
                return Err(SchedulerError::new(
                    SchedulerErrorCode::InvalidRequest,
                    "XDG_STATE_HOME must be an absolute path",
                ));
            }
            value
        }
        None => home.join(".local").join("state"),
    };
    Ok(state_home.join("zmux").join("scheduler"))
}

fn response_error(
    request_id: Option<Uuid>,
    code: SchedulerErrorCode,
    message: impl Into<String>,
) -> SchedulerResponse {
    SchedulerResponse::error(request_id, SchedulerError::new(code, message))
}

fn response_store_error(request_id: Option<Uuid>, error: StoreError) -> SchedulerResponse {
    let (code, retryable, outcome_unknown) = match &error {
        StoreError::Busy => (SchedulerErrorCode::Busy, true, false),
        StoreError::CommitUncertain(_) => (SchedulerErrorCode::StoreUnavailable, true, true),
        StoreError::Unsafe(_) | StoreError::Corrupt(_) | StoreError::Io(_) => {
            (SchedulerErrorCode::StoreUnavailable, false, false)
        }
    };
    let message = error.to_string();
    let mut detail = SchedulerError::new(code, message);
    detail.retryable = retryable;
    detail.outcome_unknown = outcome_unknown;
    SchedulerResponse::error(request_id, detail)
}

#[cfg(test)]
mod tests;

fn map_store_error(error: StoreError) -> SchedulerError {
    let (code, retryable, outcome_unknown) = match &error {
        StoreError::Busy => (SchedulerErrorCode::Busy, true, false),
        StoreError::CommitUncertain(_) => (SchedulerErrorCode::StoreUnavailable, true, true),
        StoreError::Unsafe(_) | StoreError::Corrupt(_) | StoreError::Io(_) => {
            (SchedulerErrorCode::StoreUnavailable, false, false)
        }
    };
    let mut result = SchedulerError::new(code, error.to_string());
    result.retryable = retryable;
    result.outcome_unknown = outcome_unknown;
    result
}

fn map_tmux_error(error: TmuxError) -> SchedulerError {
    match error {
        TmuxError::Unavailable => SchedulerError::new(
            SchedulerErrorCode::TmuxUnavailable,
            "default tmux server is unavailable",
        ),
        TmuxError::Missing => SchedulerError::new(
            SchedulerErrorCode::TargetMissing,
            "the selected tmux pane no longer exists",
        ),
        TmuxError::Changed => SchedulerError::new(
            SchedulerErrorCode::TargetChanged,
            "the selected pane moved out of its bound session or tmux server",
        ),
        TmuxError::Rejected(message) => SchedulerError::new(
            SchedulerErrorCode::TmuxUnavailable,
            format!("tmux rejected the scheduler operation: {message}"),
        ),
        TmuxError::Timeout => {
            let mut error = SchedulerError::new(
                SchedulerErrorCode::TmuxUnavailable,
                "tmux operation timed out",
            );
            error.retryable = true;
            error
        }
    }
}

fn linger_enabled() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        if !std::path::Path::new("/run/systemd/system").exists() {
            return None;
        }
        let user = nix::unistd::User::from_uid(geteuid()).ok().flatten()?;
        Some(
            std::path::Path::new("/var/lib/systemd/linger")
                .join(user.name)
                .exists(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
