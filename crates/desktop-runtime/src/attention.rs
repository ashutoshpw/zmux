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
use zmux_workspace_core::{SessionId, Snapshot, SurfaceId, WorkspaceId};

pub const MAX_NOTIFICATIONS: usize = 128;
pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 128;
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 2048;
pub const DEFAULT_NOTIFICATION_TITLE: &str = "Notification";
pub fn default_notification_title() -> String {
    DEFAULT_NOTIFICATION_TITLE.into()
}

/// Notification handles resolve only against admitted attention history, never
/// against workspace targets or the currently focused object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum NotificationSelector {
    Id(u64),
    Uuid(Uuid),
}
impl From<u64> for NotificationSelector {
    fn from(id: u64) -> Self {
        Self::Id(id)
    }
}
impl std::fmt::Display for NotificationSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(id) => write!(formatter, "{id}"),
            Self::Uuid(uuid) => write!(formatter, "notification:{uuid}"),
        }
    }
}
impl std::str::FromStr for NotificationSelector {
    type Err = AttentionError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(id) = value.parse::<u64>()
            && id > 0
        {
            return Ok(Self::Id(id));
        }
        let value = value.strip_prefix("notification:").unwrap_or(value);
        Uuid::parse_str(value).map(Self::Uuid).map_err(|_| {
            AttentionError::InvalidSelector(
                "notification target must be a positive ID, UUID, or notification:UUID",
            )
        })
    }
}
impl<'de> Deserialize<'de> for NotificationSelector {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Id(u64),
            Text(String),
        }
        match Input::deserialize(deserializer)? {
            Input::Id(id) if id > 0 => Ok(Self::Id(id)),
            Input::Text(value) => value.parse().map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom("notification ID must be positive")),
        }
    }
}

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
/// Creation and selection intent, before session ownership is captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationAddress {
    Workspace(WorkspaceId),
    Surface(SurfaceId),
}
impl From<SurfaceId> for NotificationAddress {
    fn from(surface_id: SurfaceId) -> Self {
        Self::Surface(surface_id)
    }
}
/// Workspace notifications never borrow a pane. Surface tokens retain their
/// captured process generation while their live workspace may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationDestination {
    Workspace {
        workspace_id: WorkspaceId,
    },
    Surface {
        surface_id: SurfaceId,
        #[serde(deserialize_with = "deserialize_session_id")]
        session_id: Option<SessionId>,
    },
}
fn deserialize_session_id<'de, D>(deserializer: D) -> Result<Option<SessionId>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}
impl NotificationDestination {
    pub fn address(self) -> NotificationAddress {
        match self {
            Self::Workspace { workspace_id } => NotificationAddress::Workspace(workspace_id),
            Self::Surface { surface_id, .. } => NotificationAddress::Surface(surface_id),
        }
    }
    pub fn surface_id(self) -> Option<SurfaceId> {
        match self {
            Self::Surface { surface_id, .. } => Some(surface_id),
            Self::Workspace { .. } => None,
        }
    }
    pub fn session_id(self) -> Option<SessionId> {
        match self {
            Self::Surface { session_id, .. } => session_id,
            Self::Workspace { .. } => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationTarget {
    pub epoch: Uuid,
    pub identity_namespace: Uuid,
    pub notification_id: u64,
    pub destination: NotificationDestination,
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
    pub uuid: Uuid,
    pub reference: String,
    pub surface_id: Option<SurfaceId>,
    pub identity_namespace: Uuid,
    /// Last workspace owning this exact live session; retained after close.
    pub workspace_id: WorkspaceId,
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
    JumpToUnread,
}

#[derive(Clone, Debug, Serialize)]
pub struct NotificationNavigation {
    pub opened: bool,
    pub notification_id: Option<u64>,
    pub notification_uuid: Option<Uuid>,
    pub target: Option<NotificationTarget>,
}
#[derive(Debug, Error)]
pub enum AttentionError {
    #[error("notification title must not be empty")]
    EmptyTitle,
    #[error(
        "notification target is not a live workspace or surface in an initialized identity namespace"
    )]
    InvalidOwner,
    #[error("notification identity space exhausted")]
    Exhausted,
    #[error("notification generation, owner or epoch is stale")]
    StaleTarget,
    #[error("notification authorization observation is stale")]
    StaleAuthorization,
    #[error("notification cursor belongs to a previous attention epoch")]
    StaleEpoch,
    #[error("notification {0} does not exist")]
    NotFound(NotificationSelector),
    #[error("{0}")]
    InvalidSelector(&'static str),
}
impl AttentionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyTitle => "invalid_notification_title",
            Self::InvalidOwner => "invalid_notification_owner",
            Self::Exhausted => "notification_ids_exhausted",
            Self::StaleTarget => "stale_notification_target",
            Self::StaleAuthorization => "stale_notification_authorization",
            Self::StaleEpoch => "stale_notification_epoch",
            Self::NotFound(_) => "notification_not_found",
            Self::InvalidSelector(_) => "invalid_notification_selector",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttentionCommand {
    Notify {
        surface_id: SurfaceId,
        #[serde(default = "default_notification_title")]
        title: String,
        #[serde(default)]
        subtitle: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        desktop: Option<bool>,
    },
    Read {
        id: NotificationSelector,
    },
    Open {
        id: NotificationSelector,
    },
    Dismiss {
        id: NotificationSelector,
    },
    JumpToUnread,
    /// A cursor is mandatory: a newly arrived generation must survive a stale clear.
    Clear {
        through_id: u64,
    },
    AppActive {
        active: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationAction {
    MarkRead {
        selection: NotificationSelection,
    },
    Dismiss {
        selection: DismissSelection,
    },
    Clear {
        scope: NotificationScope,
        #[serde(default)]
        through_id: Option<u64>,
        #[serde(default)]
        expected_epoch: Option<Uuid>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotificationSelection {
    Id { id: NotificationSelector },
    All,
    Workspace,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case", deny_unknown_fields)]
pub enum DismissSelection {
    Id { id: NotificationSelector },
    AllRead,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationScope {
    All,
    Workspace,
    Surface,
    Caller,
}

/// Resolved by the shared target resolver on the executor, never by the CLI.
pub enum NotificationFilter {
    Id(u64),
    All,
    Workspace {
        identity_namespace: Uuid,
        workspace_id: WorkspaceId,
    },
    Surface {
        identity_namespace: Uuid,
        surface_id: SurfaceId,
    },
}
impl NotificationFilter {
    fn matches(&self, item: &Notification) -> bool {
        match self {
            Self::Id(id) => item.id == *id,
            Self::All => true,
            Self::Workspace {
                identity_namespace,
                workspace_id,
            } => {
                item.identity_namespace == *identity_namespace && item.workspace_id == *workspace_id
            }
            Self::Surface {
                identity_namespace,
                surface_id,
            } => {
                item.identity_namespace == *identity_namespace
                    && item.surface_id == Some(*surface_id)
            }
        }
    }
}
pub enum NotificationMutationKind {
    MarkRead,
    Dismiss,
    DismissRead,
    Clear,
}
#[derive(Clone, Debug, Serialize)]
pub struct NotificationMutation {
    pub epoch: Uuid,
    pub cursor: u64,
    pub affected_ids: Vec<u64>,
    pub affected_uuids: Vec<Uuid>,
    pub affected_count: usize,
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
    pub fn cursor(&self) -> u64 {
        self.next_id
    }
    pub fn mutation_receipt(&self, affected_ids: Vec<u64>, cursor: u64) -> NotificationMutation {
        NotificationMutation {
            epoch: self.epoch,
            cursor,
            affected_count: affected_ids.len(),
            affected_uuids: affected_ids
                .iter()
                .filter_map(|id| {
                    self.items
                        .iter()
                        .find(|item| item.id == *id)
                        .map(|item| item.uuid)
                })
                .collect(),
            affected_ids,
        }
    }
    pub fn resolve(&self, selector: NotificationSelector) -> Result<u64, AttentionError> {
        self.items
            .iter()
            .find(|item| match selector {
                NotificationSelector::Id(id) => item.id == id,
                NotificationSelector::Uuid(uuid) => item.uuid == uuid,
            })
            .map(|item| item.id)
            .ok_or(AttentionError::NotFound(selector))
    }
    pub fn apply_mutation(
        &mut self,
        filter: &NotificationFilter,
        kind: NotificationMutationKind,
        through_id: Option<u64>,
        expected_epoch: Option<Uuid>,
    ) -> Result<NotificationMutation, AttentionError> {
        if expected_epoch.is_some_and(|epoch| epoch != self.epoch) {
            return Err(AttentionError::StaleEpoch);
        }
        if let NotificationFilter::Id(id) = filter
            && !self.items.iter().any(|item| item.id == *id)
        {
            return Err(AttentionError::NotFound((*id).into()));
        }
        let cursor = through_id.unwrap_or(self.next_id).min(self.next_id);
        let affected_ids: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.id <= cursor && filter.matches(item))
            .filter(|item| match kind {
                NotificationMutationKind::MarkRead => !item.read,
                NotificationMutationKind::DismissRead => item.read,
                _ => true,
            })
            .map(|item| item.id)
            .collect();
        let receipt = self.mutation_receipt(affected_ids.clone(), cursor);
        match kind {
            NotificationMutationKind::MarkRead => {
                for item in &mut self.items {
                    if affected_ids.contains(&item.id) {
                        acknowledge(item);
                    }
                }
            }
            _ => self.items.retain(|item| !affected_ids.contains(&item.id)),
        }
        Ok(receipt)
    }
    pub fn notify_owned(
        &mut self,
        model: &Snapshot,
        address: impl Into<NotificationAddress>,
        source: &str,
        content: &NotificationContent,
        read: bool,
    ) -> Result<u64, AttentionError> {
        if model.identity_namespace.is_nil() {
            return Err(AttentionError::InvalidOwner);
        }
        let (workspace_id, destination) = match address.into() {
            NotificationAddress::Workspace(workspace_id) => {
                if !model
                    .workspaces
                    .iter()
                    .any(|workspace| workspace.id == workspace_id)
                {
                    return Err(AttentionError::InvalidOwner);
                }
                (
                    workspace_id,
                    NotificationDestination::Workspace { workspace_id },
                )
            }
            NotificationAddress::Surface(surface_id) => {
                let (workspace_id, surface) = model
                    .workspaces
                    .iter()
                    .find_map(|workspace| {
                        workspace
                            .root
                            .panes()
                            .into_iter()
                            .flat_map(|pane| &pane.surfaces)
                            .find(|surface| surface.id == surface_id)
                            .map(|surface| (workspace.id, surface))
                    })
                    .ok_or(AttentionError::InvalidOwner)?;
                (
                    workspace_id,
                    NotificationDestination::Surface {
                        surface_id,
                        session_id: surface.session_id,
                    },
                )
            }
        };
        let target = NotificationTarget {
            epoch: self.epoch,
            identity_namespace: model.identity_namespace,
            notification_id: 0,
            destination,
        };
        self.insert(workspace_id, source, content, read, target)
    }
    fn insert(
        &mut self,
        workspace_id: WorkspaceId,
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
        let uuid = Uuid::new_v5(&self.epoch, format!("zmux.notification.v1:{id}").as_bytes());
        self.items.push(Notification {
            id,
            uuid,
            reference: format!("notification:{uuid}"),
            surface_id: target.destination.surface_id(),
            identity_namespace: target.identity_namespace,
            workspace_id,
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
        acknowledge(item);
        Ok(())
    }
    pub fn reconcile(&mut self, model: &Snapshot) -> bool {
        let mut changed = false;
        for item in &mut self.items {
            if let Some(target) = &item.target
                && let Some(workspace_id) = current_workspace(model, self.epoch, target)
                && item.workspace_id != workspace_id
            {
                item.workspace_id = workspace_id;
                changed = true;
            }
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
    pub fn open_target(
        &self,
        model: &Snapshot,
        id: u64,
    ) -> Result<NotificationAddress, AttentionError> {
        let item = self.find(id).map_err(|_| AttentionError::StaleTarget)?;
        let target = item.target.as_ref().ok_or(AttentionError::StaleTarget)?;
        self.native_inbox_target(model, target)
    }
    /// An in-app row may be read or suppressed, but must retain its exact live owner.
    pub fn native_inbox_target(
        &self,
        model: &Snapshot,
        target: &NotificationTarget,
    ) -> Result<NotificationAddress, AttentionError> {
        self.native_item(target)?;
        if !current_owner(model, self.epoch, target) {
            return Err(AttentionError::StaleTarget);
        }
        Ok(target.destination.address())
    }
    /// Native banners additionally require a still-unread, eligible delivery.
    pub fn native_open_target(
        &self,
        model: &Snapshot,
        target: &NotificationTarget,
    ) -> Result<NotificationAddress, AttentionError> {
        let item = self.native_item(target)?;
        if item.read || !item.delivery.active() || !current_owner(model, self.epoch, target) {
            return Err(AttentionError::StaleTarget);
        }
        if !self.authorization.allows_scheduling()
            || item.delivery.authorization_generation.is_none()
        {
            return Err(AttentionError::StaleAuthorization);
        }
        Ok(target.destination.address())
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
fn acknowledge(item: &mut Notification) {
    item.read = true;
    if item.delivery.active() {
        item.delivery.suppress("read");
    }
}
fn same_origin(first: Option<&NotificationTarget>, second: Option<&NotificationTarget>) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(first), Some(second)) => {
            first.epoch == second.epoch
                && first.identity_namespace == second.identity_namespace
                && first.destination == second.destination
        }
        _ => false,
    }
}
fn current_owner(model: &Snapshot, epoch: Uuid, target: &NotificationTarget) -> bool {
    current_workspace(model, epoch, target).is_some()
}
fn current_workspace(
    model: &Snapshot,
    epoch: Uuid,
    target: &NotificationTarget,
) -> Option<WorkspaceId> {
    if target.epoch != epoch || model.identity_namespace != target.identity_namespace {
        return None;
    }
    match target.destination {
        NotificationDestination::Workspace { workspace_id } => model
            .workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .map(|workspace| workspace.id),
        NotificationDestination::Surface {
            surface_id,
            session_id,
        } => model.workspaces.iter().find_map(|workspace| {
            workspace
                .root
                .panes()
                .into_iter()
                .flat_map(|pane| &pane.surfaces)
                .any(|surface| surface.id == surface_id && surface.session_id == session_id)
                .then_some(workspace.id)
        }),
    }
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
