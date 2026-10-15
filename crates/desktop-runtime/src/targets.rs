//! Control requests retain symbolic targets until the workspace executor accepts the operation.
use serde::{Deserialize, Serialize};
use zmux_workspace_core::{
    Snapshot,
    targets::{CallerContext, TargetAction, TargetError, TargetScope},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    /// Optional optimistic concurrency guard for callers acting on an observed catalog/index.
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    pub operation: TargetOperation,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetOperation {
    Action {
        action: TargetAction,
    },
    PasteFiles {
        paths: Vec<std::path::PathBuf>,
    },
    SendText {
        text: String,
    },
    Notify {
        title: String,
        body: String,
    },
    ReadScreen,
    #[default]
    ListPanes,
    ListPaneSurfaces,
}
impl TargetRequest {
    pub fn validate_revision(&self, model: &Snapshot) -> Result<(), TargetError> {
        if let Some(expected) = self.expected_model_revision
            && expected != model.revision
        {
            return Err(TargetError::StaleRevision {
                expected,
                actual: model.revision,
            });
        }
        Ok(())
    }
}
