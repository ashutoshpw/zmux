use super::{RequestReceipt, ScheduledJob, SchedulerResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoreState {
    pub version: u16,
    pub helper_instance_id: Uuid,
    pub jobs: Vec<ScheduledJob>,
    pub receipts: Vec<StoredReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pruned_before_utc: Option<String>,
}

impl StoreState {
    pub(super) fn new() -> Self {
        Self {
            version: 1,
            helper_instance_id: Uuid::new_v4(),
            jobs: Vec::new(),
            receipts: Vec::new(),
            pruned_before_utc: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredReceipt {
    pub fingerprint: String,
    pub receipt: RequestReceipt,
    pub response: SchedulerResponse,
    pub expires_at_unix_nanos: i128,
}
