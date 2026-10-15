//! Independent native-download lifetimes, bounded history and once-only wait tickets.
//! Document navigation never changes a captured download origin. Filesystem observers
//! live on a separate fixed worker, and pages cannot submit native lifecycle events.
use crate::{
    browser::BrowserOwner,
    download_paths::{Observation, PathObservers},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{
    Snapshot, SurfaceId, SurfaceKind,
    targets::{CallerContext, TargetKind, TargetScope},
};

pub const MAX_ACTIVE: usize = 4;
pub const MAX_ACTIVE_PER_SURFACE: usize = 2;
pub const MAX_HISTORY: usize = 64;
pub const MAX_HISTORY_PER_SURFACE: usize = 25;
pub const MAX_WAITERS: usize = 16;
// Leave 192 KiB for sixteen independent ticket results/paths and queue metadata.
const MAX_HISTORY_BYTES: usize = 320 * 1024;
const MAX_RECORD_BYTES: usize = 7 * 1024;
pub const DEFAULT_WAIT_MS: u64 = 10_000;
pub const MAX_WAIT_MS: u64 = 120_000;

#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct DownloadError {
    pub code: &'static str,
    pub message: String,
}
fn error(code: &'static str, message: impl Into<String>) -> DownloadError {
    DownloadError {
        code,
        message: message.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadToken {
    pub epoch: Uuid,
    pub download_id: Uuid,
    pub host_id: Uuid,
}
#[derive(Clone, Debug, Serialize)]
pub struct DownloadOrigin {
    pub browser: BrowserOwner,
    pub surface_uuid: Uuid,
    pub profile_id: Option<Uuid>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    AwaitingDestination,
    Downloading,
    Finalizing,
    Cancelling,
    Saved,
    Cancelled,
    Failed,
}
impl DownloadState {
    fn terminal(self) -> bool {
        matches!(self, Self::Saved | Self::Cancelled | Self::Failed)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct DownloadRecord {
    #[serde(flatten)]
    pub token: DownloadToken,
    pub origin: DownloadOrigin,
    pub filename: String,
    pub status: DownloadState,
    pub path: Option<String>,
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
    pub cleanup_complete: bool,
    // An observation from native publication, never a synchronous stat in list.
    pub path_exists: Option<bool>,
}
struct ActiveRecord {
    record: DownloadRecord,
    physical: bool,
    started: Instant,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeDownload {
    Begin {
        owner: BrowserOwner,
        token: DownloadToken,
        filename: String,
    },
    Destination {
        token: DownloadToken,
        filename: String,
    },
    Progress {
        token: DownloadToken,
        bytes: u64,
        total_bytes: Option<u64>,
    },
    Finalizing {
        token: DownloadToken,
    },
    Saved {
        token: DownloadToken,
        path: String,
        bytes: u64,
        cleanup_complete: bool,
        message: Option<String>,
    },
    Failed {
        token: DownloadToken,
        message: String,
        cleanup_complete: bool,
    },
    Cancelled {
        token: DownloadToken,
        cleanup_complete: bool,
    },
    Drain,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadRequest {
    #[serde(default)]
    pub scope: TargetScope,
    #[serde(default)]
    pub caller: CallerContext,
    pub action: DownloadAction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DownloadAction {
    List {
        #[serde(default)]
        limit: Option<usize>,
    },
    Wait {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Cancel {
        download_id: Uuid,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct WaitReceipt {
    pub epoch: Uuid,
    pub ticket_id: Uuid,
    pub surface_uuid: Uuid,
    pub state: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}
struct Ticket {
    receipt: WaitReceipt,
    surface: SurfaceId,
    deadline: Instant,
    path: Option<String>,
    physical: bool,
}

pub struct DownloadRuntime {
    pub epoch: Uuid,
    records: VecDeque<ActiveRecord>,
    events: VecDeque<Uuid>,
    tickets: VecDeque<Ticket>,
    paths: Option<PathObservers>,
}
impl Default for DownloadRuntime {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4(),
            records: VecDeque::new(),
            events: VecDeque::new(),
            tickets: VecDeque::new(),
            paths: None,
        }
    }
}
fn bounded(value: &str, bytes: usize) -> bool {
    !value.chars().any(char::is_control)
        && serde_json::to_vec(value).is_ok_and(|v| v.len() <= bytes)
}
fn validate_record(record: &DownloadRecord) -> Result<(), DownloadError> {
    if !serde_json::to_vec(record).is_ok_and(|v| v.len() <= MAX_RECORD_BYTES) {
        return Err(error(
            "download_metadata_capacity",
            "Download record exceeds its metadata budget",
        ));
    }
    Ok(())
}
fn valid_filename(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.contains(['/', '\\'])
        && bounded(name, 512)
}
fn valid_path(path: &str) -> bool {
    PathBuf::from(path).is_absolute() && bounded(path, 4096)
}

impl DownloadRuntime {
    pub fn wait_receipts(&self) -> Vec<WaitReceipt> {
        self.tickets.iter().map(|t| t.receipt.clone()).collect()
    }
    pub fn records(&self) -> Vec<DownloadRecord> {
        self.records.iter().map(|r| r.record.clone()).collect()
    }
    pub fn begin(
        &mut self,
        model: &Snapshot,
        owner: BrowserOwner,
        token: DownloadToken,
        filename: String,
        now: Instant,
    ) -> Result<DownloadToken, DownloadError> {
        let surface = model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|s| s.id == owner.surface_id && s.kind == SurfaceKind::Browser)
            .ok_or_else(|| error("stale_download_origin", "Download surface is unavailable"))?;
        if owner.identity_namespace != model.identity_namespace || !valid_filename(&filename) {
            return Err(error(
                "invalid_download_origin",
                "Download origin or filename is invalid",
            ));
        }
        if self.records.iter().filter(|r| r.physical).count() >= MAX_ACTIVE
            || self
                .records
                .iter()
                .filter(|r| r.physical && r.record.origin.browser.surface_id == surface.id)
                .count()
                >= MAX_ACTIVE_PER_SURFACE
        {
            return Err(error(
                "download_capacity",
                "Native download capacity is full",
            ));
        }
        if token.epoch != self.epoch
            || token.host_id != owner.host_id
            || token.download_id.is_nil()
            || self
                .records
                .iter()
                .any(|r| r.record.token.download_id == token.download_id)
        {
            return Err(error(
                "invalid_download_token",
                "Download admission token is stale or already used",
            ));
        }
        let record = DownloadRecord {
            token: token.clone(),
            origin: DownloadOrigin {
                browser: owner,
                surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
                profile_id: surface.browser_profile_id,
            },
            filename,
            status: DownloadState::AwaitingDestination,
            path: None,
            bytes: 0,
            total_bytes: None,
            error: None,
            cleanup_complete: false,
            path_exists: None,
        };
        validate_record(&record)?;
        self.records.push_back(ActiveRecord {
            record,
            physical: true,
            started: now,
        });
        self.trim();
        Ok(token)
    }
    fn entry(&mut self, token: &DownloadToken) -> Result<&mut ActiveRecord, DownloadError> {
        self.records
            .iter_mut()
            .find(|r| r.record.token == *token && token.epoch == self.epoch)
            .ok_or_else(|| {
                error(
                    "download_not_found",
                    "Download owner token is stale or missing",
                )
            })
    }
    pub fn update(&mut self, command: NativeDownload) -> Result<Value, DownloadError> {
        match command {
            NativeDownload::Begin { .. } => {
                return Err(error(
                    "invalid_download_request",
                    "Begin requires validated browser admission",
                ));
            }
            NativeDownload::Drain => {
                return Ok(
                    json!({"cancel": self.records.iter().filter(|r| r.physical && r.record.status == DownloadState::Cancelling).map(|r| &r.record.token).collect::<Vec<_>>()}),
                );
            }
            NativeDownload::Destination { token, filename } => {
                if !valid_filename(&filename) {
                    return Err(error(
                        "invalid_download_filename",
                        "Unsafe download filename",
                    ));
                }
                let entry = self.entry(&token)?;
                if entry.record.status != DownloadState::AwaitingDestination {
                    return Err(error(
                        "download_transition",
                        "Destination is no longer pending",
                    ));
                }
                let mut candidate = entry.record.clone();
                candidate.filename = filename.clone();
                validate_record(&candidate)?;
                entry.record.filename = filename;
                entry.record.status = DownloadState::Downloading;
            }
            NativeDownload::Progress {
                token,
                bytes,
                total_bytes,
            } => {
                let entry = self.entry(&token)?;
                if entry.record.status != DownloadState::Downloading || bytes < entry.record.bytes {
                    return Err(error(
                        "download_transition",
                        "Late or decreasing download progress",
                    ));
                }
                entry.record.bytes = bytes;
                entry.record.total_bytes = total_bytes;
            }
            NativeDownload::Finalizing { token } => {
                let entry = self.entry(&token)?;
                if entry.record.status != DownloadState::Downloading {
                    return Err(error(
                        "download_transition",
                        "Download cannot reserve final publication",
                    ));
                }
                // Final publication reservation wins against later cancellation.
                // Cancel now reports too late rather than deleting a saved file.
                entry.record.status = DownloadState::Finalizing;
            }
            NativeDownload::Saved {
                token,
                path,
                bytes,
                cleanup_complete,
                message,
            } => {
                if message.as_ref().is_some_and(|m| !bounded(m, 1024)) {
                    return Err(error(
                        "invalid_download_error",
                        "Download error exceeds its metadata bound",
                    ));
                }
                if !valid_path(&path) {
                    return Err(error(
                        "invalid_download_path",
                        "Saved path must be an absolute bounded path",
                    ));
                }
                let entry = self.entry(&token)?;
                if entry.record.status != DownloadState::Finalizing {
                    return Err(error(
                        "download_transition",
                        "Download has no final publication reservation",
                    ));
                }
                let mut candidate = entry.record.clone();
                candidate.path = Some(path.clone());
                candidate.error = message.clone();
                validate_record(&candidate)?;
                entry.record.status = DownloadState::Saved;
                entry.record.path = Some(path);
                entry.record.path_exists = None;
                entry.record.bytes = bytes;
                entry.record.error = message;
                entry.record.cleanup_complete = cleanup_complete;
                entry.physical = !cleanup_complete;
                self.terminal(token.download_id);
            }
            NativeDownload::Failed {
                token,
                message,
                cleanup_complete,
            } => {
                if !bounded(&message, 1024) {
                    return Err(error(
                        "invalid_download_error",
                        "Download error exceeds its metadata bound",
                    ));
                }
                let entry = self.entry(&token)?;
                if entry.record.status.terminal() {
                    return Err(error("download_transition", "Download already finished"));
                }
                let mut candidate = entry.record.clone();
                candidate.error = Some(message.clone());
                validate_record(&candidate)?;
                entry.record.status = DownloadState::Failed;
                entry.record.error = Some(message);
                entry.record.cleanup_complete = cleanup_complete;
                entry.physical = !cleanup_complete;
                self.terminal(token.download_id);
            }
            NativeDownload::Cancelled {
                token,
                cleanup_complete,
            } => {
                let entry = self.entry(&token)?;
                if entry.record.status.terminal()
                    || entry.record.status == DownloadState::Finalizing
                {
                    return Err(error(
                        "download_transition",
                        "Cancellation cannot replace final publication",
                    ));
                }
                entry.record.status = DownloadState::Cancelled;
                entry.record.cleanup_complete = cleanup_complete;
                entry.physical = !cleanup_complete;
                self.terminal(token.download_id);
            }
        }
        self.trim();
        Ok(json!({"accepted": true}))
    }
    pub fn cancel(&mut self, surface_uuid: Uuid, id: Uuid) -> Result<(), DownloadError> {
        let entry = self
            .records
            .iter_mut()
            .find(|r| {
                r.record.token.download_id == id && r.record.origin.surface_uuid == surface_uuid
            })
            .ok_or_else(|| {
                error(
                    "download_not_found",
                    "Download does not belong to this surface",
                )
            })?;
        if entry.record.status.terminal() || entry.record.status == DownloadState::Finalizing {
            return Err(error(
                "download_too_late",
                "Download has finished or reserved final publication",
            ));
        }
        entry.record.status = DownloadState::Cancelling;
        Ok(())
    }
    pub fn list(&self, surface_uuid: Uuid, limit: Option<usize>) -> Result<Value, DownloadError> {
        let limit = limit.unwrap_or(25);
        if !(1..=25).contains(&limit) {
            return Err(error(
                "invalid_download_limit",
                "Download list limit must be 1 through 25",
            ));
        }
        let records: Vec<_> = self
            .records
            .iter()
            .rev()
            .filter(|r| r.record.origin.surface_uuid == surface_uuid)
            .take(limit)
            .map(|r| &r.record)
            .collect();
        Ok(
            json!({"downloads": records, "count": records.len(), "limit": limit, "path_exists_source": "not_checked"}),
        )
    }
    pub fn wait(
        &mut self,
        model: &Snapshot,
        surface: SurfaceId,
        path: Option<String>,
        timeout_ms: Option<u64>,
        now: Instant,
    ) -> Result<WaitReceipt, DownloadError> {
        if path.as_ref().is_some_and(|p| !valid_path(p)) {
            return Err(error(
                "invalid_download_path",
                "Observed path must be an absolute bounded path",
            ));
        }
        if self.tickets.len() == MAX_WAITERS {
            let index = self
                .tickets
                .iter()
                .position(|t| t.receipt.state != "pending" && !t.physical)
                .ok_or_else(|| error("download_wait_capacity", "Download wait tickets are full"))?;
            self.tickets.remove(index);
        }
        let timeout = timeout_ms.unwrap_or(DEFAULT_WAIT_MS).clamp(1, MAX_WAIT_MS);
        let receipt = WaitReceipt {
            epoch: self.epoch,
            ticket_id: Uuid::new_v4(),
            surface_uuid: model.object_uuid(TargetKind::Surface, surface.0),
            state: "pending".into(),
            result: None,
            error: None,
        };
        let deadline = now + Duration::from_millis(timeout);
        if let Some(path) = &path {
            if self.paths.is_none() {
                self.paths =
                    Some(PathObservers::start().map_err(|e| error("download_path_worker", e))?);
            }
            self.paths
                .as_ref()
                .expect("started")
                .submit(Observation {
                    id: receipt.ticket_id,
                    path: PathBuf::from(path),
                    deadline,
                })
                .map_err(|e| error("download_path_worker", e))?;
        }
        self.tickets.push_back(Ticket {
            receipt: receipt.clone(),
            surface,
            deadline,
            physical: path.is_some(),
            path,
        });
        self.deliver(now);
        self.status(receipt.epoch, receipt.ticket_id)
    }
    pub fn status(&self, epoch: Uuid, id: Uuid) -> Result<WaitReceipt, DownloadError> {
        self.tickets
            .iter()
            .find(|t| t.receipt.epoch == epoch && t.receipt.ticket_id == id)
            .map(|t| t.receipt.clone())
            .ok_or_else(|| {
                error(
                    "download_wait_not_found",
                    "Download wait ticket expired or belongs to another runtime",
                )
            })
    }
    fn terminal(&mut self, id: Uuid) {
        self.events.push_back(id);
        self.deliver(Instant::now());
    }
    fn deliver(&mut self, now: Instant) {
        for ticket in &mut self.tickets {
            if ticket.receipt.state != "pending" {
                continue;
            }
            if now >= ticket.deadline {
                ticket.receipt.state = "failed".into();
                ticket.receipt.error = Some("timeout".into());
                continue;
            }
            if ticket.path.is_some() {
                continue;
            }
            if let Some(index) = self.events.iter().position(|id| {
                self.records.iter().any(|r| {
                    r.record.token.download_id == *id
                        && r.record.origin.browser.surface_id == ticket.surface
                        && r.record.origin.surface_uuid == ticket.receipt.surface_uuid
                })
            }) {
                let id = self.events.remove(index).expect("position");
                if let Some(entry) = self
                    .records
                    .iter()
                    .find(|r| r.record.token.download_id == id)
                {
                    ticket.receipt.state = "completed".into();
                    ticket.receipt.result = Some(json!({"download": entry.record}));
                }
            }
        }
    }
    pub fn poll(&mut self, model: &Snapshot, now: Instant) -> bool {
        let states: Vec<_> = self.records.iter().map(|r| r.record.status).collect();
        let before = self
            .tickets
            .iter()
            .filter(|t| t.receipt.state == "pending")
            .count();
        for entry in &mut self.records {
            let live = model.identity_namespace == entry.record.origin.browser.identity_namespace
                && model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .any(|s| {
                        s.id == entry.record.origin.browser.surface_id
                            && s.kind == SurfaceKind::Browser
                            && s.browser_profile_id == entry.record.origin.profile_id
                    });
            if (!live || now.saturating_duration_since(entry.started) > Duration::from_secs(900))
                && !entry.record.status.terminal()
                && entry.record.status != DownloadState::Finalizing
            {
                entry.record.status = DownloadState::Cancelling;
            }
        }
        self.deliver(now);
        if let Some(paths) = &self.paths {
            loop {
                let completion = match paths.completion() {
                    Ok(Some(completion)) => completion,
                    Ok(None) => break,
                    Err(message) => {
                        // A disconnected completion channel proves the worker no longer
                        // owns any observation. Do not silently strand physical credits.
                        for ticket in self.tickets.iter_mut().filter(|t| t.physical) {
                            ticket.physical = false;
                            if ticket.receipt.state == "pending" {
                                ticket.receipt.state = "failed".into();
                                ticket.receipt.error = Some(message.clone());
                            }
                        }
                        break;
                    }
                };
                if let Some(ticket) = self
                    .tickets
                    .iter_mut()
                    .find(|t| t.receipt.ticket_id == completion.id)
                {
                    ticket.physical = false;
                    if ticket.receipt.state == "pending" {
                        // Metadata may block beyond the poll call's captured instant.
                        let result = if Instant::now() >= ticket.deadline {
                            Err("timeout")
                        } else {
                            completion.result
                        };
                        match result {
                            Ok(()) => {
                                ticket.receipt.state = "completed".into();
                                ticket.receipt.result =
                                    Some(json!({"path": ticket.path, "downloaded": true}));
                            }
                            Err(message) => {
                                ticket.receipt.state = "failed".into();
                                ticket.receipt.error = Some(message.into());
                            }
                        }
                    }
                }
            }
        }
        states
            != self
                .records
                .iter()
                .map(|r| r.record.status)
                .collect::<Vec<_>>()
            || before
                != self
                    .tickets
                    .iter()
                    .filter(|t| t.receipt.state == "pending")
                    .count()
    }
    pub fn detach(&mut self, host: Uuid) {
        for entry in &mut self.records {
            if entry.record.origin.browser.host_id == host
                && !entry.record.status.terminal()
                && entry.record.status != DownloadState::Finalizing
            {
                entry.record.status = DownloadState::Cancelling;
            }
        }
    }
    fn trim(&mut self) {
        loop {
            let oversized = self.records.len() > MAX_HISTORY
                || serde_json::to_vec(&self.records()).is_ok_and(|v| v.len() > MAX_HISTORY_BYTES);
            let crowded = self.records.iter().find_map(|r| {
                (self
                    .records
                    .iter()
                    .filter(|p| p.record.origin.surface_uuid == r.record.origin.surface_uuid)
                    .count()
                    > MAX_HISTORY_PER_SURFACE)
                    .then_some(r.record.origin.surface_uuid)
            });
            if !oversized && crowded.is_none() {
                break;
            }
            let index = self.records.iter().position(|r| {
                !r.physical && (oversized || crowded == Some(r.record.origin.surface_uuid))
            });
            let Some(index) = index else {
                break;
            };
            if let Some(removed) = self.records.remove(index) {
                self.events
                    .retain(|id| *id != removed.record.token.download_id);
            }
        }
        debug_assert!(
            self.records
                .iter()
                .all(|r| serde_json::to_vec(&r.record).is_ok_and(|v| v.len() <= MAX_RECORD_BYTES))
        );
    }
    pub fn shutdown(&mut self, deadline: Instant) -> Result<(), String> {
        if let Some(paths) = &mut self.paths {
            paths.shutdown(deadline)?;
        }
        Ok(())
    }
}
