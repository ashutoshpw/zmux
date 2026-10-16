use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_workspace_core::{
    SessionId, SurfaceKind,
    targets::{TargetKind, TargetScope},
};

pub(super) type Result<T> = std::result::Result<T, String>;
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn validate_body(body: &str) -> Result<()> {
    if body.is_empty()
        || body.len() > MAX_BODY_BYTES
        || body
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err("body must be 1..4096 UTF-8 bytes without unsafe controls".into());
    }
    Ok(())
}
fn monotonic_ms() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    u64::try_from(t.tv_sec)
        .expect("monotonic clock seconds are nonnegative")
        .saturating_mul(1000)
        + u64::try_from(t.tv_nsec).expect("monotonic nanoseconds are nonnegative") / 1_000_000
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Address {
    pub surface_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sender {
    pub surface_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    pub name: String,
    pub attribution: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    pub id: Uuid,
    pub request_id: Uuid,
    pub namespace: Uuid,
    pub thread_id: Uuid,
    pub reply_to: Option<Uuid>,
    pub sender: Sender,
    pub recipient: Address,
    pub body: String,
    pub sequence: u64,
    pub created_at: u64,
    pub state: MessageState,
    pub failure: Option<String>,
    pub delivered_consumer: Option<Uuid>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Setting {
    scope: SettingScope,
    owner: Option<Uuid>,
    enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Consumer {
    id: Uuid,
    namespace: Uuid,
    surface: Uuid,
    session: SessionId,
    epoch: Uuid,
    provider: String,
    interactive: bool,
    held: bool,
    token_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    id: Uuid,
    consumer: Uuid,
    surface: Uuid,
    ids: Vec<Uuid>,
    acked: bool,
    expires_monotonic_ms: u64,
    issued_monotonic_ms: u64,
    handed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: Uuid,
    input_hash: String,
    result: Value,
    full: bool,
    secret_lost: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u32,
    sequence: u64,
    messages: Vec<Message>,
    settings: Vec<Setting>,
    consumers: Vec<Consumer>,
    leases: Vec<Lease>,
    records: Vec<Record>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            version: 1,
            sequence: 0,
            messages: vec![],
            settings: vec![],
            consumers: vec![],
            leases: vec![],
            records: vec![],
        }
    }
}
#[derive(Clone)]
pub(super) struct LiveSurface {
    pub(super) address: Address,
    session: SessionId,
    focused: bool,
}
pub(super) fn surfaces(model: &Snapshot) -> Vec<LiveSurface> {
    model
        .workspaces
        .iter()
        .flat_map(|w| {
            w.root.panes().into_iter().flat_map(move |p| {
                p.surfaces.iter().filter_map(move |s| {
                    if s.kind != SurfaceKind::Terminal {
                        return None;
                    }
                    Some(LiveSurface {
                        address: Address {
                            surface_id: model.object_uuid(TargetKind::Surface, s.id.0),
                            workspace_id: model.object_uuid(TargetKind::Workspace, w.id.0),
                            workspace_name: w.name.clone(),
                        },
                        session: s.session_id?,
                        focused: w.focused_pane == p.id && p.selected_surface == s.id,
                    })
                })
            })
        })
        .collect()
}
pub(super) fn owner_generation(model: &Snapshot) -> Vec<(Uuid, Uuid, SessionId)> {
    surfaces(model)
        .into_iter()
        .map(|s| (s.address.surface_id, s.address.workspace_id, s.session))
        .collect()
}
fn explicit_surface(model: &Snapshot, target: &str) -> Result<LiveSurface> {
    let scope = TargetScope {
        surface: Some(
            target
                .parse()
                .map_err(|e: zmux_workspace_core::targets::TargetError| e.to_string())?,
        ),
        ..Default::default()
    };
    let resolved = scope
        .resolve(model, &CallerContext::default())
        .map_err(|e| e.to_string())?;
    let uuid = model.object_uuid(TargetKind::Surface, resolved.surface.0);
    surfaces(model)
        .into_iter()
        .find(|s| s.address.surface_id == uuid)
        .ok_or("target must be a live terminal".into())
}
fn workspace(model: &Snapshot, target: &str) -> Result<Uuid> {
    if let Ok(handle) = target.parse() {
        return model
            .resolve_order_workspace(&handle, None)
            .map(|(_, w)| model.object_uuid(TargetKind::Workspace, w.0))
            .map_err(|e| e.to_string());
    }
    let exact: Vec<_> = model
        .workspaces
        .iter()
        .filter(|w| w.name == target)
        .collect();
    let matches = if exact.is_empty() {
        model
            .workspaces
            .iter()
            .filter(|w| w.name.starts_with(target))
            .collect()
    } else {
        exact
    };
    if target.is_empty() || matches.len() != 1 {
        return Err("workspace title missing or ambiguous".into());
    }
    Ok(model.object_uuid(TargetKind::Workspace, matches[0].id.0))
}
fn caller(model: &Snapshot, c: &CallerContext, label: Option<&str>) -> Result<Sender> {
    if let Some(label) = label
        && (label.is_empty() || label.len() > 64 || label.chars().any(char::is_control))
    {
        return Err("sender name must be 1..64 UTF-8 bytes and single line".into());
    }
    let c = c.bind_local_tmux(model).map_err(|e| e.to_string())?;
    if c.surface.is_none() && c.workspace.is_none() && c.window.is_none() {
        return Ok(Sender {
            surface_id: None,
            workspace_id: None,
            workspace_name: None,
            name: label.unwrap_or("local user").into(),
            attribution: "external_local_user".into(),
        });
    }
    // Caller workspace/window fields are ambient hints. A surviving surface determines
    // its current owner, matching the shared caller contract after a live move.
    if c.surface.is_none()
        && c.workspace.is_none()
        && let Some(window) = &c.window
    {
        model.resolve_window(window).map_err(|e| e.to_string())?;
    }
    let resolved = TargetScope::default()
        .resolve(model, &c)
        .map_err(|e| e.to_string())?;
    let s = surfaces(model)
        .into_iter()
        .find(|s| {
            s.address.surface_id == model.object_uuid(TargetKind::Surface, resolved.surface.0)
        })
        .ok_or("sender must be a live terminal")?;
    let mut name = label.unwrap_or(&s.address.workspace_name).to_owned();
    while name.len() > 64 {
        name.pop();
    }
    Ok(Sender {
        surface_id: Some(s.address.surface_id),
        workspace_id: Some(s.address.workspace_id),
        name,
        workspace_name: Some(s.address.workspace_name),
        attribution: "local_surface_claim".into(),
    })
}
impl Journal {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.messages.len() > 128
            || self.settings.len() > 256
            || self.consumers.len() > 128
            || self.leases.len() > 128
            || self.records.len() > 384
        {
            return Err("invalid or oversized agent message journal".into());
        }
        let mut ids = HashSet::new();
        let mut sequences = HashSet::new();
        let mut requests = HashSet::new();
        for m in &self.messages {
            if m.id.is_nil()
                || !ids.insert(m.id)
                || m.sequence == 0
                || m.sequence > self.sequence
                || !sequences.insert(m.sequence)
                || !requests.insert(m.request_id)
                || m.request_id.is_nil()
                || m.thread_id.is_nil()
                || m.namespace.is_nil()
                || m.recipient.surface_id.is_nil()
                || m.recipient.workspace_id.is_nil()
                || m.body.is_empty()
                || m.body.len() > MAX_BODY_BYTES
                || m.body
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
                || m.sender.name.is_empty()
                || m.sender.name.len() > 64
                || m.sender.name.chars().any(char::is_control)
            {
                return Err("invalid message identity or body in journal".into());
            }
        }
        let mut ids = HashSet::new();
        if self
            .records
            .iter()
            .any(|r| r.id.is_nil() || !ids.insert(r.id))
        {
            return Err("duplicate or nil request evidence".into());
        }
        if self
            .messages
            .iter()
            .filter(|m| matches!(m.state, MessageState::Queued | MessageState::Delivered))
            .any(|m| !self.records.iter().any(|r| r.id == m.request_id))
        {
            return Err("unsettled message missing request evidence".into());
        }
        let mut owners = HashSet::new();
        for s in &self.settings {
            if (s.scope == SettingScope::Global) != s.owner.is_none()
                || s.owner.is_some_and(|id| id.is_nil())
                || !owners.insert((s.scope as u8, s.owner))
            {
                return Err("invalid or duplicate message setting".into());
            }
        }
        let mut consumers = HashSet::new();
        let mut surfaces = HashSet::new();
        for c in &self.consumers {
            if c.id.is_nil()
                || c.namespace.is_nil()
                || c.surface.is_nil()
                || c.epoch.is_nil()
                || c.session.0 == 0
                || !consumers.insert(c.id)
                || !surfaces.insert(c.surface)
                || c.provider.is_empty()
                || c.provider.len() > 64
                || c.provider.chars().any(char::is_control)
                || c.token_hash.len() != 64
                || !c.token_hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("invalid or duplicate consumer registration".into());
            }
        }
        let mut leases = HashSet::new();
        let mut leased = HashSet::new();
        for l in &self.leases {
            let mut exact = HashSet::new();
            if l.id.is_nil()
                || !leases.insert(l.id)
                || l.ids.is_empty()
                || l.ids.len() > 8
                || l.surface.is_nil()
                || l.consumer.is_nil()
                || l.ids
                    .iter()
                    .any(|id| !exact.insert(*id) || !l.acked && !leased.insert(*id))
            {
                return Err("invalid or duplicate message lease".into());
            }
        }
        if self.records.iter().any(|r| {
            r.input_hash.len() != 64 || !r.input_hash.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err("invalid retained request hash".into());
        }
        Ok(())
    }
    fn enabled(&self, address: &Address) -> bool {
        self.settings.iter().all(|s| {
            s.enabled
                || !(s.scope == SettingScope::Global
                    || s.scope == SettingScope::Surface && s.owner == Some(address.surface_id)
                    || s.scope == SettingScope::Workspace && s.owner == Some(address.workspace_id))
        })
    }
    fn registered(&self, s: &LiveSurface, epoch: Uuid) -> bool {
        self.consumers.iter().any(|c| {
            c.surface == s.address.surface_id
                && c.session == s.session
                && c.epoch == epoch
                && c.interactive
        })
    }
    fn recipient(&self, model: &Snapshot, target: &str, epoch: Uuid) -> Result<LiveSurface> {
        if !target.starts_with("workspace:")
            && let Ok(s) = explicit_surface(model, target)
        {
            return Ok(s);
        }
        let id = workspace(model, target)?;
        let candidates: Vec<_> = surfaces(model)
            .into_iter()
            .filter(|s| s.address.workspace_id == id)
            .collect();
        let registered: Vec<_> = candidates
            .iter()
            .filter(|s| self.registered(s, epoch))
            .collect();
        if let Some(s) = registered.iter().find(|s| s.focused) {
            return Ok((*s).clone());
        }
        if registered.len() == 1 {
            return Ok(registered[0].clone());
        }
        if registered.len() > 1 {
            return Err("multiple registered terminals; supply an explicit surface".into());
        }
        candidates
            .into_iter()
            .find(|s| s.focused)
            .ok_or("workspace has no focused live terminal".into())
    }
    pub fn reconcile(
        &mut self,
        closed: &HashSet<Uuid>,
        deadlines: &HashMap<Uuid, Instant>,
        cancelled: &HashSet<Uuid>,
    ) {
        let current = Instant::now();
        let system_monotonic = monotonic_ms();
        let expired: HashSet<_> = self
            .leases
            .iter()
            .filter(|l| {
                !l.acked
                    && (!deadlines.get(&l.id).map(|d| *d > current).unwrap_or(
                        l.issued_monotonic_ms <= system_monotonic
                            && l.expires_monotonic_ms > system_monotonic,
                    ) || !self.consumers.iter().any(|c| c.id == l.consumer))
            })
            .map(|l| l.id)
            .collect();
        self.leases.retain(|l| {
            !expired.contains(&l.id) && !cancelled.contains(&l.id) && !closed.contains(&l.surface)
        });
        self.consumers.retain(|c| !closed.contains(&c.surface));
        let active: HashSet<_> = self
            .leases
            .iter()
            .filter(|l| !l.acked && l.handed)
            .flat_map(|l| l.ids.iter().copied())
            .collect();
        let disabled: HashSet<_> = self
            .messages
            .iter()
            .filter(|m| !self.enabled(&m.recipient))
            .map(|m| m.id)
            .collect();
        for m in &mut self.messages {
            if m.state == MessageState::Queued
                && (closed.contains(&m.recipient.surface_id)
                    || disabled.contains(&m.id) && !active.contains(&m.id))
            {
                m.state = MessageState::Failed;
                m.failure = Some(
                    if closed.contains(&m.recipient.surface_id) {
                        "recipient_closed"
                    } else {
                        "messaging_disabled"
                    }
                    .into(),
                );
            }
        }
    }
    pub fn status(
        &self,
        id: Uuid,
        full: bool,
        ephemeral: &HashMap<Uuid, Value>,
    ) -> Result<Option<(Value, bool)>> {
        let Some(r) = self.records.iter().find(|r| r.id == id) else {
            return Ok(None);
        };
        if r.full && !full {
            return Err("full socket access required for consumer operation result".into());
        }
        if r.secret_lost {
            return ephemeral.get(&id).cloned().map(|v|Some((v,r.full))).ok_or("consumer capability unavailable after runtime restart; explicitly register a new generation".into());
        }
        Ok(Some((r.result.clone(), r.full)))
    }
    pub fn cancel_lease(&mut self, id: Uuid) {
        self.leases.retain(|l| l.id != id);
    }
    pub fn retain_runtime_maps(
        &self,
        deadlines: &mut HashMap<Uuid, Instant>,
        ephemeral: &mut HashMap<Uuid, Value>,
    ) {
        deadlines.retain(|id, _| self.leases.iter().any(|l| l.id == *id));
        ephemeral.retain(|id, _| self.records.iter().any(|r| r.id == *id && r.secret_lost));
    }
    pub fn handoff(
        &mut self,
        result: &Value,
        model: &Snapshot,
        epoch: Uuid,
        deadlines: &HashMap<Uuid, Instant>,
    ) -> Result<Option<Uuid>> {
        if !result.get("messages").is_some_and(Value::is_array) {
            return Ok(None);
        }
        let Some(id) = result.get("lease_id").and_then(Value::as_str) else {
            return Ok(None);
        };
        let id = id
            .parse::<Uuid>()
            .map_err(|_| "invalid retained lease identity")?;
        let lease = self
            .leases
            .iter()
            .find(|l| l.id == id)
            .ok_or("claim lease no longer exists")?
            .clone();
        let consumer = self
            .consumers
            .iter()
            .find(|c| {
                c.id == lease.consumer
                    && c.epoch == epoch
                    && c.namespace == model.identity_namespace
            })
            .ok_or("claim consumer generation superseded or restarted")?;
        let surface = explicit_surface(model, &consumer.surface.to_string())?;
        if surface.session != consumer.session
            || consumer.held
            || !consumer.interactive
            || deadlines.get(&id).is_none_or(|d| *d <= Instant::now())
        {
            return Err("claim consumer held, session changed, or lease expired".into());
        }
        if !self.enabled(&surface.address)
            || lease.ids.iter().any(|id| {
                self.messages
                    .iter()
                    .any(|m| m.id == *id && !self.enabled(&m.recipient))
            })
        {
            return Err("messaging disabled before context exposure".into());
        }
        if !lease.handed
            && (!self.enabled(&surface.address)
                || lease.ids.iter().any(|id| {
                    self.messages.iter().any(|m| {
                        m.id == *id
                            && (!self.enabled(&m.recipient) || m.state != MessageState::Queued)
                    })
                }))
        {
            return Err("claim disabled or closed before handoff".into());
        }
        self.leases
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("claim lease disappeared")?
            .handed = true;
        Ok(Some(id))
    }
    fn consumer(
        &self,
        model: &Snapshot,
        surface: &str,
        id: Uuid,
        token: &str,
        epoch: Uuid,
    ) -> Result<Consumer> {
        let s = explicit_surface(model, surface)?;
        let c = self
            .consumers
            .iter()
            .find(|c| {
                c.id == id
                    && c.surface == s.address.surface_id
                    && c.namespace == model.identity_namespace
                    && c.session == s.session
                    && c.epoch == epoch
                    && c.token_hash == digest(token.as_bytes())
            })
            .ok_or("stale or invalid consumer capability")?;
        Ok(c.clone())
    }
    pub fn validate_current(
        &self,
        request: &MessageRequest,
        admitted: &Snapshot,
        current: &Snapshot,
        result: &Value,
    ) -> Result<()> {
        if admitted.identity_namespace != current.identity_namespace {
            return Err("desktop namespace changed after message admission".into());
        }
        let target = match &request.action {
            MessageAction::Send { .. } => result
                .get("message")
                .and_then(|m| m.get("recipient"))
                .and_then(|r| r.get("surface_id"))
                .and_then(Value::as_str),
            MessageAction::Register { surface, .. }
            | MessageAction::Poll { surface, .. }
            | MessageAction::Claim { surface, .. }
            | MessageAction::Ack { surface, .. }
            | MessageAction::ReadTurn { surface, .. } => Some(surface.as_str()),
            _ => None,
        };
        if let Some(target) = target {
            let pinned = explicit_surface(admitted, target)?;
            let live = explicit_surface(current, &pinned.address.surface_id.to_string())?;
            if live.session != pinned.session {
                return Err("target session changed after message admission".into());
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        request: &MessageRequest,
        model: &Snapshot,
        epoch: Uuid,
        deadlines: &mut HashMap<Uuid, Instant>,
        ephemeral: &mut HashMap<Uuid, Value>,
    ) -> Result<(Value, bool)> {
        if request.request_id.is_nil() {
            return Err("request ID must be nonnil".into());
        }
        let hash = digest(&serde_json::to_vec(request).map_err(|e| e.to_string())?);
        if let Some(r) = self.records.iter().find(|r| r.id == request.request_id) {
            if r.input_hash != hash {
                return Err("request ID reused with conflicting input".into());
            }
            return self
                .status(request.request_id, true, ephemeral)?
                .map(|v| (v.0, true))
                .ok_or("request evidence unavailable".into());
        }
        let result = match &request.action {
            MessageAction::Send {
                target,
                reply_to,
                body,
                sender_name,
                thread_id,
            } => {
                if target.is_some() == reply_to.is_some() {
                    return Err("send requires exactly one target or reply-to".into());
                }
                validate_body(body)?;
                let sender = caller(model, &request.caller, sender_name.as_deref())?;
                let (recipient, thread) = if let Some(parent) = reply_to {
                    let parent = self
                        .messages
                        .iter()
                        .find(|m| m.id == *parent)
                        .ok_or("reply parent unknown or expired")?;
                    if parent.namespace != model.identity_namespace
                        || thread_id.is_some_and(|t| t != parent.thread_id)
                    {
                        return Err("reply namespace or thread conflicts with parent".into());
                    }
                    let id = parent
                        .sender
                        .surface_id
                        .ok_or("original sender has no replyable surface")?;
                    (explicit_surface(model, &id.to_string())?, parent.thread_id)
                } else {
                    (
                        self.recipient(model, target.as_deref().unwrap_or_default(), epoch)?,
                        thread_id.unwrap_or_else(Uuid::new_v4),
                    )
                };
                if !self.enabled(&recipient.address) {
                    return Err("messaging disabled for recipient".into());
                }
                if self.messages.len() >= 128 {
                    let index = self
                        .messages
                        .iter()
                        .position(|m| matches!(m.state, MessageState::Read | MessageState::Failed))
                        .ok_or("message capacity reached (128); unread work is retained")?;
                    self.messages.remove(index);
                }
                self.sequence = self
                    .sequence
                    .checked_add(1)
                    .ok_or("message sequence exhausted")?;
                let has_agent = self.registered(&recipient, epoch);
                let message = Message {
                    id: Uuid::new_v4(),
                    request_id: request.request_id,
                    namespace: model.identity_namespace,
                    thread_id: thread,
                    reply_to: *reply_to,
                    sender,
                    recipient: recipient.address,
                    body: body.clone(),
                    sequence: self.sequence,
                    created_at: now(),
                    state: MessageState::Queued,
                    failure: None,
                    delivered_consumer: None,
                };
                let result =
                    json!({"durability":"saved","recipient_has_agent":has_agent,"message":message});
                self.messages.push(message);
                result
            }
            MessageAction::List {
                surface,
                states,
                limit,
                cursor,
            } => {
                if *limit == 0 || *limit > MAX_PAGE {
                    return Err("inbox limit must be 1..32".into());
                }
                let filter = surface
                    .as_deref()
                    .map(|s| {
                        if let Ok(uuid) = s.parse::<Uuid>()
                            && self.messages.iter().any(|m| {
                                m.namespace == model.identity_namespace
                                    && m.recipient.surface_id == uuid
                            })
                        {
                            return Ok(uuid);
                        }
                        explicit_surface(model, s).map(|s| s.address.surface_id)
                    })
                    .transpose()?;
                let rows: Vec<_> = self
                    .messages
                    .iter()
                    .filter(|m| {
                        m.namespace == model.identity_namespace
                            && filter.is_none_or(|id| m.recipient.surface_id == id)
                            && (states.is_empty() || states.contains(&m.state))
                            && cursor.is_none_or(|c| m.sequence > c)
                    })
                    .collect();
                let next = (rows.len() > *limit).then(|| rows[*limit - 1].sequence);
                json!({"messages":rows.into_iter().take(*limit).collect::<Vec<_>>(),"next_cursor":next})
            }
            MessageAction::MarkRead { ids } => {
                if ids.len() > MAX_PAGE || ids.is_empty() {
                    return Err("mark-read requires 1..32 exact IDs".into());
                }
                if ids.iter().any(|id| {
                    !self
                        .messages
                        .iter()
                        .any(|m| m.id == *id && m.namespace == model.identity_namespace)
                }) {
                    return Err("message ID unknown or expired".into());
                }
                for m in &mut self.messages {
                    if ids.contains(&m.id)
                        && matches!(m.state, MessageState::Queued | MessageState::Delivered)
                    {
                        m.state = MessageState::Read;
                    }
                }
                json!({"messages":self.messages.iter().filter(|m|ids.contains(&m.id)).collect::<Vec<_>>()})
            }
            MessageAction::Settings {
                scope,
                target,
                enabled,
            } => {
                let owner = match scope {
                    SettingScope::Global => {
                        if target.is_some() {
                            return Err("global setting accepts no target".into());
                        }
                        None
                    }
                    SettingScope::Workspace => Some(if let Some(target) = target {
                        workspace(model, target)?
                    } else {
                        caller(model, &request.caller, None)?
                            .workspace_id
                            .ok_or("settings require explicit target or validated caller")?
                    }),
                    SettingScope::Surface => Some(if let Some(target) = target {
                        self.recipient(model, target, epoch)?.address.surface_id
                    } else {
                        caller(model, &request.caller, None)?
                            .surface_id
                            .ok_or("settings require explicit target or validated caller")?
                    }),
                };
                if let Some(enabled) = enabled {
                    if let Some(s) = self
                        .settings
                        .iter_mut()
                        .find(|s| s.scope == *scope && s.owner == owner)
                    {
                        s.enabled = *enabled;
                    } else {
                        if self.settings.len() >= 256 {
                            return Err("message settings capacity reached (256)".into());
                        }
                        self.settings.push(Setting {
                            scope: *scope,
                            owner,
                            enabled: *enabled,
                        });
                    }
                    self.reconcile(&HashSet::new(), deadlines, &HashSet::new());
                }
                let configured = self
                    .settings
                    .iter()
                    .find(|s| s.scope == *scope && s.owner == owner)
                    .is_none_or(|s| s.enabled);
                let effective = self.settings.iter().all(|s| {
                    s.enabled
                        || !(s.scope == SettingScope::Global
                            || s.scope == *scope && s.owner == owner
                            || *scope == SettingScope::Surface
                                && s.scope == SettingScope::Workspace
                                && surfaces(model).iter().any(|v| {
                                    Some(v.address.surface_id) == owner
                                        && Some(v.address.workspace_id) == s.owner
                                }))
                });
                json!({"scope":scope,"owner":owner,"enabled":configured,"effective_enabled":effective,"reason":if effective{"enabled"}else{"messaging_disabled"}})
            }
            MessageAction::Register {
                surface,
                provider,
                interactive,
            } => {
                if provider.is_empty()
                    || provider.len() > 64
                    || provider.chars().any(char::is_control)
                {
                    return Err("provider label must be 1..64 bytes without controls".into());
                }
                let s = explicit_surface(model, surface)?;
                self.consumers.retain(|c| c.surface != s.address.surface_id);
                if self.consumers.len() >= 128 {
                    return Err("consumer capacity reached (128)".into());
                }
                let id = Uuid::new_v4();
                let token = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
                self.consumers.push(Consumer {
                    id,
                    namespace: model.identity_namespace,
                    surface: s.address.surface_id,
                    session: s.session,
                    epoch,
                    provider: provider.clone(),
                    interactive: *interactive,
                    held: false,
                    token_hash: digest(token.as_bytes()),
                });
                json!({"consumer":id,"token":token,"surface_id":s.address.surface_id,"epoch":epoch,"provider_authenticated":false,"wake_supported":false})
            }
            MessageAction::Poll {
                surface,
                consumer,
                token,
                held,
            } => {
                let c = self.consumer(model, surface, *consumer, token, epoch)?;
                if let Some(held) = held {
                    self.consumers
                        .iter_mut()
                        .find(|x| x.id == c.id)
                        .ok_or("consumer disappeared")?
                        .held = *held;
                }
                let held = held.unwrap_or(c.held);
                json!({"queued":self.messages.iter().filter(|m|m.recipient.surface_id==c.surface&&m.state==MessageState::Queued).count(),"held":held,"registered":true})
            }
            MessageAction::Claim {
                surface,
                consumer,
                token,
            } => {
                let c = self.consumer(model, surface, *consumer, token, epoch)?;
                if !c.interactive || c.held {
                    return Err("consumer is held or not interactive".into());
                }
                let s = explicit_surface(model, surface)?;
                if !self.enabled(&s.address) {
                    return Err("messaging disabled for recipient".into());
                }
                let active: HashSet<_> = self
                    .leases
                    .iter()
                    .filter(|l| !l.acked)
                    .flat_map(|l| l.ids.iter().copied())
                    .collect();
                let selected: Vec<_> = self
                    .messages
                    .iter()
                    .filter(|m| {
                        m.recipient.surface_id == c.surface
                            && m.state == MessageState::Queued
                            && !active.contains(&m.id)
                            && self.enabled(&m.recipient)
                    })
                    .take(8)
                    .cloned()
                    .collect();
                if selected.is_empty() {
                    json!({"lease_id":null,"messages":[],"lease_ttl_ms":10000})
                } else {
                    if self.leases.len() >= 128 {
                        let index = self
                            .leases
                            .iter()
                            .position(|l| l.acked)
                            .ok_or("lease capacity reached (128)")?;
                        self.leases.remove(index);
                    }
                    let id = Uuid::new_v4();
                    let ids = selected.iter().map(|m| m.id).collect();
                    self.leases.push(Lease {
                        id,
                        consumer: c.id,
                        surface: c.surface,
                        ids,
                        acked: false,
                        issued_monotonic_ms: monotonic_ms(),
                        expires_monotonic_ms: monotonic_ms().saturating_add(10000),
                        handed: false,
                    });
                    deadlines.insert(id, Instant::now() + Duration::from_secs(10));
                    json!({"lease_id":id,"messages":selected,"lease_ttl_ms":10000,"delivery":"at_least_once"})
                }
            }
            MessageAction::Ack {
                surface,
                consumer,
                token,
                lease_id,
                message_ids,
            } => {
                let c = self.consumer(model, surface, *consumer, token, epoch)?;
                let lease = self
                    .leases
                    .iter_mut()
                    .find(|l| l.id == *lease_id && l.consumer == c.id && l.surface == c.surface)
                    .ok_or("unknown or expired lease")?;
                if &lease.ids != message_ids {
                    return Err("ack IDs must exactly match lease order and set".into());
                }
                if !lease.handed {
                    return Err("lease context was never handed over".into());
                }
                if !lease.acked && deadlines.get(lease_id).is_none_or(|d| *d <= Instant::now()) {
                    return Err("lease expired".into());
                }
                if !lease.acked {
                    if message_ids.iter().any(|id| {
                        !self
                            .messages
                            .iter()
                            .any(|m| m.id == *id && m.state == MessageState::Queued)
                    }) {
                        return Err("ack message missing or no longer queued".into());
                    }
                    for m in &mut self.messages {
                        if message_ids.contains(&m.id) {
                            if m.state != MessageState::Queued {
                                return Err("message no longer queued".into());
                            }
                            m.state = MessageState::Delivered;
                            m.delivered_consumer = Some(c.id);
                        }
                    }
                    lease.acked = true;
                }
                json!({"lease_id":lease_id,"message_ids":message_ids,"state":"delivered"})
            }
            MessageAction::ReadTurn {
                surface,
                consumer,
                token,
                ids,
            } => {
                let c = self.consumer(model, surface, *consumer, token, epoch)?;
                if ids.is_empty()
                    || ids.len() > 8
                    || ids.iter().any(|id| {
                        !self.messages.iter().any(|m| {
                            m.id == *id
                                && m.recipient.surface_id == c.surface
                                && m.delivered_consumer == Some(c.id)
                                && matches!(m.state, MessageState::Delivered | MessageState::Read)
                        })
                    })
                {
                    return Err("read-turn requires exact IDs delivered to this generation".into());
                }
                for m in &mut self.messages {
                    if ids.contains(&m.id) {
                        m.state = MessageState::Read;
                    }
                }
                json!({"message_ids":ids,"state":"read"})
            }
        };
        if !request.action.is_read() {
            let secret = matches!(request.action, MessageAction::Register { .. });
            let retained = if secret {
                ephemeral.insert(request.request_id, result.clone());
                let mut redacted = result.clone();
                redacted
                    .as_object_mut()
                    .ok_or("invalid registration result")?
                    .remove("token");
                redacted
            } else {
                result.clone()
            };
            self.records.push(Record {
                id: request.request_id,
                input_hash: hash,
                result: retained,
                full: request.action.requires_full_result(),
                secret_lost: secret,
            });
            let unsettled: HashSet<_> = self
                .messages
                .iter()
                .filter(|m| matches!(m.state, MessageState::Queued | MessageState::Delivered))
                .map(|m| m.request_id)
                .collect();
            while self
                .records
                .iter()
                .filter(|r| !unsettled.contains(&r.id))
                .count()
                > 256
            {
                let i = self
                    .records
                    .iter()
                    .position(|r| !unsettled.contains(&r.id))
                    .ok_or("receipt capacity reached")?;
                let removed = self.records.remove(i);
                ephemeral.remove(&removed.id);
            }
        }
        Ok((result, false))
    }
}
