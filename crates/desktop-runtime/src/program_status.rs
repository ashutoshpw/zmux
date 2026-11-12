//! Desktop projection and single-source alert policy for terminal-local OSC 7501 records.
//!
//! Protocol storage remains in the terminal/session owner. This module binds
//! those snapshots to exact workspace owners, keeps a small admitted-alert
//! journal, and never turns self-declared program labels into provider identity.

use std::{
    cmp::Reverse,
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zmux_program_status::{
    CommandEpoch, ProgramStatusSnapshot, Record, SessionEpoch, State, Transition, TransitionCause,
};
use zmux_workspace_core::{
    PaneId, SessionId, Snapshot as Model, SurfaceId, WindowId, WorkspaceId, targets::TargetKind,
};

use crate::{Frame, agent_lifecycle};

pub const MAX_PROGRAM_STATUS_ALERT_EVENTS: usize = 64;
pub const MAX_PROVIDER_ROUTES: usize = 128;
pub const MAX_PROGRAM_STATUS_PROJECTION_BYTES: usize = 768 * 1024;
pub const PROGRAM_STATUS_ALERT_COOLDOWN: Duration = Duration::from_secs(2);
pub const MAX_PENDING_PROVIDER_CHECKPOINTS: usize = 64;

const MAX_EVENT_TITLE_BYTES: usize = 192;
const MAX_EVENT_MESSAGE_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    NeedsUser,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlertSource {
    Provider {
        reservation: Option<ProviderRouteIdentity>,
    },
    Osc7501,
}

/// Exact terminal owner plus a bounded record projection. Nonfocused owners
/// carry only their deterministic priority summary; selecting that exact
/// terminal refreshes the desktop snapshot with its complete record set.
#[derive(Clone, Debug, Serialize)]
pub struct Terminal {
    pub source: &'static str,
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub workspace_id: WorkspaceId,
    pub workspace_uuid: Uuid,
    pub pane_id: PaneId,
    pub pane_uuid: Uuid,
    pub surface_id: SurfaceId,
    pub surface_uuid: Uuid,
    pub session_id: SessionId,
    pub session_epoch: SessionEpoch,
    pub command_epoch: CommandEpoch,
    pub availability: Availability,
    pub record_count: usize,
    pub records: Vec<Record>,
    pub records_complete: bool,
    pub omitted_records: usize,
    pub suppressed_alert_count: u64,
}

/// Events are source-arbitrated by Rust and retain their own presentation data,
/// so a later clear or eviction cannot erase an already admitted alert.
#[derive(Clone, Debug, Serialize)]
pub struct AlertEvent {
    pub cursor: u64,
    pub source: &'static str,
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub workspace_id: WorkspaceId,
    pub workspace_uuid: Uuid,
    pub pane_id: PaneId,
    pub pane_uuid: Uuid,
    pub surface_id: SurfaceId,
    pub surface_uuid: Uuid,
    pub session_id: SessionId,
    pub session_epoch: SessionEpoch,
    pub command_epoch: CommandEpoch,
    pub record_id: String,
    pub kind: AlertKind,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AlertSnapshot {
    pub epoch: Uuid,
    pub cursor: u64,
    pub dropped_through_cursor: u64,
    pub events: Vec<AlertEvent>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Projection {
    pub terminals: Vec<Terminal>,
    pub alerts: AlertSnapshot,
}

/// Full owner and command identity required for native row selection.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SelectionTarget {
    pub identity_namespace: Uuid,
    pub window_id: WindowId,
    pub window_uuid: Uuid,
    pub workspace_id: WorkspaceId,
    pub workspace_uuid: Uuid,
    pub pane_id: PaneId,
    pub pane_uuid: Uuid,
    pub surface_id: SurfaceId,
    pub surface_uuid: Uuid,
    pub session_id: SessionId,
    pub session_epoch: SessionEpoch,
    pub command_epoch: CommandEpoch,
    /// NativeBridge sends this as `id`, matching ProgramStatusRecord.id.
    #[serde(rename = "id")]
    pub record_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct OwnerKey {
    target: agent_lifecycle::Target,
    session_epoch: SessionEpoch,
}

#[derive(Clone, Copy, Debug)]
struct CurrentOwner {
    target: agent_lifecycle::Target,
    window_id: WindowId,
    window_uuid: Uuid,
    workspace_id: WorkspaceId,
    workspace_uuid: Uuid,
    pane_id: PaneId,
    pane_uuid: Uuid,
}

struct OwnerTrack {
    session_epoch: SessionEpoch,
    command_epoch: CommandEpoch,
    cursor: zmux_program_status::TransitionCursor,
    source: Option<AlertSource>,
    last_external_alert: Option<Instant>,
    suppressed_alert_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProviderRouteIdentity {
    provider: agent_lifecycle::Provider,
    route_id: Uuid,
    runtime_id: Uuid,
    generation: Uuid,
    transport: Option<TransportOwner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportOwner {
    pub runtime_id: Uuid,
    pub generation: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderRouteStart {
    pub target: agent_lifecycle::Target,
    pub provider: agent_lifecycle::Provider,
    pub route_id: Uuid,
    pub runtime_id: Uuid,
    pub generation: Uuid,
    pub session_epoch: SessionEpoch,
    pub command_epoch: CommandEpoch,
    pub cursor: zmux_program_status::TransitionCursor,
    pub transport: Option<TransportOwner>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProviderRoute {
    target: agent_lifecycle::Target,
    provider: agent_lifecycle::Provider,
    route_id: Uuid,
    runtime_id: Uuid,
    generation: Uuid,
    session_epoch: SessionEpoch,
    command_epoch: CommandEpoch,
    start_cursor: zmux_program_status::TransitionCursor,
    transport: Option<TransportOwner>,
    /// Route-start ACK enqueue is only a reservation. Provider lifecycle
    /// evidence or its normal spawned-invocation end confirms ownership.
    confirmed: bool,
    ended: bool,
}

impl ProviderRoute {
    fn identity(self) -> ProviderRouteIdentity {
        ProviderRouteIdentity {
            provider: self.provider,
            route_id: self.route_id,
            runtime_id: self.runtime_id,
            generation: self.generation,
            transport: self.transport,
        }
    }
}

/// Desktop-owned route and alert policy. Provider routes never appear in rows
/// or snapshots; only the OSC-owned presentation and admitted alert feed do.
pub struct Runtime {
    tracks: HashMap<OwnerKey, OwnerTrack>,
    routes: VecDeque<ProviderRoute>,
    pending_provider_checks: HashMap<agent_lifecycle::Target, usize>,
    pending_provider_check_count: usize,
    alert_epoch: Uuid,
    alert_cursor: u64,
    alert_dropped_through_cursor: u64,
    alert_events: VecDeque<AlertEvent>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            tracks: HashMap::new(),
            routes: VecDeque::new(),
            pending_provider_checks: HashMap::new(),
            pending_provider_check_count: 0,
            alert_epoch: Uuid::new_v4(),
            alert_cursor: 0,
            alert_dropped_through_cursor: 0,
            alert_events: VecDeque::new(),
        }
    }
}

impl Runtime {
    pub fn alert_epoch(&self) -> Uuid {
        self.alert_epoch
    }

    /// Hold one exact terminal's OSC journal while a provider event waits for
    /// the session worker's ordered status checkpoint.
    pub fn begin_provider_checkpoint(&mut self, target: agent_lifecycle::Target) -> bool {
        if self.pending_provider_check_count >= MAX_PENDING_PROVIDER_CHECKPOINTS {
            return false;
        }
        let count = self.pending_provider_checks.entry(target).or_default();
        *count += 1;
        self.pending_provider_check_count += 1;
        true
    }

    pub fn end_provider_checkpoint(&mut self, target: agent_lifecycle::Target) {
        let Some(count) = self.pending_provider_checks.get_mut(&target) else {
            return;
        };
        *count -= 1;
        self.pending_provider_check_count = self.pending_provider_check_count.saturating_sub(1);
        if *count == 0 {
            self.pending_provider_checks.remove(&target);
        }
    }

    pub fn alert_cursor(&self) -> u64 {
        self.alert_cursor
    }

    pub fn add_provider_route(
        &mut self,
        model: &Model,
        route: ProviderRouteStart,
    ) -> Result<(), String> {
        self.add_provider_route_with_confirmation(model, route, false)
    }

    /// Register provider ownership already backed by an accepted lifecycle
    /// observation. A route start alone remains a reservation until this
    /// evidence or its normal spawned-invocation end arrives.
    pub fn add_confirmed_provider_route(
        &mut self,
        model: &Model,
        route: ProviderRouteStart,
    ) -> Result<(), String> {
        self.add_provider_route_with_confirmation(model, route, true)
    }

    fn add_provider_route_with_confirmation(
        &mut self,
        model: &Model,
        route: ProviderRouteStart,
        confirmed: bool,
    ) -> Result<(), String> {
        if route.route_id.is_nil()
            || route.runtime_id.is_nil()
            || route.generation.is_nil()
            || !agent_lifecycle::has_provider_owner(model, route.target, route.provider)
        {
            return Err("provider route does not identify an exact current terminal".into());
        }
        let mut confirmed_existing = None;
        if let Some(existing) = self
            .routes
            .iter_mut()
            .find(|existing| existing.route_id == route.route_id)
        {
            if existing.target == route.target
                && existing.provider == route.provider
                && existing.runtime_id == route.runtime_id
                && existing.generation == route.generation
                && existing.session_epoch == route.session_epoch
                && existing.command_epoch == route.command_epoch
                && existing.start_cursor == route.cursor
                && existing.transport == route.transport
            {
                if confirmed {
                    existing.confirmed = true;
                    confirmed_existing = Some(*existing);
                }
            } else {
                return Err("provider route ID is already bound to a different owner".into());
            }
            if let Some(existing) = confirmed_existing {
                self.confirm_provider_source(existing);
            }
            return Ok(());
        }
        if self.routes.len() >= MAX_PROVIDER_ROUTES {
            return Err("provider route capacity is exhausted".into());
        }
        if self
            .tracks
            .get(&OwnerKey {
                target: route.target,
                session_epoch: route.session_epoch,
            })
            .is_some_and(|track| {
                track.command_epoch.0 > route.command_epoch.0
                    || (track.command_epoch == route.command_epoch && track.cursor > route.cursor)
            })
        {
            return Err("provider route snapshot is older than the admitted command state".into());
        }
        self.routes.push_back(ProviderRoute {
            target: route.target,
            provider: route.provider,
            route_id: route.route_id,
            runtime_id: route.runtime_id,
            generation: route.generation,
            session_epoch: route.session_epoch,
            command_epoch: route.command_epoch,
            start_cursor: route.cursor,
            transport: route.transport,
            confirmed,
            ended: false,
        });
        if confirmed {
            self.confirm_provider_source(*self.routes.back().expect("route was inserted"));
        }
        // Keep the reservation separate from the consumed journal cursor. The
        // terminal may already contain older transitions that the desktop has
        // not observed yet; those must be arbitrated before this route starts.
        Ok(())
    }

    pub fn end_provider_route(
        &mut self,
        target: agent_lifecycle::Target,
        route_id: Uuid,
        runtime_id: Uuid,
        generation: Uuid,
        transport: Option<TransportOwner>,
    ) -> bool {
        let mut changed = false;
        for route in &mut self.routes {
            if route.target == target
                && route.route_id == route_id
                && route.runtime_id == runtime_id
                && route.generation == generation
                && route.transport == transport
            {
                changed |= !route.confirmed || !route.ended;
                route.confirmed = true;
                route.ended = true;
                if let Some(track) = self.tracks.get_mut(&OwnerKey {
                    target: route.target,
                    session_epoch: route.session_epoch,
                }) && track.command_epoch == route.command_epoch
                    && track.source
                        == Some(AlertSource::Provider {
                            reservation: Some(route.identity()),
                        })
                {
                    track.source = Some(AlertSource::Provider { reservation: None });
                }
            }
        }
        changed
    }

    pub fn cancel_provider_route(
        &mut self,
        target: agent_lifecycle::Target,
        route_id: Uuid,
        runtime_id: Uuid,
        generation: Uuid,
        transport: Option<TransportOwner>,
    ) -> bool {
        let before = self.routes.len();
        let mut removed = Vec::new();
        self.routes.retain(|route| {
            let matches = route.target == target
                && route.route_id == route_id
                && route.runtime_id == runtime_id
                && route.generation == generation
                && route.transport == transport;
            if matches && !route.confirmed {
                removed.push(*route);
                false
            } else {
                true
            }
        });
        for route in removed {
            self.release_predictive_provider_source(route);
        }
        self.routes.len() != before
    }

    /// Recover an exact transport generation after loss/replacement. Unconfirmed
    /// reservations are removed and their predictive lock is released; routes
    /// backed by provider evidence or a normal invocation end remain owned as
    /// ended epochs so earlier journal transitions are still arbitrated.
    pub fn retire_transport_routes(
        &mut self,
        target: agent_lifecycle::Target,
        transport: TransportOwner,
    ) -> bool {
        let mut changed = false;
        let mut removed = Vec::new();
        let mut confirmed = Vec::new();
        self.routes.retain_mut(|route| {
            if route.target != target || route.transport != Some(transport) {
                return true;
            }
            if !route.confirmed {
                removed.push(*route);
                changed = true;
                return false;
            }
            changed |= !route.ended;
            route.ended = true;
            confirmed.push(*route);
            true
        });
        for route in removed {
            self.release_predictive_provider_source(route);
        }
        for route in confirmed {
            self.confirm_route_source(route);
        }
        changed
    }

    fn confirm_provider_source(&mut self, evidence: ProviderRoute) {
        for (key, track) in &mut self.tracks {
            if key.target != evidence.target
                || key.session_epoch != evidence.session_epoch
                || track.command_epoch != evidence.command_epoch
            {
                continue;
            }
            let Some(AlertSource::Provider {
                reservation: Some(reservation),
            }) = track.source
            else {
                continue;
            };
            let same_owner = reservation.provider == evidence.provider
                && reservation.runtime_id == evidence.runtime_id
                && reservation.transport == evidence.transport
                && match evidence.transport {
                    Some(transport) => evidence.generation == transport.generation,
                    None => reservation.generation == evidence.generation,
                };
            if same_owner {
                track.source = Some(AlertSource::Provider { reservation: None });
            }
        }
    }

    fn confirm_route_source(&mut self, route: ProviderRoute) {
        let key = OwnerKey {
            target: route.target,
            session_epoch: route.session_epoch,
        };
        if let Some(track) = self.tracks.get_mut(&key)
            && track.command_epoch == route.command_epoch
            && track.source
                == Some(AlertSource::Provider {
                    reservation: Some(route.identity()),
                })
        {
            track.source = Some(AlertSource::Provider { reservation: None });
        }
    }

    fn release_predictive_provider_source(&mut self, route: ProviderRoute) {
        let key = OwnerKey {
            target: route.target,
            session_epoch: route.session_epoch,
        };
        if let Some(track) = self.tracks.get_mut(&key)
            && track.command_epoch == route.command_epoch
            && track.source
                == Some(AlertSource::Provider {
                    reservation: Some(route.identity()),
                })
        {
            track.source = None;
            track.last_external_alert = None;
        }
    }

    /// Process ordered transitions before native snapshot coalescing. Returns
    /// true when at least one new OSC alert was admitted.
    pub fn observe(&mut self, model: &Model, frames: &[Frame], now: Instant) -> Vec<AlertEvent> {
        self.observe_inner(model, frames, now, None)
    }

    /// Consume one checkpointed terminal through the worker barrier even while
    /// ordinary publication is held for a pending provider admission.
    pub fn observe_checkpoint(
        &mut self,
        model: &Model,
        frames: &[Frame],
        target: agent_lifecycle::Target,
        now: Instant,
    ) -> Vec<AlertEvent> {
        self.observe_inner(model, frames, now, Some(target))
    }

    fn observe_inner(
        &mut self,
        model: &Model,
        frames: &[Frame],
        now: Instant,
        checkpoint_target: Option<agent_lifecycle::Target>,
    ) -> Vec<AlertEvent> {
        let mut owners = Vec::new();
        for frame in frames {
            let Some(snapshot) = frame.snapshot.as_deref() else {
                continue;
            };
            let status = &snapshot.viewport.program_status;
            let Some(owner) = current_owner(model, frame.session_id) else {
                continue;
            };
            owners.push((frame, owner, status));
        }

        let live_keys: Vec<_> = owners
            .iter()
            .map(|(_, owner, status)| OwnerKey {
                target: owner.target,
                session_epoch: status.session_epoch,
            })
            .collect();
        self.tracks.retain(|key, _| {
            live_keys.contains(key) || self.pending_provider_checks.contains_key(&key.target)
        });
        let mut retired_routes = Vec::new();
        self.routes.retain(|route| {
            let retained = self.pending_provider_checks.contains_key(&route.target)
                || live_keys.iter().any(|key| {
                    key.target == route.target
                        && key.session_epoch == route.session_epoch
                        && owners.iter().any(|(frame, owner, status)| {
                            owner.target == route.target
                                && status.session_epoch == route.session_epoch
                                && !frame.reconnectable
                        })
                });
            if !retained {
                retired_routes.push(*route);
            }
            retained
        });
        for route in retired_routes {
            self.release_predictive_provider_source(route);
        }

        let mut emitted = Vec::new();
        for (frame, owner, status) in owners {
            if self.pending_provider_checks.contains_key(&owner.target)
                && checkpoint_target != Some(owner.target)
            {
                continue;
            }
            let key = OwnerKey {
                target: owner.target,
                session_epoch: status.session_epoch,
            };
            let mut track = self.tracks.remove(&key).unwrap_or_else(|| {
                if frame.reconnectable || status.dropped_through.is_some() {
                    baseline(status)
                } else {
                    fresh_owner(status)
                }
            });
            if track.session_epoch != status.session_epoch {
                self.tracks.insert(key, baseline(status));
                continue;
            }

            let has_gap = status
                .dropped_through
                .is_some_and(|dropped| track.cursor < dropped)
                || status
                    .transitions
                    .first()
                    .is_some_and(|first| first.cursor.0 > track.cursor.0.saturating_add(1));
            if has_gap {
                let command_changed = track.command_epoch != status.command_epoch;
                track.cursor = status.cursor;
                track.command_epoch = status.command_epoch;
                if command_changed {
                    track.source = None;
                    track.last_external_alert = None;
                }
                let mut retired_routes = Vec::new();
                self.routes.retain(|route| {
                    let retained = route.target != owner.target
                        || (route.session_epoch == status.session_epoch
                            && route.command_epoch == status.command_epoch
                            && status
                                .dropped_through
                                .is_none_or(|dropped| route.start_cursor >= dropped));
                    if !retained {
                        retired_routes.push(*route);
                    }
                    retained
                });
                for route in retired_routes {
                    if track.source
                        == Some(AlertSource::Provider {
                            reservation: Some(route.identity()),
                        })
                    {
                        track.source = None;
                        track.last_external_alert = None;
                    }
                }
                self.tracks.insert(key, track);
                continue;
            }

            let observed_cursor = track.cursor;
            for transition in status
                .transitions
                .iter()
                .filter(|transition| transition.cursor > observed_cursor)
            {
                if transition.cause == TransitionCause::Prompt {
                    track.source = None;
                    track.last_external_alert = None;
                    track.command_epoch =
                        CommandEpoch(transition.command_epoch.0.saturating_add(1));
                    self.routes.retain(|route| {
                        route.target != owner.target
                            || route.session_epoch != status.session_epoch
                            || route.command_epoch != transition.command_epoch
                    });
                    continue;
                }

                if transition.command_epoch != track.command_epoch {
                    track.source = None;
                    track.last_external_alert = None;
                    track.command_epoch = transition.command_epoch;
                    self.routes.retain(|route| {
                        route.target != owner.target
                            || route.session_epoch != status.session_epoch
                            || route.command_epoch == transition.command_epoch
                    });
                }

                if accepts_alerts(frame)
                    && let Some((kind, record)) = eligible_alert(transition)
                {
                    if track.source.is_none() {
                        track.source = Some(
                            match self.route_active(
                                owner.target,
                                status.session_epoch,
                                transition.command_epoch,
                                transition.cursor,
                            ) {
                                Some(reservation) => AlertSource::Provider {
                                    reservation: Some(reservation),
                                },
                                None => AlertSource::Osc7501,
                            },
                        );
                    }
                    match track.source {
                        Some(AlertSource::Provider { .. }) => {
                            track.suppressed_alert_count =
                                track.suppressed_alert_count.saturating_add(1);
                        }
                        Some(AlertSource::Osc7501) => {
                            let cooled_down = track.last_external_alert.is_some_and(|last| {
                                now.saturating_duration_since(last) < PROGRAM_STATUS_ALERT_COOLDOWN
                            });
                            if cooled_down {
                                track.suppressed_alert_count =
                                    track.suppressed_alert_count.saturating_add(1);
                            } else {
                                track.last_external_alert = Some(now);
                                if let Some(event) =
                                    self.append_alert(owner, status, transition, record, kind)
                                {
                                    emitted.push(event);
                                }
                            }
                        }
                        None => {}
                    }
                }
                track.cursor = transition.cursor;
            }
            if track.command_epoch != status.command_epoch {
                self.routes.retain(|route| {
                    route.target != owner.target
                        || route.session_epoch != status.session_epoch
                        || route.command_epoch == status.command_epoch
                });
                track.command_epoch = status.command_epoch;
                track.source = None;
                track.last_external_alert = None;
            }
            track.cursor = status.cursor;
            self.tracks.insert(key, track);
        }
        emitted
    }

    pub fn provider_alert_allowed(
        &mut self,
        target: agent_lifecycle::Target,
        current_status: &ProgramStatusSnapshot,
    ) -> bool {
        let key = OwnerKey {
            target,
            session_epoch: current_status.session_epoch,
        };
        let track = self
            .tracks
            .entry(key)
            .or_insert_with(|| fresh_owner(current_status));
        if track.command_epoch != current_status.command_epoch {
            track.command_epoch = current_status.command_epoch;
            track.source = None;
            track.last_external_alert = None;
        }
        match track.source {
            Some(AlertSource::Osc7501) => {
                track.suppressed_alert_count = track.suppressed_alert_count.saturating_add(1);
                false
            }
            Some(AlertSource::Provider { .. }) => {
                // Existing provider event/turn deduplication owns its cadence.
                // The OSC cooldown must not suppress a legitimate adapter event.
                track.source = Some(AlertSource::Provider { reservation: None });
                true
            }
            None => {
                track.source = Some(AlertSource::Provider { reservation: None });
                true
            }
        }
    }

    pub fn projection(&self, model: &Model, frames: &[Frame]) -> Projection {
        let focused_surface = focused_surface(model);
        let mut terminals = Vec::new();
        for frame in frames {
            let Some(snapshot) = frame.snapshot.as_deref() else {
                continue;
            };
            let status = &snapshot.viewport.program_status;
            let Some(owner) = current_owner(model, frame.session_id) else {
                continue;
            };
            let mut records = status.records.clone();
            let complete = focused_surface == Some(owner.target.surface_id);
            if !complete && records.len() > 1 {
                records.sort_by_key(|record| {
                    (Reverse(state_priority(record.state)), record.id.clone())
                });
                records.truncate(1);
            }
            let omitted_records = status.records.len().saturating_sub(records.len());
            let suppressed_alert_count = self
                .tracks
                .get(&OwnerKey {
                    target: owner.target,
                    session_epoch: status.session_epoch,
                })
                .map_or(0, |track| track.suppressed_alert_count);
            terminals.push(Terminal {
                source: "osc7501",
                identity_namespace: owner.target.identity_namespace,
                window_id: owner.window_id,
                window_uuid: owner.window_uuid,
                workspace_id: owner.workspace_id,
                workspace_uuid: owner.workspace_uuid,
                pane_id: owner.pane_id,
                pane_uuid: owner.pane_uuid,
                surface_id: owner.target.surface_id,
                surface_uuid: owner.target.surface_uuid,
                session_id: owner.target.session_id,
                session_epoch: status.session_epoch,
                command_epoch: status.command_epoch,
                availability: if !frame.starting
                    && !frame.reconnectable
                    && snapshot.state == zmux_session_runtime::State::Running
                {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                record_count: status.records.len(),
                records,
                records_complete: complete || status.records.len() <= 1,
                omitted_records,
                suppressed_alert_count,
            });
        }
        terminals.sort_by_key(|terminal| {
            (
                terminal.window_id.0,
                terminal.workspace_id.0,
                terminal.pane_id.0,
                terminal.surface_id.0,
            )
        });
        let current_owners: Vec<_> = terminals
            .iter()
            .map(|terminal| {
                (
                    terminal.identity_namespace,
                    terminal.surface_id,
                    terminal.surface_uuid,
                    terminal.session_id,
                    terminal.session_epoch,
                )
            })
            .collect();
        let events = self
            .alert_events
            .iter()
            .filter(|event| {
                current_owners
                    .iter()
                    .any(|(namespace, surface, surface_uuid, session, epoch)| {
                        event.identity_namespace == *namespace
                            && event.surface_id == *surface
                            && event.surface_uuid == *surface_uuid
                            && event.session_id == *session
                            && event.session_epoch == *epoch
                    })
            })
            .cloned()
            .collect();
        let mut projection = Projection {
            terminals,
            alerts: AlertSnapshot {
                epoch: self.alert_epoch,
                cursor: self.alert_cursor,
                dropped_through_cursor: self.alert_dropped_through_cursor,
                events,
            },
        };
        enforce_projection_budget(&mut projection, focused_surface);
        projection
    }

    pub fn matches_selection(
        &self,
        target: &SelectionTarget,
        model: &Model,
        frames: &[Frame],
    ) -> bool {
        let Some(owner) = current_owner(model, target.session_id) else {
            return false;
        };
        if owner.target.identity_namespace != target.identity_namespace
            || owner.window_id != target.window_id
            || owner.window_uuid != target.window_uuid
            || owner.workspace_id != target.workspace_id
            || owner.workspace_uuid != target.workspace_uuid
            || owner.pane_id != target.pane_id
            || owner.pane_uuid != target.pane_uuid
            || owner.target.surface_id != target.surface_id
            || owner.target.surface_uuid != target.surface_uuid
            || owner.target.session_id != target.session_id
        {
            return false;
        }
        frames.iter().any(|frame| {
            frame.session_id == target.session_id
                && frame.snapshot.as_ref().is_some_and(|snapshot| {
                    let status = &snapshot.viewport.program_status;
                    status.session_epoch == target.session_epoch
                        && status.command_epoch == target.command_epoch
                        && status
                            .records
                            .iter()
                            .any(|record| record.id.as_ref() == target.record_id)
                })
        })
    }

    fn route_active(
        &self,
        target: agent_lifecycle::Target,
        session_epoch: SessionEpoch,
        command_epoch: CommandEpoch,
        cursor: zmux_program_status::TransitionCursor,
    ) -> Option<ProviderRouteIdentity> {
        self.routes
            .iter()
            .find(|route| {
                route.target == target
                    && route.session_epoch == session_epoch
                    && route.command_epoch == command_epoch
                    && route.start_cursor < cursor
            })
            .map(|route| route.identity())
    }

    fn append_alert(
        &mut self,
        owner: CurrentOwner,
        status: &ProgramStatusSnapshot,
        transition: &Transition,
        record: &Record,
        kind: AlertKind,
    ) -> Option<AlertEvent> {
        let cursor = self.alert_cursor.checked_add(1)?;
        let event = AlertEvent {
            cursor,
            source: "osc7501",
            identity_namespace: owner.target.identity_namespace,
            window_id: owner.window_id,
            window_uuid: owner.window_uuid,
            workspace_id: owner.workspace_id,
            workspace_uuid: owner.workspace_uuid,
            pane_id: owner.pane_id,
            pane_uuid: owner.pane_uuid,
            surface_id: owner.target.surface_id,
            surface_uuid: owner.target.surface_uuid,
            session_id: owner.target.session_id,
            session_epoch: status.session_epoch,
            command_epoch: transition.command_epoch,
            record_id: record.id.to_string(),
            kind,
            app: record
                .effective_app
                .as_deref()
                .or(record.app.as_deref())
                .map(str::to_owned),
            title: record
                .title
                .as_deref()
                .map(|value| excerpt(value, MAX_EVENT_TITLE_BYTES)),
            message: record
                .msg
                .as_deref()
                .map(|value| excerpt(value, MAX_EVENT_MESSAGE_BYTES)),
        };
        self.alert_cursor = cursor;
        self.alert_events.push_back(event.clone());
        if self.alert_events.len() > MAX_PROGRAM_STATUS_ALERT_EVENTS
            && let Some(dropped) = self.alert_events.pop_front()
        {
            self.alert_dropped_through_cursor =
                self.alert_dropped_through_cursor.max(dropped.cursor);
        }
        Some(event)
    }
}

fn projection_bytes(projection: &Projection) -> usize {
    serde_json::to_vec(projection)
        .expect("program-status projection is always JSON serializable")
        .len()
}

/// Reserve at most 768 KiB for the complete status and alert projection. This
/// leaves headroom inside the 1 MiB control response for the workspace and
/// existing lifecycle fields. Journal transitions are never serialized here.
fn enforce_projection_budget(projection: &mut Projection, focused_surface: Option<SurfaceId>) {
    while projection_bytes(projection) > MAX_PROGRAM_STATUS_PROJECTION_BYTES {
        if projection.alerts.events.is_empty() {
            break;
        }
        let dropped = projection.alerts.events.remove(0);
        projection.alerts.dropped_through_cursor =
            projection.alerts.dropped_through_cursor.max(dropped.cursor);
    }

    if projection_bytes(projection) <= MAX_PROGRAM_STATUS_PROJECTION_BYTES {
        return;
    }

    // Keep nonfocused rows useful as summaries while shedding their least
    // essential user-provided text first. The full record set remains focused.
    for terminal in &mut projection.terminals {
        if Some(terminal.surface_id) != focused_surface {
            for record in &mut terminal.records {
                record.title = None;
                record.msg = None;
                record.app = None;
                record.effective_app = None;
            }
        }
    }

    while projection_bytes(projection) > MAX_PROGRAM_STATUS_PROJECTION_BYTES {
        let candidate = projection
            .terminals
            .iter()
            .enumerate()
            .filter(|(_, terminal)| terminal.records.len() > 1)
            .flat_map(|(terminal_index, terminal)| {
                terminal
                    .records
                    .iter()
                    .enumerate()
                    .map(move |(record_index, record)| {
                        (
                            terminal.surface_id == focused_surface.unwrap_or(terminal.surface_id),
                            state_priority(record.state),
                            record.id.as_ref(),
                            terminal_index,
                            record_index,
                        )
                    })
            })
            .min_by_key(|(focused, priority, id, _, _)| (*focused, *priority, *id));
        let Some((_, _, _, terminal_index, record_index)) = candidate else {
            break;
        };
        let terminal = &mut projection.terminals[terminal_index];
        terminal.records.remove(record_index);
        terminal.omitted_records = terminal.omitted_records.saturating_add(1);
        terminal.records_complete = false;
    }

    if projection_bytes(projection) <= MAX_PROGRAM_STATUS_PROJECTION_BYTES {
        return;
    }

    // The configured protocol maxima fit comfortably below the budget. This
    // final fallback still enforces the wire bound if a future DTO grows.
    for terminal in &mut projection.terminals {
        terminal.omitted_records = terminal
            .omitted_records
            .saturating_add(terminal.records.len());
        terminal.records.clear();
        terminal.records_complete = false;
    }
    while projection_bytes(projection) > MAX_PROGRAM_STATUS_PROJECTION_BYTES {
        if projection.alerts.events.is_empty() {
            break;
        }
        let dropped = projection.alerts.events.remove(0);
        projection.alerts.dropped_through_cursor =
            projection.alerts.dropped_through_cursor.max(dropped.cursor);
    }
    debug_assert!(projection_bytes(projection) <= MAX_PROGRAM_STATUS_PROJECTION_BYTES);
}

fn baseline(snapshot: &ProgramStatusSnapshot) -> OwnerTrack {
    OwnerTrack {
        session_epoch: snapshot.session_epoch,
        command_epoch: snapshot.command_epoch,
        cursor: snapshot.cursor,
        source: None,
        last_external_alert: None,
        suppressed_alert_count: 0,
    }
}

fn fresh_owner(snapshot: &ProgramStatusSnapshot) -> OwnerTrack {
    let cursor = snapshot
        .transitions
        .iter()
        .filter(|transition| transition.command_epoch.0 < snapshot.command_epoch.0)
        .map(|transition| transition.cursor)
        .max()
        .unwrap_or_default();
    OwnerTrack {
        session_epoch: snapshot.session_epoch,
        command_epoch: snapshot.command_epoch,
        cursor,
        source: None,
        last_external_alert: None,
        suppressed_alert_count: 0,
    }
}

fn accepts_alerts(frame: &Frame) -> bool {
    !frame.reconnectable
        && frame.snapshot.as_ref().is_some_and(|snapshot| {
            matches!(
                snapshot.state,
                zmux_session_runtime::State::Running | zmux_session_runtime::State::Exited
            )
        })
}

fn eligible_alert(transition: &Transition) -> Option<(AlertKind, &Record)> {
    if transition.cause != TransitionCause::Report {
        return None;
    }
    let record = transition.after_record.as_ref()?;
    let kind = match record.state {
        State::Blocked if transition.before_state != Some(State::Blocked) => AlertKind::NeedsUser,
        State::Done
            if matches!(
                transition.before_state,
                Some(State::Working | State::Blocked)
            ) =>
        {
            AlertKind::Completed
        }
        State::Error => AlertKind::NeedsUser,
        _ => return None,
    };
    Some((kind, record))
}

fn state_priority(state: State) -> u8 {
    match state {
        State::Blocked => 5,
        State::Error => 4,
        State::Working => 3,
        State::Done => 2,
        State::Idle => 1,
    }
}

fn excerpt(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn focused_surface(model: &Model) -> Option<SurfaceId> {
    let workspace = model
        .focused_workspace
        .and_then(|id| model.workspaces.iter().find(|workspace| workspace.id == id))?;
    workspace
        .root
        .panes()
        .into_iter()
        .find(|pane| pane.id == workspace.focused_pane)
        .map(|pane| pane.selected_surface)
}

fn current_owner(model: &Model, session_id: SessionId) -> Option<CurrentOwner> {
    for window in &model.windows {
        for workspace_id in &window.workspace_ids {
            let Some(workspace) = model
                .workspaces
                .iter()
                .find(|item| item.id == *workspace_id)
            else {
                continue;
            };
            for pane in workspace.root.panes() {
                for surface in &pane.surfaces {
                    if surface.kind != zmux_workspace_core::SurfaceKind::Terminal
                        || surface.session_id != Some(session_id)
                    {
                        continue;
                    }
                    return Some(CurrentOwner {
                        target: agent_lifecycle::Target {
                            identity_namespace: model.identity_namespace,
                            surface_id: surface.id,
                            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
                            session_id,
                        },
                        window_id: window.id,
                        window_uuid: model.object_uuid(TargetKind::Window, window.id.0),
                        workspace_id: workspace.id,
                        workspace_uuid: model.object_uuid(TargetKind::Workspace, workspace.id.0),
                        pane_id: pane.id,
                        pane_uuid: model.object_uuid(TargetKind::Pane, pane.id.0),
                    });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use zmux_program_status::{ProgramStatusSnapshot, TransitionCursor};
    use zmux_terminal_adapter::{Size, Terminal};
    use zmux_workspace_core::{Action, WorkspaceStore, targets::TargetKind};

    fn owner_fixture() -> (Model, SessionId) {
        let namespace = Uuid::new_v4();
        let mut store = WorkspaceStore::with_identity_namespace(namespace);
        store
            .apply(Action::CreateWorkspace {
                name: "Program status fixture".into(),
            })
            .unwrap();
        let model = store.snapshot();
        let session_id = model.workspaces[0].root.panes()[0]
            .selected()
            .session_id
            .unwrap();
        (model, session_id)
    }

    fn frame(session_id: SessionId, status: ProgramStatusSnapshot) -> Frame {
        let terminal = Terminal::new(Size { rows: 24, cols: 80 }).unwrap();
        let mut viewport = terminal.viewport();
        viewport.program_status = status;
        Frame {
            session_id,
            process_id: Some(1),
            starting: false,
            reconnectable: false,
            snapshot: Some(Arc::new(zmux_session_runtime::Snapshot {
                state: zmux_session_runtime::State::Running,
                viewport,
                bytes_read: 0,
                exit_code: None,
                failure: None,
            })),
            failure: None,
        }
    }

    fn record(id: &str, state: State, title: &str, message: &str) -> Record {
        Record {
            id: Arc::from(id),
            state,
            kind: None,
            progress: None,
            app: Some(Arc::from("fixture")),
            effective_app: None,
            msg: Some(Arc::from(message)),
            title: Some(Arc::from(title)),
        }
    }

    fn status(
        command_epoch: u64,
        cursor: u64,
        dropped_through: Option<u64>,
        transitions: Vec<Transition>,
    ) -> ProgramStatusSnapshot {
        ProgramStatusSnapshot {
            session_epoch: SessionEpoch(71),
            command_epoch: CommandEpoch(command_epoch),
            cursor: TransitionCursor(cursor),
            dropped_through: dropped_through.map(TransitionCursor),
            records: Vec::new(),
            transitions,
        }
    }

    fn report(
        cursor: u64,
        command_epoch: u64,
        id: &str,
        before_state: Option<State>,
        after_record: Option<Record>,
    ) -> Transition {
        Transition {
            cursor: TransitionCursor(cursor),
            command_epoch: CommandEpoch(command_epoch),
            id: Arc::from(id),
            before_state,
            after_record,
            cause: TransitionCause::Report,
        }
    }

    #[test]
    fn a_new_owner_consumes_fast_completion_before_a_same_read_clear() {
        let (model, session_id) = owner_fixture();
        let mut runtime = Runtime::default();
        let status = status(
            0,
            3,
            None,
            vec![
                report(
                    1,
                    0,
                    "root",
                    None,
                    Some(record("root", State::Working, "", "")),
                ),
                report(
                    2,
                    0,
                    "root",
                    Some(State::Working),
                    Some(record("root", State::Done, "Finished", "Work completed")),
                ),
                Transition {
                    cursor: TransitionCursor(3),
                    command_epoch: CommandEpoch(0),
                    id: Arc::from("root"),
                    before_state: Some(State::Done),
                    after_record: None,
                    cause: TransitionCause::Clear,
                },
            ],
        );

        let alerts = runtime.observe(&model, &[frame(session_id, status)], Instant::now());

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Completed);
        assert_eq!(alerts[0].title.as_deref(), Some("Finished"));
        assert_eq!(alerts[0].message.as_deref(), Some("Work completed"));
        assert_eq!(alerts[0].record_id, "root");
    }

    #[test]
    fn a_new_owner_consumes_an_initial_blocked_transition() {
        let (model, session_id) = owner_fixture();
        let mut runtime = Runtime::default();
        let status = status(
            0,
            1,
            None,
            vec![report(
                1,
                0,
                "permission",
                Some(State::Working),
                Some(record(
                    "permission",
                    State::Blocked,
                    "Approve",
                    "Needs approval",
                )),
            )],
        );

        let alerts = runtime.observe(&model, &[frame(session_id, status)], Instant::now());

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::NeedsUser);
        assert_eq!(alerts[0].message.as_deref(), Some("Needs approval"));
    }

    #[test]
    fn recovery_after_a_transition_gap_baselines_without_replaying_alerts() {
        let (model, session_id) = owner_fixture();
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        let recovered = status(
            0,
            2,
            Some(1),
            vec![report(
                2,
                0,
                "permission",
                Some(State::Working),
                Some(record(
                    "permission",
                    State::Blocked,
                    "Approve",
                    "Needs approval",
                )),
            )],
        );

        assert!(
            runtime
                .observe(&model, &[frame(session_id, recovered)], Instant::now())
                .is_empty()
        );
        assert_eq!(runtime.alert_cursor(), 0);
    }

    #[test]
    fn ended_old_command_route_survives_until_its_earlier_journal_events_are_consumed() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        let route_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        runtime
            .add_provider_route(
                &model,
                ProviderRouteStart {
                    target,
                    provider: agent_lifecycle::Provider::CodexLocal,
                    route_id,
                    runtime_id,
                    generation,
                    session_epoch: SessionEpoch(71),
                    command_epoch: CommandEpoch(0),
                    cursor: TransitionCursor(0),
                    transport: None,
                },
            )
            .unwrap();
        assert!(runtime.end_provider_route(target, route_id, runtime_id, generation, None));

        let old_command_events = status(
            1,
            2,
            None,
            vec![
                report(
                    1,
                    0,
                    "permission",
                    Some(State::Working),
                    Some(record(
                        "permission",
                        State::Blocked,
                        "Approve",
                        "Needs approval",
                    )),
                ),
                Transition {
                    cursor: TransitionCursor(2),
                    command_epoch: CommandEpoch(0),
                    id: Arc::from(""),
                    before_state: None,
                    after_record: None,
                    cause: TransitionCause::Prompt,
                },
            ],
        );

        assert!(
            runtime
                .observe(
                    &model,
                    &[frame(session_id, old_command_events)],
                    Instant::now()
                )
                .is_empty()
        );
        assert_eq!(runtime.alert_cursor(), 0);
    }

    #[test]
    fn cancelled_unspawned_route_does_not_claim_the_command_epoch() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        let route_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        runtime
            .add_provider_route(
                &model,
                ProviderRouteStart {
                    target,
                    provider: agent_lifecycle::Provider::CodexLocal,
                    route_id,
                    runtime_id,
                    generation,
                    session_epoch: SessionEpoch(71),
                    command_epoch: CommandEpoch(0),
                    cursor: TransitionCursor(0),
                    transport: None,
                },
            )
            .unwrap();
        assert!(runtime.cancel_provider_route(target, route_id, runtime_id, generation, None));

        let alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    1,
                    None,
                    vec![report(
                        1,
                        0,
                        "permission",
                        Some(State::Working),
                        Some(record(
                            "permission",
                            State::Blocked,
                            "Approve",
                            "Needs approval",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::NeedsUser);
    }

    #[test]
    fn retiring_an_unconfirmed_transport_route_releases_its_predictive_source_lock() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let transport = TransportOwner {
            runtime_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let route_id = Uuid::new_v4();
        let runtime_id = transport.runtime_id;
        let generation = Uuid::new_v4();
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        runtime
            .add_provider_route(
                &model,
                ProviderRouteStart {
                    target,
                    provider: agent_lifecycle::Provider::CodexLocal,
                    route_id,
                    runtime_id,
                    generation,
                    session_epoch: SessionEpoch(71),
                    command_epoch: CommandEpoch(0),
                    cursor: TransitionCursor(0),
                    transport: Some(transport),
                },
            )
            .unwrap();

        let reserved_alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    1,
                    None,
                    vec![report(
                        1,
                        0,
                        "reserved",
                        None,
                        Some(record(
                            "reserved",
                            State::Blocked,
                            "Waiting",
                            "Waiting for approval",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );
        assert!(reserved_alerts.is_empty());

        assert!(runtime.retire_transport_routes(target, transport));
        let alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    2,
                    None,
                    vec![report(
                        2,
                        0,
                        "after-reconnect",
                        None,
                        Some(record(
                            "after-reconnect",
                            State::Blocked,
                            "Approval needed",
                            "The provider route never received admission",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::NeedsUser);
        assert_eq!(alerts[0].record_id, "after-reconnect");
    }

    #[test]
    fn provider_alert_confirms_a_predictive_route_lock_before_cancellation() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let route_id = Uuid::new_v4();
        let runtime_id = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        runtime
            .add_provider_route(
                &model,
                ProviderRouteStart {
                    target,
                    provider: agent_lifecycle::Provider::CodexLocal,
                    route_id,
                    runtime_id,
                    generation,
                    session_epoch: SessionEpoch(71),
                    command_epoch: CommandEpoch(0),
                    cursor: TransitionCursor(0),
                    transport: None,
                },
            )
            .unwrap();

        let reserved_alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    1,
                    None,
                    vec![report(
                        1,
                        0,
                        "reserved",
                        None,
                        Some(record("reserved", State::Blocked, "Waiting", "Approve")),
                    )],
                ),
            )],
            Instant::now(),
        );
        assert!(reserved_alerts.is_empty());
        assert!(runtime.provider_alert_allowed(target, &status(0, 1, None, Vec::new())));
        assert!(runtime.cancel_provider_route(target, route_id, runtime_id, generation, None));

        let alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    2,
                    None,
                    vec![report(
                        2,
                        0,
                        "after-provider",
                        None,
                        Some(record(
                            "after-provider",
                            State::Blocked,
                            "Still owned",
                            "Approve",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );

        assert!(alerts.is_empty());
    }

    #[test]
    fn transport_retirement_preserves_ownership_of_a_normally_ended_route() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let transport = TransportOwner {
            runtime_id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let route_id = Uuid::new_v4();
        let runtime_id = transport.runtime_id;
        let generation = Uuid::new_v4();
        let mut runtime = Runtime::default();
        runtime.observe(
            &model,
            &[frame(session_id, status(0, 0, None, Vec::new()))],
            Instant::now(),
        );
        runtime
            .add_provider_route(
                &model,
                ProviderRouteStart {
                    target,
                    provider: agent_lifecycle::Provider::CodexLocal,
                    route_id,
                    runtime_id,
                    generation,
                    session_epoch: SessionEpoch(71),
                    command_epoch: CommandEpoch(0),
                    cursor: TransitionCursor(0),
                    transport: Some(transport),
                },
            )
            .unwrap();

        assert!(runtime.end_provider_route(
            target,
            route_id,
            runtime_id,
            generation,
            Some(transport),
        ));
        runtime.retire_transport_routes(target, transport);
        let alerts = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    0,
                    1,
                    None,
                    vec![report(
                        1,
                        0,
                        "after-exit",
                        None,
                        Some(record(
                            "after-exit",
                            State::Blocked,
                            "Still owned",
                            "The invocation was spawned and ended",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );

        assert!(alerts.is_empty());
    }

    #[test]
    fn provider_alerts_are_not_limited_by_the_osc_cooldown() {
        let (model, session_id) = owner_fixture();
        let surface = model.workspaces[0].root.panes()[0].selected();
        let target = agent_lifecycle::Target {
            identity_namespace: model.identity_namespace,
            surface_id: surface.id,
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
            session_id,
        };
        let mut runtime = Runtime::default();
        let current = status(0, 0, None, Vec::new());
        runtime.observe(
            &model,
            &[frame(session_id, current.clone())],
            Instant::now(),
        );

        assert!(runtime.provider_alert_allowed(target, &current));
        assert!(runtime.provider_alert_allowed(target, &current));
    }

    #[test]
    fn provider_and_osc_alerts_arbitrate_once_per_command() {
        let (model, session_id) = owner_fixture();
        let owner = current_owner(&model, session_id).unwrap();
        let mut runtime = Runtime::default();
        let initial = status(1, 0, None, Vec::new());
        runtime.observe(
            &model,
            &[frame(session_id, initial.clone())],
            Instant::now(),
        );

        // An adapter alert already owns this command. The OSC transition must
        // be consumed without creating a competing native notification.
        assert!(runtime.provider_alert_allowed(owner.target, &initial));
        let provider_first = runtime.observe(
            &model,
            &[frame(
                session_id,
                status(
                    1,
                    1,
                    None,
                    vec![report(
                        1,
                        1,
                        "permission",
                        Some(State::Working),
                        Some(record(
                            "permission",
                            State::Blocked,
                            "Approve",
                            "Needs approval",
                        )),
                    )],
                ),
            )],
            Instant::now(),
        );
        assert!(provider_first.is_empty());

        // A new command clears the source lock. If OSC wins first, the existing
        // provider alert path is suppressed for that same command.
        let osc_status = status(
            2,
            3,
            None,
            vec![
                Transition {
                    cursor: TransitionCursor(2),
                    command_epoch: CommandEpoch(1),
                    id: Arc::from(""),
                    before_state: None,
                    after_record: None,
                    cause: TransitionCause::Prompt,
                },
                report(
                    3,
                    2,
                    "question",
                    Some(State::Working),
                    Some(record("question", State::Blocked, "Answer", "Needs input")),
                ),
            ],
        );
        let osc_wins = runtime.observe(
            &model,
            &[frame(session_id, osc_status.clone())],
            Instant::now(),
        );
        assert_eq!(osc_wins.len(), 1);
        assert_eq!(osc_wins[0].kind, AlertKind::NeedsUser);
        assert!(!runtime.provider_alert_allowed(owner.target, &osc_status));
    }

    #[test]
    fn provider_event_in_new_command_escapes_prior_command_osc_lock() {
        let (model, session_id) = owner_fixture();
        let owner = current_owner(&model, session_id).unwrap();
        let mut runtime = Runtime::default();
        let command_a = status(
            0,
            1,
            None,
            vec![report(
                1,
                0,
                "command-a",
                Some(State::Working),
                Some(record("command-a", State::Done, "A finished", "done")),
            )],
        );
        let osc_alert = runtime.observe(&model, &[frame(session_id, command_a)], Instant::now());
        assert_eq!(osc_alert.len(), 1);

        // The desktop has not consumed the prompt transition yet. The worker
        // checkpoint already identifies command B, so command A's OSC lock is
        // irrelevant to this provider event.
        let command_b = status(
            1,
            2,
            None,
            vec![Transition {
                cursor: TransitionCursor(2),
                command_epoch: CommandEpoch(1),
                id: Arc::from(""),
                before_state: None,
                after_record: None,
                cause: TransitionCause::Prompt,
            }],
        );
        assert!(runtime.provider_alert_allowed(owner.target, &command_b));
    }

    #[test]
    fn pending_provider_checkpoint_holds_osc_until_exact_status_is_consumed() {
        let (model, session_id) = owner_fixture();
        let owner = current_owner(&model, session_id).unwrap();
        let mut runtime = Runtime::default();
        assert!(runtime.begin_provider_checkpoint(owner.target));

        let command_b = status(
            1,
            2,
            None,
            vec![
                Transition {
                    cursor: TransitionCursor(1),
                    command_epoch: CommandEpoch(0),
                    id: Arc::from(""),
                    before_state: None,
                    after_record: None,
                    cause: TransitionCause::Prompt,
                },
                report(
                    2,
                    1,
                    "command-b",
                    Some(State::Working),
                    Some(record("command-b", State::Blocked, "B waiting", "approval")),
                ),
            ],
        );
        let frame = frame(session_id, command_b);
        assert!(
            runtime
                .observe(&model, std::slice::from_ref(&frame), Instant::now())
                .is_empty()
        );

        let alerts = runtime.observe_checkpoint(&model, &[frame], owner.target, Instant::now());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::NeedsUser);
        runtime.end_provider_checkpoint(owner.target);
    }

    #[test]
    fn native_program_selection_uses_id_and_rejects_stale_command_or_record() {
        let (model, session_id) = owner_fixture();
        let owner = current_owner(&model, session_id).unwrap();
        let runtime = Runtime::default();
        let selection = SelectionTarget {
            identity_namespace: owner.target.identity_namespace,
            window_id: owner.window_id,
            window_uuid: owner.window_uuid,
            workspace_id: owner.workspace_id,
            workspace_uuid: owner.workspace_uuid,
            pane_id: owner.pane_id,
            pane_uuid: owner.pane_uuid,
            surface_id: owner.target.surface_id,
            surface_uuid: owner.target.surface_uuid,
            session_id,
            session_epoch: SessionEpoch(71),
            command_epoch: CommandEpoch(2),
            record_id: "question".into(),
        };
        let encoded = serde_json::to_value(&selection).unwrap();
        assert_eq!(encoded["id"], "question");
        assert!(encoded.get("record_id").is_none());
        let decoded: SelectionTarget = serde_json::from_value(encoded).unwrap();
        let mut current_status = status(
            2,
            3,
            None,
            vec![report(
                3,
                2,
                "question",
                Some(State::Working),
                Some(record("question", State::Blocked, "Answer", "Needs input")),
            )],
        );
        current_status
            .records
            .push(record("question", State::Blocked, "Answer", "Needs input"));
        let current = frame(session_id, current_status);
        assert!(runtime.matches_selection(&decoded, &model, std::slice::from_ref(&current)));

        let mut stale_command = decoded.clone();
        stale_command.command_epoch = CommandEpoch(1);
        assert!(!runtime.matches_selection(&stale_command, &model, std::slice::from_ref(&current)));
        let mut stale_record = decoded;
        stale_record.record_id = "permission".into();
        assert!(!runtime.matches_selection(&stale_record, &model, &[current]));
    }

    #[test]
    fn status_projection_is_capped_and_keeps_focused_full_records_at_protocol_maxima() {
        let (model, session_id) = owner_fixture();
        let runtime = Runtime::default();
        let records = (0..64)
            .map(|index| Record {
                id: Arc::from(format!("agent-{index:02}-{}", "i".repeat(112))),
                state: State::Working,
                kind: None,
                progress: Some(50),
                app: Some(Arc::from("a".repeat(32))),
                effective_app: Some(Arc::from("b".repeat(32))),
                msg: Some(Arc::from("m".repeat(2048))),
                title: Some(Arc::from("t".repeat(192))),
            })
            .collect();
        let mut current = status(0, 0, None, Vec::new());
        current.records = records;
        let projection = runtime.projection(&model, &[frame(session_id, current)]);
        let size = projection_bytes(&projection);

        assert!(size <= MAX_PROGRAM_STATUS_PROJECTION_BYTES, "{size} bytes");
        assert_eq!(projection.terminals.len(), 1);
        assert_eq!(projection.terminals[0].records.len(), 64);
        assert!(projection.terminals[0].records_complete);
    }
}
