//! Durable local agent inbox. Text is data; this owner never writes terminal input.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zmux_workspace_core::targets::CallerContext;

pub const MAX_BODY_BYTES: usize = 4096;
pub const MAX_PAGE: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub request_id: Uuid,
    #[serde(default)]
    pub caller: CallerContext,
    pub action: MessageAction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageAction {
    Send {
        target: Option<String>,
        reply_to: Option<Uuid>,
        body: String,
        sender_name: Option<String>,
        thread_id: Option<Uuid>,
    },
    List {
        surface: Option<String>,
        #[serde(default)]
        states: Vec<MessageState>,
        #[serde(default = "default_limit")]
        limit: usize,
        cursor: Option<u64>,
    },
    MarkRead {
        ids: Vec<Uuid>,
    },
    Settings {
        #[serde(default)]
        scope: SettingScope,
        target: Option<String>,
        enabled: Option<bool>,
    },
    Register {
        surface: String,
        provider: String,
        interactive: bool,
    },
    Poll {
        surface: String,
        consumer: Uuid,
        token: String,
        held: Option<bool>,
    },
    Claim {
        surface: String,
        consumer: Uuid,
        token: String,
    },
    Ack {
        surface: String,
        consumer: Uuid,
        token: String,
        lease_id: Uuid,
        message_ids: Vec<Uuid>,
    },
    ReadTurn {
        surface: String,
        consumer: Uuid,
        token: String,
        ids: Vec<Uuid>,
    },
}
fn default_limit() -> usize {
    MAX_PAGE
}
impl MessageAction {
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            Self::List { .. } | Self::Settings { enabled: None, .. }
        )
    }
    pub fn requires_full_result(&self) -> bool {
        matches!(
            self,
            Self::Register { .. }
                | Self::Poll { .. }
                | Self::Claim { .. }
                | Self::Ack { .. }
                | Self::ReadTurn { .. }
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageState {
    Queued,
    Delivered,
    Read,
    Failed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingScope {
    #[default]
    Surface,
    Workspace,
    Global,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub outcome_unknown: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationReceipt {
    pub request_id: Uuid,
    pub state: String,
    pub result: Option<Value>,
    pub error: Option<Failure>,
    #[serde(skip)]
    pub requires_full: bool,
    #[serde(skip)]
    pub(crate) exposure: Option<(u64, Uuid, std::time::Instant)>,
}

mod domain;
mod native;
pub use native::{NativeAction, NativeAdmissionError, NativeOrigin, NativeRequest};
mod storage;
use crate::{
    DesktopError,
    control_policy::{ControlAccess, ControlAdmission},
};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};
use zmux_workspace_core::Snapshot;

pub(crate) const JOURNAL_NAME: &str = "agent-messages.json";
pub(crate) const LOCK_NAME: &str = "agent-messages.lock";
pub(crate) fn is_reserved_name(name: &OsStr) -> bool {
    name == JOURNAL_NAME || name == LOCK_NAME
}
impl OperationReceipt {
    fn pending(id: Uuid) -> Self {
        Self {
            request_id: id,
            state: "pending".into(),
            result: None,
            error: None,
            requires_full: false,
            exposure: None,
        }
    }
    fn failed(id: Uuid, code: &str, message: impl ToString, unknown: bool) -> Self {
        Self {
            request_id: id,
            state: "failed".into(),
            result: None,
            error: Some(Failure {
                code: code.into(),
                message: message.to_string(),
                outcome_unknown: unknown,
            }),
            requires_full: false,
            exposure: None,
        }
    }
}
#[derive(Default)]
struct LiveView {
    model: Snapshot,
    closed: HashSet<Uuid>,
    cancelled: HashSet<Uuid>,
    generation: u64,
}
enum JobKind {
    Request(Box<MessageRequest>),
    Status(Uuid),
    Reconcile(Uuid),
}
struct Job {
    kind: JobKind,
    admission: Option<ControlAdmission>,
    full: bool,
    model: Snapshot,
}
struct Completion {
    receipt: OperationReceipt,
    internal: bool,
}
pub(crate) struct Runtime {
    sender: Option<SyncSender<Job>>,
    receiver: Receiver<Completion>,
    worker: Option<JoinHandle<()>>,
    live: Arc<Mutex<LiveView>>,
    receipts: HashMap<Uuid, OperationReceipt>,
    pending: HashSet<Uuid>,
    inputs: HashMap<Uuid, String>,
    stopping: bool,
    dirty: bool,
    error: Option<String>,
}
impl Runtime {
    pub fn start(directory: Option<PathBuf>) -> Result<Self, DesktopError> {
        let (tx, rx) = mpsc::sync_channel(8);
        let (done_tx, done_rx) = mpsc::sync_channel(8);
        let live = Arc::new(Mutex::new(LiveView::default()));
        let worker = if let Some(directory) = directory {
            let view = Arc::clone(&live);
            Some(
                thread::Builder::new()
                    .name("zmux-agent-messages".into())
                    .spawn(move || storage::run(directory, rx, done_tx, view))
                    .map_err(|e| DesktopError::Failure(format!("start message worker: {e}")))?,
            )
        } else {
            None
        };
        Ok(Self {
            sender: worker.as_ref().map(|_| tx),
            receiver: done_rx,
            worker,
            live,
            receipts: HashMap::new(),
            pending: HashSet::new(),
            inputs: HashMap::new(),
            stopping: false,
            dirty: false,
            error: None,
        })
    }
    pub fn observe(&mut self, model: Snapshot, closed: Vec<Uuid>) {
        if let Ok(mut view) = self.live.lock() {
            if view.model.identity_namespace != model.identity_namespace
                || domain::owner_generation(&view.model) != domain::owner_generation(&model)
            {
                view.generation = view.generation.wrapping_add(1);
            }
            view.model = model;
            self.dirty |= !closed.is_empty();
            view.closed.extend(closed);
        }
    }
    /// Called immediately before committing a canonical model action. Close overload
    /// refuses that removal, retaining every unreconciled identity without blocking input.
    pub fn commit_model(
        &mut self,
        previous: &Snapshot,
        next: &Snapshot,
    ) -> Result<(), DesktopError> {
        if self.sender.is_none() || previous.identity_namespace != next.identity_namespace {
            return Ok(());
        }
        let current: HashSet<_> = next
            .target_catalog()
            .into_iter()
            .filter(|o| o.kind == zmux_workspace_core::targets::TargetKind::Surface)
            .map(|o| o.uuid)
            .collect();
        let removed: Vec<_> = previous
            .target_catalog()
            .into_iter()
            .filter(|o| {
                o.kind == zmux_workspace_core::targets::TargetKind::Surface
                    && !current.contains(&o.uuid)
            })
            .map(|o| o.uuid)
            .collect();
        let count = {
            let view = self
                .live
                .lock()
                .map_err(|_| DesktopError::Failure("message live view unavailable".into()))?;
            view.closed
                .union(&removed.iter().copied().collect())
                .count()
        };
        if count > 512 {
            return Err(DesktopError::Failure(
                "message closure reconciliation is full (512); retry close after worker completion"
                    .into(),
            ));
        }
        self.observe(next.clone(), removed);
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(mut done) = self.receiver.try_recv() {
            self.pending.remove(&done.receipt.request_id);
            self.inputs.remove(&done.receipt.request_id);
            if done.internal {
                if done.receipt.state == "failed" {
                    self.dirty = true;
                    self.error = done.receipt.error.as_ref().map(|e| e.message.clone());
                }
                changed = true;
                continue;
            }
            if let Some((generation, lease, deadline)) = done.receipt.exposure {
                let mut view = self.live.lock().expect("message live view mutex poisoned");
                if generation != view.generation || std::time::Instant::now() >= deadline {
                    view.cancelled.insert(lease);
                    self.dirty = true;
                    done.receipt = OperationReceipt::failed(
                        done.receipt.request_id,
                        "handoff_cancelled",
                        "lease expired or workspace, held state, or policy changed before context exposure",
                        false,
                    );
                }
            }
            if self.receipts.len() >= 384
                && !self.receipts.contains_key(&done.receipt.request_id)
                && let Some(id) = self
                    .receipts
                    .keys()
                    .find(|id| !self.pending.contains(id))
                    .copied()
            {
                self.receipts.remove(&id);
            }
            self.receipts.insert(done.receipt.request_id, done.receipt);
            changed = true;
        }
        if self.dirty && !self.stopping && self.pending.len() < 8 {
            let id = Uuid::new_v4();
            if self.sender.as_ref().is_some_and(|sender| {
                sender
                    .try_send(Job {
                        kind: JobKind::Reconcile(id),
                        admission: None,
                        full: true,
                        model: Snapshot::default(),
                    })
                    .is_ok()
            }) {
                self.pending.insert(id);
                self.dirty = false;
            }
        }
        changed
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn admit(
        &mut self,
        request: MessageRequest,
        admission: Option<ControlAdmission>,
    ) -> Result<OperationReceipt, DesktopError> {
        let encoded = crate::persistence::serialize_bounded(
            &request,
            crate::control::MAX_CONTROL_REQUEST_BYTES,
            "agent request",
        )
        .map_err(|e| DesktopError::Failure(e.to_string()))?;
        if let MessageAction::Send { body, .. } = &request.action {
            domain::validate_body(body).map_err(DesktopError::Failure)?;
        }
        if matches!(
            request.action,
            MessageAction::Settings {
                enabled: Some(_),
                ..
            } | MessageAction::Register { .. }
                | MessageAction::Poll { held: Some(_), .. }
        ) && let Ok(mut view) = self.live.lock()
        {
            view.generation = view.generation.wrapping_add(1);
        }
        let hash = domain::digest(&encoded);
        if self.pending.contains(&request.request_id) {
            return if self.inputs.get(&request.request_id) == Some(&hash) {
                Ok(OperationReceipt::pending(request.request_id))
            } else {
                Err(DesktopError::Failure(
                    "request ID reused with conflicting pending input".into(),
                ))
            };
        }
        let id = request.request_id;
        let receipt = self.submit(JobKind::Request(Box::new(request)), admission)?;
        self.inputs.insert(id, hash);
        Ok(receipt)
    }
    pub fn status(
        &mut self,
        id: Uuid,
        admission: Option<ControlAdmission>,
    ) -> Result<OperationReceipt, DesktopError> {
        self.poll();
        if self.pending.contains(&id) {
            return Ok(OperationReceipt::pending(id));
        }
        if let Some(receipt) = self.receipts.get(&id) {
            if receipt.state == "awaiting_handoff" {
                return self.submit(JobKind::Status(id), admission);
            }
            let full = Self::full(&admission)?;
            if receipt.requires_full && !full {
                return Err(DesktopError::Failure(
                    "full socket access required for consumer operation result".into(),
                ));
            }
            let receipt = receipt.clone();
            if let Some((generation, lease, deadline)) = receipt.exposure {
                self.receipts.remove(&id);
                let mut view = self
                    .live
                    .lock()
                    .map_err(|_| DesktopError::Failure("message live view unavailable".into()))?;
                if generation != view.generation || std::time::Instant::now() >= deadline {
                    view.cancelled.insert(lease);
                    self.dirty = true;
                    return Ok(OperationReceipt::failed(
                        id,
                        "handoff_cancelled",
                        "lease expired or workspace, held state, or policy changed before context exposure",
                        false,
                    ));
                }
            }
            if receipt.state == "completed" {
                self.receipts.remove(&id);
            }
            return Ok(receipt);
        }
        self.submit(JobKind::Status(id), admission)
    }
    fn full(admission: &Option<ControlAdmission>) -> Result<bool, DesktopError> {
        admission
            .as_ref()
            .map(|a| {
                a.try_check()
                    .map(|g| g.policy().access() == ControlAccess::Full)
            })
            .transpose()
            .map(|v| v.unwrap_or(true))
            .map_err(Into::into)
    }
    fn submit(
        &mut self,
        kind: JobKind,
        admission: Option<ControlAdmission>,
    ) -> Result<OperationReceipt, DesktopError> {
        let id = match &kind {
            JobKind::Request(r) => r.request_id,
            JobKind::Status(id) | JobKind::Reconcile(id) => *id,
        };
        if self.stopping {
            return Err(DesktopError::Closed);
        }
        let sender = self.sender.as_ref().ok_or_else(|| {
            DesktopError::Failure("agent messages require durable runtime state".into())
        })?;
        if self.pending.len() >= 8 {
            return Err(DesktopError::Failure(
                "agent message worker capacity reached (8)".into(),
            ));
        }
        if self.pending.contains(&id) {
            return Err(DesktopError::Failure(
                "message operation already pending; query its request ID".into(),
            ));
        }
        let full = Self::full(&admission)?;
        let model = self
            .live
            .lock()
            .map_err(|_| DesktopError::Failure("message live view unavailable".into()))?
            .model
            .clone();
        sender
            .try_send(Job {
                kind,
                admission,
                full,
                model,
            })
            .map_err(|_| {
                DesktopError::Failure("agent message worker is unavailable or full".into())
            })?;
        self.pending.insert(id);
        self.receipts.remove(&id);
        Ok(OperationReceipt::pending(id))
    }
    pub fn poll_shutdown(&mut self) -> Result<bool, String> {
        self.stopping = true;
        self.sender.take();
        self.poll();
        if self.worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return Ok(false);
        }
        if let Some(w) = self.worker.take() {
            w.join()
                .map_err(|_| "agent message worker panicked".to_string())?;
        }
        self.poll();
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
