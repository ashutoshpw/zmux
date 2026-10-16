//! Explicit metadata/receipt reconciliation. This queue never dispatches WebKit.
use super::*;
use crate::profile_leases::journal::MutationPhase;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ReconcileReceipt {
    pub id: Uuid,
    pub profile: Uuid,
    pub intent: Uuid,
    pub state: String,
    pub error: Option<String>,
    pub original_phase: Option<MutationPhase>,
}
#[derive(Default)]
pub(super) struct Reconciliations(pub VecDeque<(ReconcileReceipt, bool)>);
impl NativeProfileOperations {
    pub fn drain_batch(&self) -> Result<serde_json::Value, LeaseError> {
        let mut operations = self.operations.try_lock().map_err(|_| LeaseError::Busy)?;
        let mut entries = self
            .reconciliations
            .try_lock()
            .map_err(|_| LeaseError::Busy)?;
        let mut requests = Vec::new();
        let mut reconciliations = Vec::new();
        for operation in operations
            .iter_mut()
            .filter(|v| !v.dispatched && v.outcome.is_none())
        {
            operation.dispatched = true;
            requests.push(operation.request.clone());
        }
        for (receipt, dispatched) in &mut entries.0 {
            if receipt.state == "pending" && !*dispatched {
                *dispatched = true;
                reconciliations.push(receipt.clone());
            }
        }
        Ok(serde_json::json!({"requests": requests, "reconciliations": reconciliations}))
    }
    pub fn request_reconcile(
        &self,
        profile: Uuid,
        intent: Uuid,
    ) -> Result<ReconcileReceipt, LeaseError> {
        if profile.is_nil() || intent.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        let mut entries = self
            .reconciliations
            .try_lock()
            .map_err(|_| LeaseError::Busy)?;
        if let Some((receipt, dispatched)) = entries
            .0
            .iter_mut()
            .find(|(v, _)| v.profile == profile && v.intent == intent && v.state == "pending")
        {
            // Only an explicit caller re-delivers this metadata-only request.
            // The native owner coalesces a running original reconciliation.
            *dispatched = false;
            return Ok(receipt.clone());
        }
        if entries.0.len() == MAX_MUTATIONS {
            let index = entries
                .0
                .iter()
                .position(|(v, _)| v.state != "pending")
                .ok_or(LeaseError::Capacity)?;
            entries.0.remove(index);
        }
        let receipt = ReconcileReceipt {
            id: Uuid::new_v4(),
            profile,
            intent,
            state: "pending".into(),
            error: None,
            original_phase: None,
        };
        entries.0.push_back((receipt.clone(), false));
        Ok(receipt)
    }
    pub fn reconciliation_status(&self, id: Uuid) -> Result<ReconcileReceipt, LeaseError> {
        self.reconciliations
            .try_lock()
            .map_err(|_| LeaseError::Busy)?
            .0
            .iter()
            .find(|(v, _)| v.id == id)
            .map(|(v, _)| v.clone())
            .ok_or(LeaseError::StaleToken)
    }
    pub fn drain_reconciliations(&self) -> Result<Vec<ReconcileReceipt>, LeaseError> {
        let mut entries = self
            .reconciliations
            .try_lock()
            .map_err(|_| LeaseError::Busy)?;
        let mut requests = Vec::new();
        for (receipt, dispatched) in &mut entries.0 {
            if receipt.state == "pending" && !*dispatched {
                *dispatched = true;
                requests.push(receipt.clone());
            }
        }
        Ok(requests)
    }
    pub fn finish_reconciliation(&self, id: Uuid, error: Option<String>) -> Result<(), LeaseError> {
        let receipt = self.reconciliation_status(id)?;
        let (phase, error) = if let Some(error) = error {
            (None, Some(error))
        } else {
            let status = self
                .service
                .intent_status(receipt.profile, receipt.intent)?;
            if !status.record.phase.terminal()
                || status.outcome_unknown
                || status.physical_cleanup_pending
            {
                return Err(LeaseError::OutcomeUnknown);
            }
            let phase = status.record.phase;
            let error = if phase == MutationPhase::Completed {
                None
            } else {
                Some(format!(
                    "original operation {}: {}",
                    phase.code(),
                    status.record.error.unwrap_or_else(|| {
                        "known native failure; website data may already be cleared".into()
                    })
                ))
            };
            (Some(phase), error)
        };
        let mut entries = self
            .reconciliations
            .lock()
            .map_err(|_| LeaseError::Closed)?;
        let (receipt, dispatched) = entries
            .0
            .iter_mut()
            .find(|(v, _)| v.id == id && v.state == "pending")
            .ok_or(LeaseError::StaleToken)?;
        if !*dispatched {
            return Err(LeaseError::StaleToken);
        }
        receipt.state = if error.is_some() {
            "failed"
        } else {
            "completed"
        }
        .into();
        receipt.error = error.map(|e| e.chars().take(1024).collect());
        receipt.original_phase = phase;
        Ok(())
    }
    pub(crate) fn pending_reconciliations(&self) -> usize {
        self.reconciliations.try_lock().map_or(MAX_MUTATIONS, |v| {
            v.0.iter().filter(|(r, _)| r.state == "pending").count()
        })
    }
    /// An original destructive queue entry may already have been retired after
    /// a lost finish response. Durable physical proof remains authoritative.
    pub fn complete_if_pending(
        &self,
        intent: Uuid,
        result: Result<(), String>,
    ) -> Result<(), LeaseError> {
        let pending = self
            .operations
            .lock()
            .map_err(|_| LeaseError::Closed)?
            .iter()
            .any(|v| v.request.intent == intent && v.outcome.is_none());
        if pending {
            self.complete(intent, result)?;
        }
        Ok(())
    }
}
