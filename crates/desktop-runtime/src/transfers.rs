//! Transfer admission and completion share the workspace executor with all terminal actions.
use super::*;
use zmux_remote_runtime::upload::{UploadJob, UploadSnapshot, UploadState, quote_shell_path};

pub const MAX_UPLOADS: usize = 4;
const MAX_UPLOAD_HISTORY: usize = 8;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransferCommand {
    PasteFiles {
        surface_id: SurfaceId,
        #[serde(default)]
        expected_session_id: Option<SessionId>,
        paths: Vec<PathBuf>,
    },
    Cancel {
        id: u64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct TransferSnapshot {
    pub id: u64,
    pub surface_id: SurfaceId,
    pub session_id: SessionId,
    #[serde(flatten)]
    pub upload: UploadSnapshot,
    pub delivery: String,
    pub delivery_error: Option<String>,
    /// The source may be removed only after the worker has stopped reading it.
    pub source_released: bool,
}
pub(super) struct TransferSlot {
    snapshot: TransferSnapshot,
    job: UploadJob,
    launch_id: u64,
}

impl Worker {
    pub(super) fn transfer(&mut self, action: TransferCommand) -> Result<(), DesktopError> {
        match action {
            TransferCommand::Cancel { id } => {
                let slot = self
                    .transfers
                    .iter_mut()
                    .find(|s| s.snapshot.id == id)
                    .ok_or_else(|| DesktopError::Failure("upload does not exist".into()))?;
                if slot.snapshot.delivery != "pending" {
                    return Err(DesktopError::Failure("upload already finished".into()));
                }
                cancel_transfer_delivery(slot, None);
                Ok(())
            }
            TransferCommand::PasteFiles {
                surface_id,
                expected_session_id,
                paths,
            } => {
                if paths.is_empty() || paths.len() > zmux_remote_runtime::upload::MAX_UPLOAD_FILES {
                    return Err(DesktopError::Failure("choose 1–16 files".into()));
                }
                if paths.iter().any(|p| {
                    !p.is_absolute() || p.to_str().is_none_or(|s| s.chars().any(char::is_control))
                }) {
                    return Err(DesktopError::Failure(
                        "file paths must be absolute UTF-8 without control characters".into(),
                    ));
                }
                let model = self.model.snapshot();
                let surface = model
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .find(|s| s.id == surface_id)
                    .ok_or(DesktopError::TerminalUnavailable)?;
                let session_id = surface.session_id.ok_or(DesktopError::NotTerminal)?;
                if let Some(expected) = expected_session_id
                    && expected != session_id
                {
                    return Err(DesktopError::Failure(format!(
                        "originating terminal session changed (expected {}, found {})",
                        expected.0, session_id.0
                    )));
                }
                let slot = self
                    .slots
                    .get(&session_id)
                    .ok_or(DesktopError::TerminalUnavailable)?;
                let session = self.writable_session(session_id)?;
                if surface.remote.is_none() {
                    let text = paths
                        .iter()
                        .map(|p| quote_shell_path(p.to_str().expect("validated UTF-8")))
                        .collect::<Vec<_>>()
                        .join(" ");
                    return paste_paths(session, &text);
                }
                let connection = Arc::clone(
                    slot.connection
                        .as_ref()
                        .ok_or(DesktopError::TerminalUnavailable)?,
                );
                let launch_id = slot.launch_id;
                if self
                    .transfers
                    .iter()
                    .filter(|t| !t.job.is_finished())
                    .count()
                    >= MAX_UPLOADS
                {
                    return Err(DesktopError::Failure(
                        "four uploads are already active".into(),
                    ));
                }
                // Reserve history before launching: a rejected admission creates no worker.
                if self.transfers.len() == MAX_UPLOAD_HISTORY {
                    if let Some(index) = self
                        .transfers
                        .iter()
                        .position(|t| t.job.is_finished() && t.snapshot.delivery != "pending")
                    {
                        self.transfers.remove(index);
                    } else {
                        return Err(DesktopError::Failure(
                            "upload history is full; wait for pending transfers".into(),
                        ));
                    }
                }
                let id = self.next_transfer_id;
                self.next_transfer_id = id
                    .checked_add(1)
                    .ok_or_else(|| DesktopError::Failure("upload identity exhausted".into()))?;
                let job = UploadJob::start(connection, paths, Duration::from_secs(120))
                    .map_err(|e| DesktopError::Failure(e.to_string()))?;
                let upload = job.snapshot().unwrap_or(UploadSnapshot {
                    state: UploadState::Preparing,
                    bytes_sent: 0,
                    total_bytes: 0,
                    remote_paths: vec![],
                    remote_result_uncertain: false,
                    error: None,
                });
                self.transfers.push(TransferSlot {
                    job,
                    launch_id,
                    snapshot: TransferSnapshot {
                        id,
                        surface_id,
                        session_id,
                        upload,
                        delivery: "pending".into(),
                        delivery_error: None,
                        source_released: false,
                    },
                });
                Ok(())
            }
        }
    }

    pub(super) fn poll_transfers(&mut self) -> bool {
        let model = self.model.snapshot();
        let mut changed = false;
        let mut transfers = std::mem::take(&mut self.transfers);
        for slot in &mut transfers {
            let source_released = slot.job.is_finished();
            if source_released != slot.snapshot.source_released {
                slot.snapshot.source_released = source_released;
                changed = true;
            }
            let origin_live = model
                .workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .flat_map(|p| &p.surfaces)
                .any(|s| {
                    s.id == slot.snapshot.surface_id
                        && s.session_id == Some(slot.snapshot.session_id)
                });
            if !origin_live && slot.snapshot.delivery == "pending" {
                cancel_transfer_delivery(slot, Some("originating terminal closed"));
                changed = true;
            }
            if let Ok(upload) = slot.job.snapshot() {
                changed |= publish_upload_snapshot(&mut slot.snapshot, upload);
            }
            if slot.snapshot.delivery != "pending" {
                continue;
            }
            match slot.snapshot.upload.state {
                UploadState::Completed => {
                    let text = slot
                        .snapshot
                        .upload
                        .remote_paths
                        .iter()
                        .map(|s| quote_shell_path(s))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let result = self
                        .writable_session(slot.snapshot.session_id)
                        .and_then(|session| paste_paths(session, &text));
                    match result {
                        Ok(()) => slot.snapshot.delivery = "inserted".into(),
                        Err(DesktopError::QueueFull | DesktopError::Busy) => continue,
                        Err(error) => {
                            slot.snapshot.delivery = "failed".into();
                            slot.snapshot.delivery_error = Some(error.to_string());
                        }
                    }
                    changed = true;
                }
                UploadState::Cancelled | UploadState::Failed => {
                    slot.snapshot.delivery = "failed".into();
                    changed = true;
                }
                _ => {}
            }
        }
        self.transfers = transfers;
        changed
    }
    pub(super) fn transfer_snapshots(&self) -> Vec<TransferSnapshot> {
        self.transfers.iter().map(|s| s.snapshot.clone()).collect()
    }
    pub(super) fn cancel_transfers(&self) {
        for slot in &self.transfers {
            slot.job.cancel();
        }
    }
    pub(super) fn cancel_transfers_for_origin(&mut self, session_id: SessionId, launch_id: u64) {
        for slot in &mut self.transfers {
            if transfer_matches_origin(&slot.snapshot, slot.launch_id, session_id, launch_id)
                && slot.snapshot.delivery == "pending"
            {
                cancel_transfer_delivery(slot, Some("originating terminal closed"));
            }
        }
    }
    pub(super) fn transfers_finished(&self) -> bool {
        self.transfers.iter().all(|s| s.job.is_finished())
    }
}

fn publish_upload_snapshot(snapshot: &mut TransferSnapshot, upload: UploadSnapshot) -> bool {
    let changed = upload.state != snapshot.upload.state
        || upload.bytes_sent != snapshot.upload.bytes_sent
        || upload.total_bytes != snapshot.upload.total_bytes
        || upload.remote_paths.len() != snapshot.upload.remote_paths.len()
        || upload.remote_result_uncertain != snapshot.upload.remote_result_uncertain
        || upload.error != snapshot.upload.error;
    snapshot.upload = upload;
    changed
}

fn cancel_transfer_delivery(slot: &mut TransferSlot, delivery_error: Option<&str>) {
    if slot.snapshot.delivery != "pending" {
        return;
    }
    let should_signal_worker = match slot.job.snapshot() {
        Ok(upload) => cancel_transfer_snapshot(&mut slot.snapshot, upload, delivery_error),
        Err(_) => {
            mark_delivery_cancelled(&mut slot.snapshot, delivery_error);
            // The producer snapshot is busy or unavailable. Signal cancellation
            // without treating a cached state as proof that work is still active.
            true
        }
    };
    if should_signal_worker {
        slot.job.cancel();
    }
}

fn cancel_transfer_snapshot(
    snapshot: &mut TransferSnapshot,
    upload: UploadSnapshot,
    delivery_error: Option<&str>,
) -> bool {
    let should_signal_worker = matches!(
        upload.state,
        UploadState::Preparing | UploadState::Uploading
    );
    publish_upload_snapshot(snapshot, upload);
    mark_delivery_cancelled(snapshot, delivery_error);
    should_signal_worker
}

fn mark_delivery_cancelled(snapshot: &mut TransferSnapshot, delivery_error: Option<&str>) {
    snapshot.delivery = "cancelled".into();
    snapshot.delivery_error = delivery_error.map(str::to_owned);
}

fn transfer_matches_origin(
    snapshot: &TransferSnapshot,
    transfer_launch_id: u64,
    session_id: SessionId,
    launch_id: u64,
) -> bool {
    snapshot.session_id == session_id && transfer_launch_id == launch_id
}

fn paste_paths(session: &Session, text: &str) -> Result<(), DesktopError> {
    let snapshot = session
        .snapshot()
        .map_err(|e| DesktopError::Failure(e.to_string()))?;
    let text = if snapshot.viewport.bracketed_paste {
        format!("\u{1b}[200~{text}\u{1b}[201~")
    } else {
        text.into()
    };
    if text.len() > INPUT_CHUNK_BYTES {
        return Err(DesktopError::Failure(
            "uploaded paths exceed the terminal insertion limit; copy the returned paths manually"
                .into(),
        ));
    }
    session.send_input(text.as_bytes()).map_err(|e| match e {
        SessionError::QueueFull => DesktopError::QueueFull,
        SessionError::Busy => DesktopError::Busy,
        _ => DesktopError::Failure(e.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_transport_failure_does_not_replace_close_cancellation() {
        let mut snapshot = transfer_snapshot(UploadState::Uploading);
        let upload_before_cancellation = snapshot.upload.clone();
        assert!(cancel_transfer_snapshot(
            &mut snapshot,
            upload_before_cancellation,
            Some("originating terminal closed"),
        ));
        assert_eq!(snapshot.upload.state, UploadState::Uploading);
        assert_eq!(snapshot.delivery, "cancelled");

        let transport_error = "write upload input: Broken pipe (os error 32); remote result is uncertain, verify remote temporary storage";

        assert!(publish_upload_snapshot(
            &mut snapshot,
            UploadSnapshot {
                state: UploadState::Cancelled,
                bytes_sent: 16_384,
                total_bytes: 8_388_608,
                remote_paths: Vec::new(),
                remote_result_uncertain: true,
                error: Some(transport_error.into()),
            },
        ));

        assert_eq!(snapshot.upload.state, UploadState::Cancelled);
        assert_eq!(snapshot.delivery, "cancelled");
        assert_eq!(
            snapshot.delivery_error.as_deref(),
            Some("originating terminal closed")
        );
        assert_eq!(snapshot.upload.bytes_sent, 16_384);
        assert_eq!(snapshot.upload.total_bytes, 8_388_608);
        assert!(snapshot.upload.remote_result_uncertain);
        assert!(snapshot.upload.remote_paths.is_empty());
        assert_eq!(snapshot.upload.error.as_deref(), Some(transport_error));
    }

    #[test]
    fn closing_origin_preserves_completed_upload_outcome() {
        assert_terminal_upload_outcome_is_preserved(UploadState::Completed);
    }

    #[test]
    fn closing_origin_preserves_failed_upload_outcome() {
        assert_terminal_upload_outcome_is_preserved(UploadState::Failed);
    }

    #[test]
    fn retirement_matches_the_exact_session_launch_generation() {
        let snapshot = transfer_snapshot(UploadState::Uploading);
        assert!(transfer_matches_origin(&snapshot, 42, SessionId(14), 42));
        assert!(!transfer_matches_origin(&snapshot, 41, SessionId(14), 42));
        assert!(!transfer_matches_origin(&snapshot, 42, SessionId(15), 42));
    }

    fn transfer_snapshot(state: UploadState) -> TransferSnapshot {
        TransferSnapshot {
            id: 12,
            surface_id: SurfaceId(13),
            session_id: SessionId(14),
            upload: UploadSnapshot {
                state,
                bytes_sent: 16_384,
                total_bytes: 8_388_608,
                remote_paths: Vec::new(),
                remote_result_uncertain: false,
                error: None,
            },
            delivery: "pending".into(),
            delivery_error: None,
            source_released: false,
        }
    }

    fn assert_terminal_upload_outcome_is_preserved(state: UploadState) {
        let mut terminal = transfer_snapshot(UploadState::Uploading);
        let mut worker_snapshot = terminal.upload.clone();
        worker_snapshot.state = state;
        worker_snapshot.remote_paths = vec!["/tmp/already-acknowledged".into()];
        worker_snapshot.remote_result_uncertain = state == UploadState::Failed;
        worker_snapshot.error = (state == UploadState::Failed).then(|| "earlier failure".into());

        assert!(!cancel_transfer_snapshot(
            &mut terminal,
            worker_snapshot,
            Some("originating terminal closed"),
        ));
        assert_eq!(
            terminal.upload.state, state,
            "closing after a terminal worker result must preserve that result"
        );
        assert_eq!(terminal.upload.remote_paths, ["/tmp/already-acknowledged"]);
        assert_eq!(
            terminal.upload.remote_result_uncertain,
            state == UploadState::Failed
        );
        assert_eq!(
            terminal.upload.error.as_deref(),
            (state == UploadState::Failed).then_some("earlier failure")
        );
        assert_eq!(terminal.delivery, "cancelled");
    }
}
