//! Admission and destructive ownership on the dedicated filesystem worker.
use super::{journal::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstructionPermit {
    pub construction: Uuid,
    pub lease: LeaseToken,
}
pub(super) struct PhysicalConstruction {
    pub permit: ConstructionPermit,
    _admission: File,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationToken {
    pub service_epoch: Uuid,
    pub controller: Uuid,
    pub intent: Uuid,
    pub profile: Uuid,
}
/// Trusted native preflight supplies every exact inactive local store token.
/// This is never accepted from generic control requests.
#[derive(Clone, Debug)]
pub struct MutationRequest {
    pub catalog: PathBuf,
    pub controller: Uuid,
    pub intent: Uuid,
    pub profile: Uuid,
    pub kind: MutationKind,
    pub local_stores: Vec<LeaseToken>,
}
/// Only these correlated known outcomes may end physical ownership. A timeout,
/// transport error or missing callback is deliberately not an outcome variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeMutationOutcome {
    Completed,
    AbortedBeforeClear { message: String },
    RemovalFailed { message: String },
}
pub(super) struct PhysicalMutation {
    pub token: MutationToken,
    record: IntentRecord,
    _admission: File,
    lifetime: Option<File>,
    local: Vec<LeaseToken>,
}
impl Registry {
    pub(super) fn construction_count(&self) -> usize {
        self.constructions.len() + self.leases.values().filter(|v| !v.constructed).count()
    }
    pub(super) fn reuse(
        &mut self,
        directory: PathBuf,
        token: LeaseToken,
        construction: Uuid,
    ) -> Result<ConstructionPermit, LeaseError> {
        if construction.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        if self.constructions.contains_key(&construction) {
            return Err(LeaseError::AlreadyOwned);
        }
        let lease = self
            .leases
            .get(&token.id)
            .filter(|v| v.token == token && v.constructed && token.mode == LeaseMode::Shared)
            .ok_or(LeaseError::StaleToken)?;
        if self.construction_count() >= MAX_CONSTRUCTION_PERMITS {
            return Err(LeaseError::Capacity);
        }
        if self
            .mutations
            .values()
            .any(|v| v.token.profile == token.profile)
        {
            return Err(LeaseError::Busy);
        }
        let admission = open_lock(
            &self.directory,
            &admission_name(token.profile),
            LeaseMode::Shared,
            false,
        )?;
        ensure_unquarantined(&self.directory, token.profile)?;
        inspect_lock(
            &self.directory,
            &lease._lock,
            &lock_name(token.profile),
            false,
        )?;
        let directory = state_directory(&directory).map_err(|e| LeaseError::Io(e.to_string()))?;
        if !profile_exists(&directory, token.profile).map_err(|e| LeaseError::Io(e.to_string()))? {
            return Err(LeaseError::UnknownProfile);
        }
        let permit = ConstructionPermit {
            construction,
            lease: token,
        };
        self.constructions.insert(
            construction,
            PhysicalConstruction {
                permit,
                _admission: admission,
            },
        );
        Ok(permit)
    }
    pub(super) fn construction_done(
        &mut self,
        permit: ConstructionPermit,
    ) -> Result<(), LeaseError> {
        if self
            .constructions
            .get(&permit.construction)
            .is_none_or(|v| v.permit != permit)
        {
            return Err(LeaseError::StaleToken);
        }
        self.constructions.remove(&permit.construction);
        Ok(())
    }
    pub(super) fn mutation_start(
        &mut self,
        request: MutationRequest,
    ) -> Result<MutationToken, LeaseError> {
        let MutationRequest {
            catalog,
            controller,
            intent,
            profile,
            kind,
            local_stores,
        } = request;
        if controller.is_nil() || intent.is_nil() || profile.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        if local_stores.is_empty() && self.leases.len() >= MAX_LEASES {
            return Err(LeaseError::Capacity);
        }
        let next = self.next.checked_add(1).ok_or(LeaseError::Exhausted)?;
        if self.mutations.len() >= MAX_MUTATIONS {
            return Err(LeaseError::Capacity);
        }
        if self.mutations.contains_key(&intent)
            || self.mutations.values().any(|v| v.token.profile == profile)
        {
            return Err(LeaseError::Busy);
        }
        // Pending constructors and any local owner missing from the exact native
        // preflight prevent surrender. Never close user views to create quiescence.
        let local: Vec<_> = self
            .leases
            .values()
            .filter(|v| v.token.profile == profile)
            .collect();
        if local.len() != local_stores.len()
            || local.iter().any(|v| {
                !v.constructed
                    || v.token.mode != LeaseMode::Shared
                    || v.token.owner.epoch != controller
                    || !local_stores.contains(&v.token)
            })
            || self
                .constructions
                .values()
                .any(|v| v.permit.lease.profile == profile)
        {
            return Err(LeaseError::Busy);
        }
        recover_terminal(&self.directory, profile)?;
        let admission = open_lock(
            &self.directory,
            &admission_name(profile),
            LeaseMode::Exclusive,
            false,
        )?;
        ensure_unquarantined(&self.directory, profile)?;
        let catalog_directory =
            state_directory(&catalog).map_err(|e| LeaseError::Io(e.to_string()))?;
        if !profile_exists(&catalog_directory, profile)
            .map_err(|e| LeaseError::Io(e.to_string()))?
        {
            return Err(LeaseError::UnknownProfile);
        }
        let (catalog_device, catalog_inode) = catalog_identity(&catalog_directory)?;
        let token = MutationToken {
            service_epoch: self.epoch,
            controller,
            intent,
            profile,
        };
        let record = IntentRecord {
            version: 1,
            intent,
            profile,
            service_epoch: self.epoch,
            controller,
            catalog,
            catalog_device,
            catalog_inode,
            kind,
            phase: MutationPhase::Reserved,
            error: None,
        };
        // Register before durable work: a lost acknowledgement or uncertain I/O
        // cannot discard the gate or grant permission to repeat native effects.
        self.mutations.insert(
            intent,
            PhysicalMutation {
                token,
                record,
                _admission: admission,
                lifetime: None,
                local: local_stores,
            },
        );
        self.persist_current(token)?;
        let originals = self.mutations[&intent].local.clone();
        for original in &originals {
            self.leases[&original.id]
                ._lock
                .unlock()
                .map_err(|e| LeaseError::Io(e.to_string()))?;
        }
        let lock = match open_lock(
            &self.directory,
            &lock_name(profile),
            LeaseMode::Exclusive,
            false,
        ) {
            Ok(lock) => lock,
            Err(error) => {
                self.restore_local(token)?;
                self.terminal(
                    token,
                    MutationPhase::FailedBeforeClear,
                    Some(error.to_string()),
                )?;
                self.mutations.remove(&intent);
                return Err(error);
            }
        };
        self.mutations.get_mut(&intent).unwrap().lifetime = Some(lock);
        if originals.is_empty() {
            // A temporary native store needs a lifetime share on known failure,
            // even when no cache owner existed before the destructive operation.
            let fallback = LeaseToken {
                service_epoch: self.epoch,
                id: self.next,
                owner: LeaseOwner {
                    epoch: controller,
                    store: intent,
                },
                profile,
                mode: LeaseMode::Shared,
            };
            let descriptor = open_file(&self.directory, &lock_name(profile))?;
            inspect_lock(&self.directory, &descriptor, &lock_name(profile), false)?;
            self.next = next;
            self.leases.insert(
                fallback.id,
                PhysicalLease {
                    token: fallback,
                    _lock: descriptor,
                    admission: None,
                    constructed: true,
                },
            );
            self.mutations
                .get_mut(&intent)
                .unwrap()
                .local
                .push(fallback);
        }
        // The durable intent precedes the compatibility sentinel. Neither permits
        // native execution until clear_dispatched is itself durably acknowledged.
        set_sentinel(
            self.mutations
                .get_mut(&intent)
                .unwrap()
                .lifetime
                .as_mut()
                .unwrap(),
        )?;
        Ok(token)
    }
    fn exact(&self, token: MutationToken) -> Result<&PhysicalMutation, LeaseError> {
        self.mutations
            .get(&token.intent)
            .filter(|v| v.token == token)
            .ok_or(LeaseError::StaleToken)
    }
    fn ensure_current(&self, token: MutationToken) -> Result<(), LeaseError> {
        let mutation = self.exact(token)?;
        inspect_lock(
            &self.directory,
            &mutation._admission,
            &admission_name(token.profile),
            false,
        )?;
        if let Some(lock) = &mutation.lifetime {
            inspect_lock(&self.directory, lock, &lock_name(token.profile), true)?;
            if mutation.record.phase != MutationPhase::Reserved
                && !mutation.record.phase.terminal()
                && lock
                    .metadata()
                    .map_err(|e| LeaseError::Io(e.to_string()))?
                    .len()
                    != QUARANTINE_SENTINEL.len() as u64
            {
                return Err(LeaseError::UnsafeIntent);
            }
        }
        if read(&self.directory, token.profile)?.as_ref() != Some(&mutation.record) {
            return Err(LeaseError::UnsafeIntent);
        }
        Ok(())
    }
    fn persist_current(&self, token: MutationToken) -> Result<(), LeaseError> {
        write(&self.directory, &self.exact(token)?.record)
    }
    pub(super) fn mutation_advance(
        &mut self,
        token: MutationToken,
        phase: MutationPhase,
    ) -> Result<(), LeaseError> {
        self.ensure_current(token)?;
        let mutation = self.exact(token)?;
        if mutation.record.phase == phase {
            return self.persist_current(token);
        }
        if phase.terminal()
            || phase == MutationPhase::CatalogCommitting
            || !mutation.record.phase.permits(phase)
        {
            return Err(LeaseError::StaleToken);
        }
        let lock = mutation
            .lifetime
            .as_ref()
            .ok_or(LeaseError::OutcomeUnknown)?;
        inspect_lock(&self.directory, lock, &lock_name(token.profile), true)?;
        if lock
            .metadata()
            .map_err(|e| LeaseError::Io(e.to_string()))?
            .len()
            != QUARANTINE_SENTINEL.len() as u64
        {
            return Err(LeaseError::UnsafeIntent);
        }
        let mut next = mutation.record.clone();
        next.phase = phase;
        write(&self.directory, &next)?;
        self.mutations.get_mut(&token.intent).unwrap().record = next;
        Ok(())
    }
    fn restore_local(&self, token: MutationToken) -> Result<(), LeaseError> {
        for original in &self.exact(token)?.local {
            let lease = self
                .leases
                .get(&original.id)
                .filter(|v| v.token == *original)
                .ok_or(LeaseError::StaleToken)?;
            lease
                ._lock
                .try_lock_shared()
                .map_err(|_| LeaseError::OutcomeUnknown)?;
            inspect_lock(
                &self.directory,
                &lease._lock,
                &lock_name(token.profile),
                true,
            )?;
        }
        Ok(())
    }
    fn terminal(
        &mut self,
        token: MutationToken,
        phase: MutationPhase,
        error: Option<String>,
    ) -> Result<(), LeaseError> {
        let mut next = self.exact(token)?.record.clone();
        if !next.phase.permits(phase) {
            return Err(LeaseError::StaleToken);
        }
        next.phase = phase;
        next.error = error;
        write(&self.directory, &next)?;
        self.mutations.get_mut(&token.intent).unwrap().record = next;
        Ok(())
    }
    pub(super) fn mutation_finish(
        &mut self,
        token: MutationToken,
        outcome: NativeMutationOutcome,
    ) -> Result<(), LeaseError> {
        self.ensure_current(token)?;
        let phase = self.exact(token)?.record.phase;
        if phase.terminal() {
            return self.release_terminal(token);
        }
        if !matches!(outcome, NativeMutationOutcome::Completed) {
            let (terminal, error) = match outcome {
                NativeMutationOutcome::AbortedBeforeClear { message }
                    if phase == MutationPhase::Reserved =>
                {
                    (MutationPhase::FailedBeforeClear, message)
                }
                NativeMutationOutcome::RemovalFailed { message }
                    if phase == MutationPhase::RemoveDispatched =>
                {
                    (MutationPhase::FailedPartial, message)
                }
                _ => return Err(LeaseError::StaleToken),
            };
            if error.len() > 1024 {
                return Err(LeaseError::Capacity);
            }
            if let Some(lock) = &self.exact(token)?.lifetime {
                lock.unlock().map_err(|e| LeaseError::Io(e.to_string()))?;
            }
            self.restore_local(token)?;
            self.terminal(token, terminal, Some(error))?;
        } else {
            if !matches!(
                phase,
                MutationPhase::RemoveCompleted | MutationPhase::CatalogCommitting
            ) {
                return Err(LeaseError::StaleToken);
            }
            let mut record = self.exact(token)?.record.clone();
            if phase == MutationPhase::RemoveCompleted {
                record.phase = MutationPhase::CatalogCommitting;
                write(&self.directory, &record)?;
                self.mutations.get_mut(&token.intent).unwrap().record = record.clone();
            }
            let directory =
                state_directory(&record.catalog).map_err(|e| LeaseError::Io(e.to_string()))?;
            if catalog_identity(&directory)? != (record.catalog_device, record.catalog_inode) {
                return Err(LeaseError::UnsafeIntent);
            }
            crate::persistence::commit_native_profile(
                &directory,
                token.profile,
                record.kind == MutationKind::Delete,
            )
            .map_err(|e| LeaseError::Io(e.to_string()))?;
            self.terminal(token, MutationPhase::Completed, None)?;
            let local = self.exact(token)?.local.clone();
            for original in local {
                self.leases.remove(&original.id);
            }
        }
        self.release_terminal(token)
    }
    fn release_terminal(&mut self, token: MutationToken) -> Result<(), LeaseError> {
        self.ensure_current(token)?;
        if !self.exact(token)?.record.phase.terminal() {
            return Err(LeaseError::UnsafeIntent);
        }
        // Exact known terminal receipt is durable before clearing the sentinel.
        // Failed syncing retains authority; this cleanup alone may be retried.
        if let Some(lock) = &self.exact(token)?.lifetime {
            inspect_lock(&self.directory, lock, &lock_name(token.profile), true)?;
            clear_sentinel(lock)?;
        }
        self.mutations.remove(&token.intent);
        Ok(())
    }
    pub(super) fn recover_intent_terminal(
        &self,
        profile: Uuid,
        intent: Uuid,
    ) -> Result<IntentStatus, LeaseError> {
        if self.mutations.values().any(|v| v.token.profile == profile) {
            return Err(LeaseError::Busy);
        }
        let record = read(&self.directory, profile)?
            .filter(|v| v.intent == intent && v.phase.terminal())
            .ok_or(LeaseError::StaleToken)?;
        recover_terminal(&self.directory, profile)?;
        let status = self.intent_status(profile, intent)?;
        if status.record != record || status.physical_cleanup_pending {
            return Err(LeaseError::UnsafeIntent);
        }
        Ok(status)
    }
    pub(super) fn intent_status(
        &self,
        profile: Uuid,
        intent: Uuid,
    ) -> Result<IntentStatus, LeaseError> {
        if profile.is_nil() || intent.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        let record = read(&self.directory, profile)?
            .filter(|r| r.intent == intent)
            .or_else(|| {
                self.mutations
                    .get(&intent)
                    .filter(|v| v.token.profile == profile)
                    .map(|v| v.record.clone())
            })
            .ok_or(LeaseError::StaleToken)?;
        let owner = self.mutations.get(&intent).filter(|v| {
            v.token.profile == profile
                && v.token.service_epoch == record.service_epoch
                && v.token.controller == record.controller
        });
        let original_owner_available = owner.is_some();
        let consistent = owner.is_none_or(|v| v.record == record);
        let terminal_cleanup_pending = if record.phase.terminal() && !original_owner_available {
            let lock = crate::persistence::private_file(
                &self.directory,
                std::ffi::OsStr::new(&lock_name(profile)),
                QUARANTINE_SENTINEL.len(),
                "browser profile lease",
            )
            .map_err(|_| LeaseError::UnsafeIntent)?;
            inspect_lock(&self.directory, &lock, &lock_name(profile), true)?;
            lock.metadata()
                .map_err(|e| LeaseError::Io(e.to_string()))?
                .len()
                != 0
        } else {
            false
        };
        Ok(IntentStatus {
            outcome_unknown: !record.phase.terminal() || !consistent,
            recovery_required: (!record.phase.terminal() && !original_owner_available)
                || !consistent,
            retry_native: false,
            physical_cleanup_pending: original_owner_available || terminal_cleanup_pending,
            original_owner_available,
            record,
        })
    }
}
