//! Bounded browser operation ownership. WebKit work stays on the native host; no page
//! receives a native bridge, and the socket cannot submit native lifecycle events.
//!
//! Admission resolves symbolic targets exactly once on the workspace executor. Native
//! dispatch drains each operation once. Host UUID, restore epoch, core navigation and
//! native document generation all fence callbacks. A deadline/cancellation cannot undo
//! a dispatched script/navigation, so failed mutating receipts report outcome uncertainty.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{
    Action, Snapshot, Surface, SurfaceId, SurfaceKind,
    targets::{CallerContext, TargetError, TargetScope},
};

pub const MAX_SCRIPT_BYTES: usize = 8192;
pub const MAX_INPUT_TEXT_BYTES: usize = 8192;
pub const MAX_SELECTOR_BYTES: usize = 2048;
pub const MAX_ATTRIBUTE_BYTES: usize = 128;
pub const MAX_SNAPSHOT_DEPTH: u32 = 32;
pub const MAX_OPERATION_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_OPERATION_TIMEOUT_MS: u64 = 2500;
pub const MAX_PENDING_BROWSER_OPERATIONS: usize = 8;
pub const MAX_BROWSER_RECEIPTS: usize = 16;
pub const MAX_BROWSER_RESULT_BYTES: usize = 64 * 1024;
pub const MAX_BROWSER_RETAINED_RESULT_BYTES: usize = 256 * 1024;
pub const MAX_OBSERVED_TITLE_BYTES: usize = 4096;
pub const MAX_OBSERVED_URL_BYTES: usize = 8192;
pub const MIN_PAGE_ZOOM: f64 = 0.25;
pub const MAX_PAGE_ZOOM: f64 = 5.0;
fn default_timeout() -> u64 {
    DEFAULT_OPERATION_TIMEOUT_MS
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    pub action: BrowserAction,
}
impl BrowserRequest {
    pub fn validate(&self) -> Result<(), BrowserError> {
        if !(1..=MAX_OPERATION_TIMEOUT_MS).contains(&self.timeout_ms) {
            return Err(BrowserError::Invalid(
                "browser timeout must be 1..30000 milliseconds",
            ));
        }
        self.action.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserAction {
    Back {
        #[serde(default)]
        snapshot_after: bool,
    },
    Forward {
        #[serde(default)]
        snapshot_after: bool,
    },
    Reload {
        #[serde(default)]
        snapshot_after: bool,
    },
    Zoom {
        operation: BrowserZoom,
    },
    Fill {
        selector: String,
        text: String,
        #[serde(default)]
        snapshot_after: bool,
    },
    Eval {
        script: String,
    },
    Snapshot {
        #[serde(default)]
        options: SnapshotOptions,
    },
    Get {
        kind: GetKind,
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        attribute: Option<String>,
        #[serde(default)]
        property: Option<String>,
    },
    Find {
        locator: BrowserLocator,
    },
    Wait {
        selector: String,
        #[serde(default)]
        state: WaitState,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserZoom {
    In,
    Out,
    Reset,
    Set { factor: f64 },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorKind {
    Role,
    #[default]
    Text,
    Label,
    Placeholder,
    Alt,
    Title,
    Testid,
    First,
    Last,
    Nth,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserLocator {
    pub kind: LocatorKind,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub exact: bool,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub index: Option<u32>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitState {
    Attached,
    Detached,
    #[default]
    Visible,
    Hidden,
    Enabled,
    Disabled,
    Checked,
    Unchecked,
}
impl BrowserLocator {
    pub fn validate(&self) -> Result<(), BrowserError> {
        let ordered = matches!(
            self.kind,
            LocatorKind::First | LocatorKind::Last | LocatorKind::Nth
        );
        if ordered {
            if self.query.is_some() || self.name.is_some() || self.exact || self.selector.is_none()
            {
                return Err(BrowserError::Invalid(
                    "first/last/nth require only a selector and nth index",
                ));
            }
            selector(self.selector.as_deref())?;
        } else {
            if self.selector.is_some()
                || self.index.is_some()
                || (self.kind != LocatorKind::Role && self.name.is_some())
            {
                return Err(BrowserError::Invalid("semantic locator options conflict"));
            }
            let query = self
                .query
                .as_deref()
                .ok_or(BrowserError::Invalid("locator query is required"))?;
            selector(Some(query))?;
            if self.kind == LocatorKind::Role
                && (query.len() > MAX_ATTRIBUTE_BYTES
                    || !query
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-'))
            {
                return Err(BrowserError::Invalid(
                    "role must be a token of at most 128 bytes",
                ));
            }
            if let Some(name) = &self.name {
                selector(Some(name))?;
            }
        }
        if self.kind == LocatorKind::Nth {
            if self.index.is_none_or(|index| index >= 4096) {
                return Err(BrowserError::Invalid("nth index must be 0..4095"));
            }
        } else if self.index.is_some() {
            return Err(BrowserError::Invalid("index is accepted only for nth"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotOptions {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub cursor: bool,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub max_depth: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GetKind {
    Url,
    Title,
    Text,
    Html,
    Value,
    Attr,
    Count,
    Box,
    Styles,
    Visible,
    Enabled,
    Checked,
}
impl BrowserAction {
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            Self::Snapshot { .. } | Self::Get { .. } | Self::Find { .. } | Self::Wait { .. }
        )
    }
    fn is_navigation(&self) -> bool {
        matches!(
            self,
            Self::Back { .. } | Self::Forward { .. } | Self::Reload { .. }
        )
    }
    pub fn validate(&self) -> Result<(), BrowserError> {
        match self {
            Self::Fill {
                selector: value,
                text,
                ..
            } => {
                selector(Some(value))?;
                if text.len() > MAX_INPUT_TEXT_BYTES || text.contains('\0') {
                    return Err(BrowserError::Invalid(
                        "browser fill text must contain no NUL and fit 8192 UTF-8 bytes",
                    ));
                }
            }
            Self::Zoom {
                operation: BrowserZoom::Set { factor },
            } if !factor.is_finite() => {
                return Err(BrowserError::Invalid("browser zoom factor must be finite"));
            }
            Self::Find { locator } => locator.validate()?,
            Self::Wait {
                selector: value, ..
            } => selector(Some(value))?,
            Self::Eval { script }
                if script.trim().is_empty()
                    || script.len() > MAX_SCRIPT_BYTES
                    || script.contains('\0') =>
            {
                return Err(BrowserError::Invalid(
                    "browser script must be nonempty, contain no NUL and fit 8192 UTF-8 bytes",
                ));
            }
            Self::Snapshot { options } => {
                selector(options.selector.as_deref())?;
                if options
                    .max_depth
                    .is_some_and(|value| value > MAX_SNAPSHOT_DEPTH)
                {
                    return Err(BrowserError::Invalid("snapshot max_depth must be 0..32"));
                }
            }
            Self::Get {
                kind,
                selector: value,
                attribute,
                property,
            } => {
                selector(value.as_deref())?;
                if matches!(
                    kind,
                    GetKind::Value
                        | GetKind::Attr
                        | GetKind::Count
                        | GetKind::Box
                        | GetKind::Styles
                        | GetKind::Visible
                        | GetKind::Enabled
                        | GetKind::Checked
                ) && value.is_none()
                {
                    return Err(BrowserError::Invalid(
                        "this browser get kind requires a selector",
                    ));
                }
                if matches!(kind, GetKind::Url | GetKind::Title) && value.is_some() {
                    return Err(BrowserError::Invalid("URL/title do not accept a selector"));
                }
                if *kind == GetKind::Attr {
                    name(
                        attribute
                            .as_deref()
                            .ok_or(BrowserError::Invalid("attribute name is required"))?,
                    )?;
                } else if attribute.is_some() {
                    return Err(BrowserError::Invalid(
                        "attribute is accepted only for get attr",
                    ));
                }
                if let Some(property) = property {
                    if *kind != GetKind::Styles {
                        return Err(BrowserError::Invalid(
                            "property is accepted only for get styles",
                        ));
                    }
                    name(property)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
}
fn selector(value: Option<&str>) -> Result<(), BrowserError> {
    if value.is_some_and(|value| {
        value.trim().is_empty() || value.len() > MAX_SELECTOR_BYTES || value.contains('\0')
    }) {
        return Err(BrowserError::Invalid(
            "selector must be nonempty, contain no NUL and fit 2048 UTF-8 bytes",
        ));
    }
    Ok(())
}
fn name(value: &str) -> Result<(), BrowserError> {
    if value.is_empty() || value.len() > MAX_ATTRIBUTE_BYTES || value.chars().any(char::is_control)
    {
        return Err(BrowserError::Invalid(
            "attribute/property must fit 128 UTF-8 bytes and contain no control characters",
        ));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("{0}")]
    Target(#[from] TargetError),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("target surface is not a browser")]
    NotBrowser,
    #[error(
        "browser native host is not available for the current document; retry explicitly after it mounts"
    )]
    Unavailable,
    #[error("browser operation queue is full")]
    QueueFull,
    #[error("browser operation receipt expired or does not exist")]
    NotFound,
    #[error("browser operation identity space exhausted")]
    Exhausted,
    #[error("native browser owner or document generation is stale")]
    StaleOwner,
    #[error("native browser host is already attached; detach it before replacement")]
    HostConflict,
    #[error("native browser operation was not dispatched or already finished")]
    NotDispatched,
    #[error("native browser result exceeds its size/depth budget")]
    ResultTooLarge,
}
impl BrowserError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Target(error) => error.code(),
            Self::Invalid(_) => "invalid_browser_request",
            Self::NotBrowser => "not_browser",
            Self::Unavailable => "browser_unavailable",
            Self::QueueFull => "browser_queue_full",
            Self::NotFound => "browser_operation_not_found",
            Self::Exhausted => "browser_ids_exhausted",
            Self::StaleOwner => "stale_browser_owner",
            Self::HostConflict => "browser_host_conflict",
            Self::NotDispatched => "browser_not_dispatched",
            Self::ResultTooLarge => "browser_result_too_large",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserOwner {
    pub epoch: Uuid,
    pub identity_namespace: Uuid,
    pub surface_id: SurfaceId,
    pub host_id: Uuid,
    pub document_generation: u64,
    pub model_navigation: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserFailure {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub outcome_unknown: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserOperationState {
    Pending,
    Completed,
    Failed,
}
#[derive(Clone, Debug, Serialize)]
pub struct BrowserReceipt {
    pub operation_id: u64,
    pub owner: BrowserOwner,
    pub state: BrowserOperationState,
    pub result: Option<Value>,
    pub error: Option<BrowserFailure>,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeBrowserRequest {
    pub operation_id: u64,
    pub owner: BrowserOwner,
    pub action: BrowserAction,
    pub timeout_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct BrowserDispatchBatch {
    pub epoch: Uuid,
    pub requests: Vec<NativeBrowserRequest>,
}
/// Only the trusted native bridge accepts this type. There is intentionally no
/// local-control command which can register hosts, change generations or complete work.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserHostEvent {
    Attach {
        epoch: Uuid,
        surface_id: SurfaceId,
        host_id: Uuid,
        document_generation: u64,
        model_navigation: u64,
    },
    Detach {
        owner: BrowserOwner,
    },
    DocumentChanged {
        owner: BrowserOwner,
        document_generation: u64,
        model_navigation: u64,
        #[serde(default)]
        caused_by: Option<u64>,
    },
    Observed {
        owner: BrowserOwner,
        url: String,
        title: String,
    },
    Complete {
        operation_id: u64,
        owner: BrowserOwner,
        result: Value,
    },
    Fail {
        operation_id: u64,
        owner: BrowserOwner,
        error: BrowserFailure,
    },
}
struct Operation {
    receipt: BrowserReceipt,
    action: BrowserAction,
    deadline: Instant,
    dispatched: bool,
    result_bytes: usize,
}
pub struct BrowserRuntime {
    epoch: Uuid,
    hosts: HashMap<SurfaceId, BrowserOwner>,
    operations: Vec<Operation>,
    next_id: u64,
}
impl Default for BrowserRuntime {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4(),
            hosts: HashMap::new(),
            operations: Vec::new(),
            next_id: 1,
        }
    }
}
fn surface(model: &Snapshot, id: SurfaceId) -> Option<&Surface> {
    model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .find(|surface| surface.id == id && surface.kind == SurfaceKind::Browser)
}
impl BrowserRuntime {
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }
    pub fn receipts(&self) -> Vec<BrowserReceipt> {
        self.operations
            .iter()
            .map(|op| op.receipt.clone())
            .collect()
    }
    pub fn pending_request_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|op| op.receipt.state == BrowserOperationState::Pending && !op.dispatched)
            .count()
    }
    pub fn receipt(&self, id: u64) -> Result<&BrowserReceipt, BrowserError> {
        self.operations
            .iter()
            .find(|op| op.receipt.operation_id == id)
            .map(|op| &op.receipt)
            .ok_or(BrowserError::NotFound)
    }
    pub fn admit(
        &mut self,
        model: &Snapshot,
        request: BrowserRequest,
        now: Instant,
    ) -> Result<u64, BrowserError> {
        self.reconcile(model, now);
        request.validate()?;
        if let Some(expected) = request.expected_model_revision
            && expected != model.revision
        {
            return Err(TargetError::StaleRevision {
                expected,
                actual: model.revision,
            }
            .into());
        }
        let target = request.scope.resolve(model, &request.caller)?;
        let surface = surface(model, target.surface).ok_or(BrowserError::NotBrowser)?;
        let owner = self
            .hosts
            .get(&target.surface)
            .filter(|owner| {
                owner.model_navigation == surface.navigation
                    && owner.identity_namespace == model.identity_namespace
            })
            .cloned()
            .ok_or(BrowserError::Unavailable)?;
        if self
            .operations
            .iter()
            .filter(|op| op.receipt.state == BrowserOperationState::Pending)
            .count()
            >= MAX_PENDING_BROWSER_OPERATIONS
        {
            return Err(BrowserError::QueueFull);
        }
        if self.operations.len() == MAX_BROWSER_RECEIPTS {
            self.evict_finished()?;
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(BrowserError::Exhausted)?;
        self.operations.push(Operation {
            receipt: BrowserReceipt {
                operation_id: id,
                owner,
                state: BrowserOperationState::Pending,
                result: None,
                error: None,
            },
            action: request.action,
            deadline: now + Duration::from_millis(request.timeout_ms),
            dispatched: false,
            result_bytes: 0,
        });
        Ok(id)
    }
    fn evict_finished(&mut self) -> Result<(), BrowserError> {
        let index = self
            .operations
            .iter()
            .position(|op| op.receipt.state != BrowserOperationState::Pending)
            .ok_or(BrowserError::QueueFull)?;
        self.operations.remove(index);
        Ok(())
    }
    pub fn drain_requests(&mut self, model: &Snapshot, now: Instant) -> BrowserDispatchBatch {
        self.reconcile(model, now);
        let requests = self
            .operations
            .iter_mut()
            .filter(|op| op.receipt.state == BrowserOperationState::Pending && !op.dispatched)
            .map(|op| {
                op.dispatched = true;
                NativeBrowserRequest {
                    operation_id: op.receipt.operation_id,
                    owner: op.receipt.owner.clone(),
                    action: op.action.clone(),
                    timeout_ms: op
                        .deadline
                        .saturating_duration_since(now)
                        .as_millis()
                        .max(1) as u64,
                }
            })
            .collect();
        BrowserDispatchBatch {
            epoch: self.epoch,
            requests,
        }
    }
    pub fn reconcile(&mut self, model: &Snapshot, now: Instant) -> bool {
        let mut dirty = false;
        self.hosts.retain(|id, owner| {
            owner.identity_namespace == model.identity_namespace && surface(model, *id).is_some()
        });
        for op in &mut self.operations {
            if op.receipt.state != BrowserOperationState::Pending {
                continue;
            }
            let reason = if now >= op.deadline {
                Some(("browser_timeout", "browser operation deadline elapsed"))
            } else if self.hosts.get(&op.receipt.owner.surface_id) != Some(&op.receipt.owner)
                || surface(model, op.receipt.owner.surface_id)
                    .is_none_or(|surface| surface.navigation != op.receipt.owner.model_navigation)
            {
                Some((
                    "stale_browser_owner",
                    "browser was closed, replaced or navigated before completion",
                ))
            } else {
                None
            };
            if let Some((code, message)) = reason {
                fail_operation(op, code, message, false);
                dirty = true;
            }
        }
        dirty
    }
    pub fn reset(&mut self, reason: &str) {
        for op in &mut self.operations {
            if op.receipt.state == BrowserOperationState::Pending {
                fail_operation(op, "browser_cancelled", reason, false);
            }
        }
        self.hosts.clear();
        self.epoch = Uuid::new_v4();
    }
    pub fn host_event(
        &mut self,
        model: &Snapshot,
        event: BrowserHostEvent,
        now: Instant,
    ) -> Result<Option<Action>, BrowserError> {
        // DocumentChanged may bridge an intentionally changed core navigation, so
        // reconcile only after validating and adopting that trusted native transition.
        let mut action = None;
        match event {
            BrowserHostEvent::Observed { owner, url, title } => {
                self.check_owner(&owner)?;
                if owner.identity_namespace != model.identity_namespace
                    || surface(model, owner.surface_id)
                        .is_none_or(|surface| surface.navigation != owner.model_navigation)
                {
                    return Err(BrowserError::StaleOwner);
                }
                if title.len() > MAX_OBSERVED_TITLE_BYTES || url.len() > MAX_OBSERVED_URL_BYTES {
                    return Err(BrowserError::Invalid(
                        "browser observation exceeds the bounded title or URL size",
                    ));
                }
                // Validation and application share the workspace executor.
                // Reuse its URL/title normalization without issuing navigation.
                action = Some(Action::ObserveBrowser {
                    surface_id: owner.surface_id,
                    url,
                    title,
                });
            }
            BrowserHostEvent::Attach {
                epoch,
                surface_id,
                host_id,
                document_generation,
                model_navigation,
            } => {
                let surface = surface(model, surface_id).ok_or(BrowserError::NotBrowser)?;
                if epoch != self.epoch
                    || host_id.is_nil()
                    || document_generation == 0
                    || model_navigation != surface.navigation
                {
                    return Err(BrowserError::StaleOwner);
                }
                if let Some(previous) = self.hosts.get(&surface_id) {
                    if previous.host_id != host_id {
                        return Err(BrowserError::HostConflict);
                    }
                    if document_generation < previous.document_generation
                        || (document_generation == previous.document_generation
                            && model_navigation != previous.model_navigation)
                    {
                        return Err(BrowserError::StaleOwner);
                    }
                }
                self.hosts.insert(
                    surface_id,
                    BrowserOwner {
                        epoch,
                        identity_namespace: model.identity_namespace,
                        surface_id,
                        host_id,
                        document_generation,
                        model_navigation,
                    },
                );
            }
            BrowserHostEvent::Detach { owner } => {
                self.check_owner(&owner)?;
                self.hosts.remove(&owner.surface_id);
            }
            BrowserHostEvent::DocumentChanged {
                owner,
                document_generation,
                model_navigation,
                caused_by,
            } => {
                self.check_owner(&owner)?;
                if document_generation <= owner.document_generation
                    || surface(model, owner.surface_id)
                        .is_none_or(|surface| surface.navigation != model_navigation)
                {
                    return Err(BrowserError::StaleOwner);
                }
                let mut next = owner.clone();
                next.document_generation = document_generation;
                next.model_navigation = model_navigation;
                if let Some(id) = caused_by
                    && let Some(op) = self
                        .operations
                        .iter_mut()
                        .find(|op| op.receipt.operation_id == id)
                    && op.receipt.state == BrowserOperationState::Pending
                    && op.dispatched
                    && op.action.is_navigation()
                    && op.receipt.owner == owner
                    && now < op.deadline
                {
                    op.receipt.owner = next.clone();
                }
                self.hosts.insert(owner.surface_id, next);
            }
            BrowserHostEvent::Complete {
                operation_id,
                owner,
                result,
            } => {
                self.reconcile(model, now);
                self.check_owner(&owner)?;
                let size = result_size(&result).ok_or(BrowserError::ResultTooLarge);
                if size.is_err() {
                    let op = self.pending_dispatched(operation_id, &owner)?;
                    fail_operation(
                        op,
                        "browser_result_too_large",
                        "browser result exceeds its size/depth budget",
                        false,
                    );
                    return Err(BrowserError::ResultTooLarge);
                }
                let size = size.expect("result size checked");
                self.pending_dispatched(operation_id, &owner)?;
                while self
                    .operations
                    .iter()
                    .map(|op| op.result_bytes)
                    .sum::<usize>()
                    + size
                    > MAX_BROWSER_RETAINED_RESULT_BYTES
                {
                    self.evict_finished()?;
                }
                let op = self.pending_dispatched(operation_id, &owner)?;
                op.receipt.state = BrowserOperationState::Completed;
                op.receipt.result = Some(result);
                op.result_bytes = size;
            }
            BrowserHostEvent::Fail {
                operation_id,
                owner,
                error,
            } => {
                self.reconcile(model, now);
                self.check_owner(&owner)?;
                let op = self.pending_dispatched(operation_id, &owner)?;
                fail_operation(op, &error.code, &error.message, error.outcome_unknown);
            }
        }
        self.reconcile(model, now);
        Ok(action)
    }
    pub(crate) fn check_owner(&self, owner: &BrowserOwner) -> Result<(), BrowserError> {
        if owner.epoch != self.epoch || self.hosts.get(&owner.surface_id) != Some(owner) {
            return Err(BrowserError::StaleOwner);
        }
        Ok(())
    }
    fn pending_dispatched(
        &mut self,
        id: u64,
        owner: &BrowserOwner,
    ) -> Result<&mut Operation, BrowserError> {
        let op = self
            .operations
            .iter_mut()
            .find(|op| op.receipt.operation_id == id)
            .ok_or(BrowserError::NotFound)?;
        if op.receipt.owner != *owner {
            return Err(BrowserError::StaleOwner);
        }
        if op.receipt.state != BrowserOperationState::Pending || !op.dispatched {
            return Err(BrowserError::NotDispatched);
        }
        Ok(op)
    }
}
fn fail_operation(op: &mut Operation, code: &str, message: &str, unknown: bool) {
    op.receipt.state = BrowserOperationState::Failed;
    op.receipt.error = Some(BrowserFailure {
        code: code
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .take(64)
            .collect(),
        message: message.chars().take(512).collect(),
        outcome_unknown: unknown || (op.dispatched && !op.action.is_read()),
    });
}
fn result_size(value: &Value) -> Option<usize> {
    // Count JSON bytes without allocating another serialized copy. Values arrive
    // through a separately bounded native ABI, so this check also bounds retention.
    fn visit(value: &Value, depth: usize, bytes: &mut usize) -> Option<()> {
        if depth > 64 {
            return None;
        }
        let add = match value {
            Value::Null => 4,
            Value::Bool(_) => 5,
            Value::Number(value) => value.to_string().len(),
            Value::String(value) => serde_json::to_string(value).ok()?.len(),
            Value::Array(values) => {
                for value in values {
                    visit(value, depth + 1, bytes)?;
                }
                values.len() + 2
            }
            Value::Object(values) => {
                for (key, value) in values {
                    *bytes = bytes.checked_add(serde_json::to_string(key).ok()?.len() + 1)?;
                    visit(value, depth + 1, bytes)?;
                }
                values.len() + 2
            }
        };
        *bytes = bytes.checked_add(add)?;
        (*bytes <= MAX_BROWSER_RESULT_BYTES).then_some(())
    }
    let mut bytes = 0;
    visit(value, 0, &mut bytes)?;
    Some(bytes)
}
