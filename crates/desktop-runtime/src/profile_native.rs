//! Bounded shared native-operation rendezvous. No filesystem or WebKit work runs
//! under this short mutex or on the core executor. Native effects are drained once.
use crate::profile_leases::{
    LeaseError, LeaseService, MAX_MUTATIONS,
    journal::{IntentStatus, MutationKind},
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct NativeProfileRequest {
    pub controller: Uuid,
    pub intent: Uuid,
    pub profile: Uuid,
    pub kind: MutationKind,
}
struct Operation {
    request: NativeProfileRequest,
    dispatched: bool,
    outcome: Option<Result<(), String>>,
}
pub struct NativeProfileOperations {
    pub controller: Uuid,
    pub catalog: PathBuf,
    pub service: Arc<LeaseService>,
    operations: Mutex<VecDeque<Operation>>,
}
impl NativeProfileOperations {
    pub fn new(
        controller: Uuid,
        catalog: PathBuf,
        service: Arc<LeaseService>,
    ) -> Result<Self, LeaseError> {
        if controller.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        Ok(Self {
            controller,
            catalog,
            service,
            operations: Mutex::new(VecDeque::new()),
        })
    }
    pub(crate) fn admit(&self, profile: Uuid, kind: MutationKind) -> Result<Uuid, LeaseError> {
        let mut operations = self.operations.try_lock().map_err(|_| LeaseError::Busy)?;
        if operations.len() >= MAX_MUTATIONS {
            return Err(LeaseError::Capacity);
        }
        if operations.iter().any(|v| v.request.profile == profile) {
            return Err(LeaseError::Busy);
        }
        let intent = Uuid::new_v4();
        operations.push_back(Operation {
            request: NativeProfileRequest {
                controller: self.controller,
                intent,
                profile,
                kind,
            },
            dispatched: false,
            outcome: None,
        });
        Ok(intent)
    }
    pub fn request(&self, intent: Uuid) -> Result<NativeProfileRequest, LeaseError> {
        self.operations
            .lock()
            .map_err(|_| LeaseError::Closed)?
            .iter()
            .find(|v| v.request.intent == intent && v.dispatched && v.outcome.is_none())
            .map(|v| v.request.clone())
            .ok_or(LeaseError::StaleToken)
    }
    /// Response capacity must be reserved before this once-only dequeue. Status
    /// never returns another dispatch authorization after acknowledgment loss.
    pub fn drain(&self) -> Result<Vec<NativeProfileRequest>, LeaseError> {
        let mut operations = self.operations.try_lock().map_err(|_| LeaseError::Busy)?;
        let mut requests = Vec::new();
        for operation in operations
            .iter_mut()
            .filter(|v| !v.dispatched && v.outcome.is_none())
        {
            operation.dispatched = true;
            requests.push(operation.request.clone());
        }
        Ok(requests)
    }
    /// Trusted native completion only. Success requires the service's durable
    /// terminal receipt and physical cleanup; callers cannot synthesize it.
    pub fn complete(&self, intent: Uuid, result: Result<(), String>) -> Result<(), LeaseError> {
        let profile = {
            let operations = self.operations.lock().map_err(|_| LeaseError::Closed)?;
            operations
                .iter()
                .find(|v| v.request.intent == intent && v.dispatched)
                .ok_or(LeaseError::StaleToken)?
                .request
                .profile
        };
        if result.is_err() {
            match self.service.intent_status(profile, intent) {
                Ok(status)
                    if status.record.controller == self.controller
                        && matches!(
                            status.record.phase,
                            crate::profile_leases::journal::MutationPhase::FailedBeforeClear
                                | crate::profile_leases::journal::MutationPhase::FailedPartial
                        )
                        && !status.outcome_unknown
                        && !status.physical_cleanup_pending => {}
                Err(LeaseError::StaleToken) => (), // Native preflight rejected before durable admission.
                _ => return Err(LeaseError::OutcomeUnknown),
            }
        }
        if result.is_ok() {
            let status = self.service.intent_status(profile, intent)?;
            if status.record.controller != self.controller
                || status.record.phase != crate::profile_leases::journal::MutationPhase::Completed
                || status.outcome_unknown
                || status.physical_cleanup_pending
            {
                return Err(LeaseError::OutcomeUnknown);
            }
        }
        let mut operations = self.operations.lock().map_err(|_| LeaseError::Closed)?;
        let operation = operations
            .iter_mut()
            .find(|v| v.request.intent == intent && v.dispatched)
            .ok_or(LeaseError::StaleToken)?;
        if operation.outcome.is_some() {
            return Err(LeaseError::StaleToken);
        }
        operation.outcome = Some(result.map_err(|e| e.chars().take(1024).collect()));
        Ok(())
    }
    pub(crate) fn outcomes(&self) -> Vec<(Uuid, Result<(), String>)> {
        let Ok(operations) = self.operations.try_lock() else {
            return Vec::new();
        };
        operations
            .iter()
            .filter_map(|v| v.outcome.clone().map(|outcome| (v.request.intent, outcome)))
            .collect()
    }
    pub(crate) fn retire(&self, intent: Uuid) {
        if let Ok(mut operations) = self.operations.try_lock() {
            operations.retain(|v| v.request.intent != intent || v.outcome.is_none());
        }
    }
    pub(crate) fn fenced(&self, profile: Uuid) -> bool {
        self.operations
            .try_lock()
            .map_or(true, |v| v.iter().any(|op| op.request.profile == profile))
    }
    pub(crate) fn pending(&self) -> usize {
        self.operations
            .try_lock()
            .map_or(MAX_MUTATIONS, |v| v.len())
    }
    pub(crate) fn cancel_queued(&self) {
        if let Ok(mut operations) = self.operations.try_lock() {
            for operation in operations
                .iter_mut()
                .filter(|v| !v.dispatched && v.outcome.is_none())
            {
                operation.outcome = Some(Err(
                    "desktop shutdown before native dispatch; website data unchanged".into(),
                ));
            }
        }
    }
    pub fn lookup(&self, profile: Uuid, intent: Uuid) -> Result<serde_json::Value, LeaseError> {
        match self.service.intent_status(profile, intent) {
            Ok(status) => Ok(serde_json::json!(status)),
            Err(LeaseError::StaleToken) => {
                let operations = self.operations.lock().map_err(|_| LeaseError::Closed)?;
                let operation = operations
                    .iter()
                    .find(|v| v.request.profile == profile && v.request.intent == intent)
                    .ok_or(LeaseError::StaleToken)?;
                Ok(
                    serde_json::json!({"profile": profile, "intent": intent, "kind": operation.request.kind, "phase": "native_preflight_pending", "original_owner_available": true, "outcome_unknown": operation.dispatched, "recovery_required": false, "retry_native": false, "physical_cleanup_pending": true, "durable": false}),
                )
            }
            Err(error) => Err(error),
        }
    }
    /// Original-intent status remains valid without selected-catalog membership.
    /// Call off the core/UI executors; this is bounded worker I/O.
    pub fn status(&self, profile: Uuid, intent: Uuid) -> Result<IntentStatus, LeaseError> {
        self.service.intent_status(profile, intent)
    }
}
