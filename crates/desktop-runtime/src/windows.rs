//! Bounded native main-window requests. Topology admission and AppKit completion
//! are distinct; a missing acknowledgement never authorizes replay.
use crate::{Command, DesktopError, Worker};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_workspace_core::{
    Action, Snapshot, WindowId,
    targets::{CallerContext, TargetKind, TargetScope},
};

const MAX_OPERATIONS: usize = 32;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default)]
    pub expected_model_revision: Option<u64>,
    pub action: WindowAction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WindowAction {
    List,
    Current,
    New {
        #[serde(default = "default_focus")]
        focus: bool,
    },
    Focus,
    Close,
}
fn default_focus() -> bool {
    true
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Open,
    Focus,
    Close,
    Terminate,
}
#[derive(Clone, Debug, Serialize)]
pub struct WindowReceipt {
    pub epoch: Uuid,
    pub operation_id: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub state: String,
    pub kind: WindowKind,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeWindowRequest {
    pub epoch: Uuid,
    pub operation_id: Uuid,
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub host_id: Option<Uuid>,
    pub kind: WindowKind,
    pub focus: bool,
    #[serde(skip)]
    deadline: Option<Instant>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowHostToken {
    pub epoch: Uuid,
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub host_id: Uuid,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeWindowEvent {
    Complete {
        token: WindowHostToken,
        operation_id: Uuid,
    },
    Failed {
        epoch: Uuid,
        operation_id: Uuid,
        message: String,
    },
    Key {
        token: WindowHostToken,
        key: bool,
    },
}
pub(crate) struct WindowRuntime {
    pub epoch: Uuid,
    namespace: Option<Uuid>,
    hosts: HashMap<WindowId, Uuid>,
    seen: HashSet<WindowId>,
    pub observed_key: Option<WindowId>,
    requests: Vec<NativeWindowRequest>,
    receipts: VecDeque<WindowReceipt>,
    terminating: bool,
}
impl Default for WindowRuntime {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4(),
            namespace: None,
            hosts: HashMap::new(),
            seen: HashSet::new(),
            observed_key: None,
            requests: Vec::new(),
            receipts: VecDeque::new(),
            terminating: false,
        }
    }
}
fn failure(message: impl Into<String>) -> DesktopError {
    DesktopError::Failure(message.into())
}
impl WindowRuntime {
    pub fn requests(&self) -> Vec<NativeWindowRequest> {
        self.requests.clone()
    }
    pub fn receipts(&self) -> Vec<WindowReceipt> {
        self.receipts.iter().cloned().collect()
    }
    pub fn host(&self, window: WindowId) -> Option<Uuid> {
        self.hosts.get(&window).copied()
    }
    pub fn terminating(&self) -> bool {
        self.terminating
    }
    fn presentation(&self, window: WindowId) -> (&'static str, Option<&str>) {
        let failure = self
            .receipts
            .iter()
            .rev()
            .find(|receipt| receipt.window_id == window && receipt.state == "failed")
            .and_then(|receipt| receipt.error.as_deref());
        let state = if self.host(window).is_some() {
            "open"
        } else if self.requests.iter().any(|request| {
            request.window_id == window
                && request.kind == WindowKind::Open
                && request.deadline.is_some()
        }) {
            "pending"
        } else {
            "failed"
        };
        (state, failure)
    }
    pub fn hosts(&self) -> Vec<WindowHostToken> {
        self.hosts
            .iter()
            .map(|(window, host)| WindowHostToken {
                epoch: self.epoch,
                identity_namespace: self.namespace.unwrap_or_default(),
                window_id: *window,
                host_id: *host,
            })
            .collect()
    }
    pub(crate) fn request(
        &mut self,
        model: &Snapshot,
        window: WindowId,
        kind: WindowKind,
        focus: bool,
    ) -> Result<WindowReceipt, DesktopError> {
        if let Some(existing) = self
            .requests
            .iter()
            .find(|r| r.window_id == window && r.kind == kind)
        {
            return self
                .receipts
                .iter()
                .find(|r| r.operation_id == existing.operation_id)
                .cloned()
                .ok_or_else(|| failure("window receipt missing"));
        }
        while self.receipts.len() >= MAX_OPERATIONS {
            let index = self
                .receipts
                .iter()
                .position(|r| {
                    !self
                        .requests
                        .iter()
                        .any(|pending| pending.operation_id == r.operation_id)
                })
                .ok_or_else(|| failure("native window request capacity is full"))?;
            self.receipts.remove(index);
        }
        let id = Uuid::new_v4();
        if kind == WindowKind::Terminate {
            self.terminating = true;
        }
        let receipt = WindowReceipt {
            epoch: self.epoch,
            operation_id: id,
            window_id: window,
            window_uuid: model.object_uuid(TargetKind::Window, window.0),
            state: "pending".into(),
            kind,
            error: None,
        };
        self.requests.push(NativeWindowRequest {
            epoch: self.epoch,
            operation_id: id,
            identity_namespace: model.identity_namespace,
            window_id: window,
            window_uuid: model.object_uuid(TargetKind::Window, window.0),
            host_id: self.host(window),
            kind,
            focus,
            deadline: Some(Instant::now() + Duration::from_secs(30)),
        });
        self.receipts.push_back(receipt.clone());
        Ok(receipt)
    }
    pub fn reconcile(&mut self, model: &Snapshot) -> Result<bool, DesktopError> {
        let mut dirty = false;
        if self
            .namespace
            .is_some_and(|namespace| namespace != model.identity_namespace)
        {
            *self = Self::default();
            dirty = true;
        }
        self.namespace = Some(model.identity_namespace);
        self.seen.retain(|id| {
            model.windows.iter().any(|window| window.id == *id)
                || self.hosts.contains_key(id)
                || self.requests.iter().any(|request| request.window_id == *id)
        });
        for window in &model.windows {
            if self.seen.insert(window.id) {
                self.request(
                    model,
                    window.id,
                    WindowKind::Open,
                    model.focused_window == Some(window.id),
                )?;
                dirty = true;
            }
        }
        for request in &mut self.requests {
            if request
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                request.deadline = None;
                if let Some(receipt) = self
                    .receipts
                    .iter_mut()
                    .find(|r| r.operation_id == request.operation_id)
                {
                    receipt.state = "failed".into();
                    receipt.error =
                        Some("native window acknowledgement timed out; outcome is unknown".into());
                    dirty = true;
                }
            }
        }
        Ok(dirty)
    }
    fn event(
        &mut self,
        model: &Snapshot,
        event: NativeWindowEvent,
    ) -> Result<Option<WindowId>, DesktopError> {
        match event {
            NativeWindowEvent::Failed {
                epoch,
                operation_id,
                message,
            } => {
                if epoch != self.epoch
                    || message.len() > 512
                    || message.chars().any(char::is_control)
                {
                    return Err(failure("invalid native window failure"));
                }
                let index = self
                    .requests
                    .iter()
                    .position(|r| r.operation_id == operation_id)
                    .ok_or_else(|| failure("native window request is stale"))?;
                self.requests.remove(index);
                let receipt = self
                    .receipts
                    .iter_mut()
                    .find(|r| r.operation_id == operation_id)
                    .ok_or_else(|| failure("native window receipt is stale"))?;
                receipt.state = "failed".into();
                receipt.error = Some(message);
            }
            NativeWindowEvent::Key { token, key } => {
                self.check_token(&token)?;
                if !model
                    .windows
                    .iter()
                    .any(|window| window.id == token.window_id)
                {
                    return Err(failure("native key window is no longer live"));
                }
                if key {
                    self.observed_key = Some(token.window_id);
                    return Ok(Some(token.window_id));
                }
                if self.observed_key == Some(token.window_id) {
                    self.observed_key = None;
                }
            }
            NativeWindowEvent::Complete {
                token,
                operation_id,
            } => {
                if token.epoch != self.epoch
                    || Some(token.identity_namespace) != self.namespace
                    || token.host_id.is_nil()
                {
                    return Err(failure("native window token is stale"));
                }
                let index = self
                    .requests
                    .iter()
                    .position(|r| r.operation_id == operation_id && r.window_id == token.window_id)
                    .ok_or_else(|| failure("native window request is stale"))?;
                let request = &self.requests[index];
                if request.kind == WindowKind::Open {
                    if !model
                        .windows
                        .iter()
                        .any(|window| window.id == token.window_id)
                        || self
                            .hosts
                            .get(&token.window_id)
                            .is_some_and(|host| *host != token.host_id)
                    {
                        return Err(failure(
                            "native window admission no longer belongs to this host",
                        ));
                    }
                    self.hosts.insert(token.window_id, token.host_id);
                } else if request.host_id.is_some() {
                    self.check_token(&token)?;
                }
                if request.kind == WindowKind::Close {
                    if model
                        .windows
                        .iter()
                        .any(|window| window.id == token.window_id)
                    {
                        return Err(failure("native closure preceded model admission"));
                    }
                    self.hosts.remove(&token.window_id);
                    if self.observed_key == Some(token.window_id) {
                        self.observed_key = None;
                    }
                }
                let closed = request.kind == WindowKind::Close;
                self.requests.remove(index);
                if closed {
                    // Physical closure also drains a conversion/materialization whose
                    // Open acknowledgement lost the race. It cannot remain charged
                    // forever or revive a host after this exact closure receipt.
                    let abandoned: Vec<_> = self
                        .requests
                        .iter()
                        .filter(|r| r.window_id == token.window_id)
                        .map(|r| r.operation_id)
                        .collect();
                    self.requests
                        .retain(|r| !abandoned.contains(&r.operation_id));
                    for receipt in self
                        .receipts
                        .iter_mut()
                        .filter(|r| abandoned.contains(&r.operation_id))
                    {
                        receipt.state = "failed".into();
                        receipt.error = Some(
                            "window closed before its native operation was acknowledged".into(),
                        );
                    }
                    self.seen.remove(&token.window_id);
                }
                if let Some(receipt) = self
                    .receipts
                    .iter_mut()
                    .find(|r| r.operation_id == operation_id)
                    && receipt.state == "pending"
                {
                    receipt.state = "completed".into();
                }
            }
        }
        Ok(None)
    }
    fn check_token(&self, token: &WindowHostToken) -> Result<(), DesktopError> {
        if token.epoch != self.epoch
            || Some(token.identity_namespace) != self.namespace
            || self.hosts.get(&token.window_id) != Some(&token.host_id)
        {
            return Err(failure("native window host token is stale"));
        }
        Ok(())
    }
}
impl Worker {
    pub(crate) fn window_command(&mut self, request: WindowRequest) -> Result<(), DesktopError> {
        let model = self.model.snapshot();
        if request
            .expected_model_revision
            .is_some_and(|revision| revision != model.revision)
        {
            return Err(failure("workspace model revision changed"));
        }
        self.windows.reconcile(&model)?;
        let result = match request.action {
            WindowAction::List => {
                json!({"windows": model.windows.iter().map(|window| {
                    let (state, error) = self.windows.presentation(window.id);
                    json!({"window_id":window.id,"window_uuid":model.object_uuid(TargetKind::Window,window.id.0),"workspace_ids":window.workspace_ids,"selected_workspace":window.selected_workspace,"native_host_id":self.windows.host(window.id),"native_state":state,"native_error":error,"recovery":if state=="failed" { Some("Close this window to discard it, or quit and reopen to retry native materialization; no automatic retry is performed.") } else { None },"observed_key":self.windows.observed_key==Some(window.id)})
                }).collect::<Vec<_>>(),"focused_window":model.focused_window,"observed_key_window":self.windows.observed_key})
            }
            WindowAction::Current => {
                let id = request.scope.resolve_window(&model, &request.caller)?;
                json!({"window_id":id,"window_uuid":model.object_uuid(TargetKind::Window,id.0),"native_host_id":self.windows.host(id),"observed_key":self.windows.observed_key==Some(id)})
            }
            WindowAction::New { focus } => {
                if self.windows.hosts.len()
                    + self
                        .windows
                        .requests
                        .iter()
                        .filter(|r| r.kind == WindowKind::Open)
                        .count()
                    >= zmux_workspace_core::MAX_WINDOWS
                {
                    return Err(failure(
                        "native window capacity remains charged until closure acknowledgement",
                    ));
                }
                let prior: HashSet<_> = model.windows.iter().map(|window| window.id).collect();
                self.action(Action::CreateWindow { focus })?;
                let next = self.model.snapshot();
                let id = next
                    .windows
                    .iter()
                    .find(|window| !prior.contains(&window.id))
                    .ok_or_else(|| failure("created window identity missing"))?
                    .id;
                self.windows.seen.insert(id);
                serde_json::to_value(self.windows.request(&next, id, WindowKind::Open, focus)?)
                    .map_err(|e| failure(e.to_string()))?
            }
            WindowAction::Focus | WindowAction::Close => {
                let id = request.scope.resolve_window(&model, &request.caller)?;
                if matches!(request.action, WindowAction::Focus) && self.windows.host(id).is_none()
                {
                    return Err(failure("native window host is not ready"));
                }
                let kind = match request.action {
                    WindowAction::Focus => {
                        self.action(Action::FocusWindow { window_id: id })?;
                        WindowKind::Focus
                    }
                    _ if model.windows.len() == 1 => WindowKind::Terminate,
                    _ => {
                        self.action(Action::CloseWindow { window_id: id })?;
                        WindowKind::Close
                    }
                };
                serde_json::to_value(self.windows.request(
                    &self.model.snapshot(),
                    id,
                    kind,
                    kind == WindowKind::Focus,
                )?)
                .map_err(|e| failure(e.to_string()))?
            }
        };
        self.window_result = Some(result);
        Ok(())
    }
    pub(crate) fn native_window_event(
        &mut self,
        event: NativeWindowEvent,
    ) -> Result<(), DesktopError> {
        if let Some(window_id) = self.windows.event(&self.model.snapshot(), event)? {
            self.model
                .apply(Action::FocusWindow { window_id })
                .map_err(|e| failure(e.to_string()))?;
            self.acknowledge_focused();
        }
        self.window_result = Some(json!({"accepted":true}));
        Ok(())
    }
    pub(crate) fn window_action_is_private(action: &Action) -> bool {
        matches!(
            action,
            Action::CreateWindow { .. } | Action::FocusWindow { .. } | Action::CloseWindow { .. }
        )
    }
    pub(crate) fn execute_window_command(&mut self, command: Command) -> Result<(), DesktopError> {
        match command {
            Command::Windows { request } => self.window_command(request),
            Command::NativeWindow { event } => self.native_window_event(event),
            _ => unreachable!(),
        }
    }
}
