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
}
pub(super) struct TransferSlot {
    snapshot: TransferSnapshot,
    job: UploadJob,
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
                slot.job.cancel();
                slot.snapshot.delivery = "cancelled".into();
                Ok(())
            }
            TransferCommand::PasteFiles { surface_id, paths } => {
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
                let slot = self
                    .slots
                    .get(&session_id)
                    .ok_or(DesktopError::TerminalUnavailable)?;
                let session = slot
                    .session
                    .as_ref()
                    .ok_or(DesktopError::TerminalUnavailable)?;
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
                    snapshot: TransferSnapshot {
                        id,
                        surface_id,
                        session_id,
                        upload,
                        delivery: "pending".into(),
                        delivery_error: None,
                    },
                });
                Ok(())
            }
        }
    }

    pub(super) fn poll_transfers(&mut self) -> bool {
        let model = self.model.snapshot();
        let mut changed = false;
        for slot in &mut self.transfers {
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
                slot.job.cancel();
                slot.snapshot.delivery = "cancelled".into();
                slot.snapshot.delivery_error = Some("originating terminal closed".into());
                changed = true;
            }
            if let Ok(upload) = slot.job.snapshot() {
                if upload.state != slot.snapshot.upload.state
                    || upload.bytes_sent != slot.snapshot.upload.bytes_sent
                    || upload.total_bytes != slot.snapshot.upload.total_bytes
                    || upload.remote_paths.len() != slot.snapshot.upload.remote_paths.len()
                    || upload.remote_result_uncertain
                        != slot.snapshot.upload.remote_result_uncertain
                {
                    changed = true;
                }
                slot.snapshot.upload = upload;
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
                        .slots
                        .get(&slot.snapshot.session_id)
                        .and_then(|s| s.session.as_ref())
                        .ok_or(DesktopError::TerminalUnavailable)
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
    pub(super) fn transfers_finished(&self) -> bool {
        self.transfers.iter().all(|s| s.job.is_finished())
    }
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
