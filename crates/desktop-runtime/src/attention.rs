//! Bounded in-app attention and native notification ownership. Ordinary terminal
//! text never becomes a command. OS scheduling is separate from inbox admission.
//!
//! Native delivery consumes an authoritative desired snapshot, not an unbounded
//! effect queue. Epoch, namespace, notification generation and captured terminal
//! session fence callbacks. Reading/removing/coalescing a row removes its native
//! desire. Authorization revocation cancels previous desires; granting permission
//! later never replays those suppressed notifications. Scheduled means OS request
//! admission, not proof that a user saw a banner.
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{SessionId, Snapshot, SurfaceId};

pub const MAX_NOTIFICATIONS: usize = 128;
pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 128;
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 2048;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationContent {
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub desktop: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationTarget {
    pub epoch: Uuid,
    pub identity_namespace: Uuid,
    pub notification_id: u64,
    pub surface_id: SurfaceId,
    pub session_id: Option<SessionId>,
}
impl NotificationTarget {
    pub fn identifier(&self) -> String {
        format!("zmux.notification.{}.{}", self.epoch, self.notification_id)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationAuthorization {
    #[default]
    Unknown,
    NotDetermined,
    Denied,
    Authorized,
    Provisional,
    Ephemeral,
    Unavailable,
}
impl NotificationAuthorization {
    pub fn allows_scheduling(self) -> bool {
        matches!(self, Self::Authorized | Self::Provisional | Self::Ephemeral)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopDeliveryState {
    Pending,
    Scheduled,
    Suppressed,
    Failed,
}
#[derive(Clone, Debug, Serialize)]
pub struct DesktopDelivery {
    pub state: DesktopDeliveryState,
    pub reason: Option<String>,
    pub error: Option<String>,
    /// Bound to the observation authorizing this attempt, not every later refresh.
    pub authorization_generation: Option<u64>,
}
impl DesktopDelivery {
    fn pending() -> Self {
        Self {
            state: DesktopDeliveryState::Pending,
            reason: None,
            error: None,
            authorization_generation: None,
        }
    }
    fn suppress(&mut self, reason: &str) {
        self.state = DesktopDeliveryState::Suppressed;
        self.reason = Some(reason.into());
        self.error = None;
    }
    fn fail(&mut self, reason: &str, error: &str) {
        self.state = DesktopDeliveryState::Failed;
        self.reason = Some(reason.into());
        self.error = Some(clean(error, 512));
    }
    fn active(&self) -> bool {
        matches!(
            self.state,
            DesktopDeliveryState::Pending | DesktopDeliveryState::Scheduled
        )
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Notification {
    pub id: u64,
    pub surface_id: SurfaceId,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub desktop: Option<bool>,
    pub source: String,
    pub timestamp_ms: u64,
    pub read: bool,
    /// Rows lacking a captured owner are never eligible for native effects.
    pub target: Option<NotificationTarget>,
    pub delivery: DesktopDelivery,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeNotificationRequest {
    pub target: NotificationTarget,
    pub identifier: String,
    /// Zero means authorization is not known yet; native must not schedule it.
    pub authorization_generation: u64,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeNotificationSnapshot {
    pub epoch: Uuid,
    pub authorization: NotificationAuthorization,
    pub authorization_generation: u64,
    pub desired: Vec<NativeNotificationRequest>,
}
/// Accepted only by the trusted native bridge, never by the socket command enum.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeNotificationEvent {
    Authorization {
        epoch: Uuid,
        generation: u64,
        state: NotificationAuthorization,
    },
    Scheduled {
        target: NotificationTarget,
        authorization_generation: u64,
    },
    Failed {
        target: NotificationTarget,
        authorization_generation: u64,
        message: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeAttentionAction {
    Event { event: NativeNotificationEvent },
    Open { target: NotificationTarget },
    OpenInbox { target: NotificationTarget },
    Dismiss { target: NotificationTarget },
}
#[derive(Debug, Error)]
pub enum AttentionError {
    #[error("notification title must not be empty")]
    EmptyTitle,
    #[error("notification target is not a live surface in an initialized identity namespace")]
    InvalidOwner,
    #[error("notification identity space exhausted")]
    Exhausted,
    #[error("notification generation, owner or epoch is stale")]
    StaleTarget,
    #[error("notification authorization observation is stale")]
    StaleAuthorization,
}
impl AttentionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyTitle => "invalid_notification_title",
            Self::InvalidOwner => "invalid_notification_owner",
            Self::Exhausted => "notification_ids_exhausted",
            Self::StaleTarget => "stale_notification_target",
            Self::StaleAuthorization => "stale_notification_authorization",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttentionCommand {
    Notify {
        surface_id: SurfaceId,
        title: String,
        body: String,
    },
    Read {
        id: u64,
    },
    Open {
        id: u64,
    },
    Dismiss {
        id: u64,
    },
    /// A cursor is mandatory: a newly arrived generation must survive a stale clear.
    Clear {
        through_id: u64,
    },
    AppActive {
        active: bool,
    },
}

/// The executor owns this state. Native readers consume bounded snapshots; only
/// trusted native events can report authorization or scheduling outcomes.
pub struct Attention {
    pub items: Vec<Notification>,
    pub dropped: u64,
    next_id: u64,
    epoch: Uuid,
    authorization: NotificationAuthorization,
    authorization_generation: u64,
}
impl Default for Attention {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            dropped: 0,
            next_id: 0,
            epoch: Uuid::new_v4(),
            authorization: NotificationAuthorization::Unknown,
            authorization_generation: 0,
        }
    }
}
impl Attention {
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }
    pub fn notify_owned(
        &mut self,
        model: &Snapshot,
        surface_id: SurfaceId,
        source: &str,
        content: &NotificationContent,
        read: bool,
    ) -> Result<u64, AttentionError> {
        let surface = model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|surface| surface.id == surface_id)
            .ok_or(AttentionError::InvalidOwner)?;
        if model.identity_namespace.is_nil() {
            return Err(AttentionError::InvalidOwner);
        }
        let target = NotificationTarget {
            epoch: self.epoch,
            identity_namespace: model.identity_namespace,
            notification_id: 0,
            surface_id,
            session_id: surface.session_id,
        };
        self.insert(surface_id, source, content, read, target)
    }
    fn insert(
        &mut self,
        surface_id: SurfaceId,
        source: &str,
        content: &NotificationContent,
        read: bool,
        mut target: NotificationTarget,
    ) -> Result<u64, AttentionError> {
        let title = clean(&content.title, MAX_NOTIFICATION_TITLE_CHARS);
        let subtitle = clean(&content.subtitle, MAX_NOTIFICATION_TITLE_CHARS);
        let body = clean(&content.body, MAX_NOTIFICATION_BODY_CHARS);
        let source = clean(source, 32);
        if title.is_empty() {
            return Err(AttentionError::EmptyTitle);
        }
        let id = self
            .next_id
            .checked_add(1)
            .ok_or(AttentionError::Exhausted)?;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        // A same-surface replacement session never coalesces into an older owner.
        if self.items.last().is_some_and(|last| {
            !last.read
                && !read
                && last.surface_id == surface_id
                && last.source == source
                && last.title == title
                && last.subtitle == subtitle
                && last.body == body
                && last.desktop == content.desktop
                && same_origin(last.target.as_ref(), Some(&target))
                && timestamp_ms.saturating_sub(last.timestamp_ms) < 1000
        }) {
            self.items.pop();
        }
        self.next_id = id;
        if self.items.len() == MAX_NOTIFICATIONS {
            self.items.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        target.notification_id = id;
        let mut delivery = DesktopDelivery::pending();
        if read {
            delivery.suppress("focused");
        } else if content.desktop == Some(false) {
            delivery.suppress("disabled");
        } else {
            apply_authorization(
                &mut delivery,
                self.authorization,
                self.authorization_generation,
                false,
            );
        }
        self.items.push(Notification {
            id,
            surface_id,
            title,
            subtitle,
            body,
            desktop: content.desktop,
            source,
            timestamp_ms,
            read,
            target: Some(target),
            delivery,
        });
        Ok(id)
    }
    pub fn find(&self, id: u64) -> Result<&Notification, String> {
        self.items
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("notification {id} does not exist"))
    }
    pub fn read(&mut self, id: u64) -> Result<(), String> {
        let item = self
            .items
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("notification {id} does not exist"))?;
        item.read = true;
        if item.delivery.active() {
            item.delivery.suppress("read");
        }
        Ok(())
    }
    pub fn dismiss(&mut self, id: u64) -> Result<(), String> {
        self.find(id)?;
        self.items.retain(|n| n.id != id);
        Ok(())
    }
    pub fn clear(&mut self, through: u64) {
        self.items.retain(|n| n.id > through);
    }
    pub fn reconcile(&mut self, model: &Snapshot) -> bool {
        let mut changed = false;
        for item in &mut self.items {
            if item.delivery.active()
                && item
                    .target
                    .as_ref()
                    .is_none_or(|target| !current_owner(model, self.epoch, target))
            {
                item.delivery.suppress("stale_owner");
                changed = true;
            }
        }
        changed
    }
    pub fn reset_native(&mut self, reason: &str) {
        self.epoch = Uuid::new_v4();
        self.authorization = NotificationAuthorization::Unknown;
        self.authorization_generation = 0;
        for item in &mut self.items {
            if item.delivery.active() {
                item.delivery.suppress("cancelled");
                item.delivery.error = Some(clean(reason, 512));
            }
        }
    }
    pub fn native_snapshot(&self, model: &Snapshot) -> NativeNotificationSnapshot {
        NativeNotificationSnapshot {
            epoch: self.epoch,
            authorization: self.authorization,
            authorization_generation: self.authorization_generation,
            desired: self
                .items
                .iter()
                .filter(|item| !item.read && item.delivery.active())
                .filter_map(|item| {
                    let target = item.target.as_ref()?;
                    if !current_owner(model, self.epoch, target) {
                        return None;
                    }
                    Some(NativeNotificationRequest {
                        target: target.clone(),
                        identifier: target.identifier(),
                        authorization_generation: item
                            .delivery
                            .authorization_generation
                            .unwrap_or(0),
                        title: item.title.clone(),
                        subtitle: item.subtitle.clone(),
                        body: item.body.clone(),
                    })
                })
                .collect(),
        }
    }
    /// An open is validated before the executor selects the target. Only after
    /// selection succeeds may it mark this generation read and activate its window.
    pub fn open_target(&self, model: &Snapshot, id: u64) -> Result<SurfaceId, AttentionError> {
        let item = self.find(id).map_err(|_| AttentionError::StaleTarget)?;
        let target = item.target.as_ref().ok_or(AttentionError::StaleTarget)?;
        self.native_inbox_target(model, target)
    }
    /// An in-app row may be read or suppressed, but must retain its exact live owner.
    pub fn native_inbox_target(
        &self,
        model: &Snapshot,
        target: &NotificationTarget,
    ) -> Result<SurfaceId, AttentionError> {
        self.native_item(target)?;
        if !current_owner(model, self.epoch, target) {
            return Err(AttentionError::StaleTarget);
        }
        Ok(target.surface_id)
    }
    /// Native banners additionally require a still-unread, eligible delivery.
    pub fn native_open_target(
        &self,
        model: &Snapshot,
        target: &NotificationTarget,
    ) -> Result<SurfaceId, AttentionError> {
        let item = self.native_item(target)?;
        if item.read || !item.delivery.active() || !current_owner(model, self.epoch, target) {
            return Err(AttentionError::StaleTarget);
        }
        if !self.authorization.allows_scheduling()
            || item.delivery.authorization_generation.is_none()
        {
            return Err(AttentionError::StaleAuthorization);
        }
        Ok(target.surface_id)
    }
    /// Acknowledge the exact row after the executor validates its live owner.
    /// Repeating an acknowledgement is harmless; this never selects a surface
    /// or sends terminal input. Live-owner validation and this mutation must
    /// share the serialized executor boundary.
    pub fn native_dismiss(&mut self, target: &NotificationTarget) -> Result<(), AttentionError> {
        self.native_item(target)?;
        self.read(target.notification_id)
            .map_err(|_| AttentionError::StaleTarget)
    }
    fn native_item(&self, target: &NotificationTarget) -> Result<&Notification, AttentionError> {
        if target.epoch != self.epoch {
            return Err(AttentionError::StaleTarget);
        }
        self.items
            .iter()
            .find(|item| item.id == target.notification_id && item.target.as_ref() == Some(target))
            .ok_or(AttentionError::StaleTarget)
    }
    pub fn native_event(
        &mut self,
        model: &Snapshot,
        event: NativeNotificationEvent,
    ) -> Result<(), AttentionError> {
        self.reconcile(model);
        match event {
            NativeNotificationEvent::Authorization {
                epoch,
                generation,
                state,
            } => {
                if epoch != self.epoch || generation <= self.authorization_generation {
                    return Err(AttentionError::StaleAuthorization);
                }
                let revoked = self.authorization.allows_scheduling() && !state.allows_scheduling();
                self.authorization = state;
                self.authorization_generation = generation;
                for item in &mut self.items {
                    if item.delivery.active() {
                        apply_authorization(&mut item.delivery, state, generation, revoked);
                    }
                }
            }
            NativeNotificationEvent::Scheduled {
                target,
                authorization_generation,
            } => {
                self.validate_callback(model, &target, authorization_generation)?;
                let item = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == target.notification_id)
                    .expect("callback validated");
                item.delivery.state = DesktopDeliveryState::Scheduled;
            }
            NativeNotificationEvent::Failed {
                target,
                authorization_generation,
                message,
            } => {
                self.validate_callback(model, &target, authorization_generation)?;
                let item = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == target.notification_id)
                    .expect("callback validated");
                if item.delivery.state != DesktopDeliveryState::Pending {
                    return Err(AttentionError::StaleTarget);
                }
                item.delivery.fail("scheduling_failed", &message);
            }
        }
        Ok(())
    }
    fn validate_callback(
        &self,
        model: &Snapshot,
        target: &NotificationTarget,
        generation: u64,
    ) -> Result<(), AttentionError> {
        let item = self.native_item(target)?;
        if item.read || !item.delivery.active() || !current_owner(model, self.epoch, target) {
            return Err(AttentionError::StaleTarget);
        }
        if !self.authorization.allows_scheduling()
            || generation == 0
            || item.delivery.authorization_generation != Some(generation)
        {
            return Err(AttentionError::StaleAuthorization);
        }
        Ok(())
    }
}
fn same_origin(first: Option<&NotificationTarget>, second: Option<&NotificationTarget>) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            first.epoch == second.epoch
                && first.identity_namespace == second.identity_namespace
                && first.surface_id == second.surface_id
                && first.session_id == second.session_id
        }
        _ => false,
    }
}
fn current_owner(model: &Snapshot, epoch: Uuid, target: &NotificationTarget) -> bool {
    target.epoch == epoch
        && model.identity_namespace == target.identity_namespace
        && model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .any(|surface| {
                surface.id == target.surface_id && surface.session_id == target.session_id
            })
}
fn apply_authorization(
    delivery: &mut DesktopDelivery,
    state: NotificationAuthorization,
    generation: u64,
    revoked: bool,
) {
    match state {
        NotificationAuthorization::Denied => delivery.suppress("authorization_denied"),
        NotificationAuthorization::Unavailable => delivery.fail(
            "authorization_unavailable",
            "native notification authorization is unavailable",
        ),
        _ if revoked => delivery.suppress("authorization_revoked"),
        _ if state.allows_scheduling() && delivery.authorization_generation.is_none() => {
            delivery.authorization_generation = Some(generation)
        }
        _ => (),
    }
}
fn clean(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect::<String>()
        .trim()
        .into()
}
