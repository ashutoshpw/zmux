//! All journal access is serialized by the private installation lock, then authoritative reread.
use super::domain::{Journal, Result};
use super::*;
use crate::persistence::{self, PersistenceError};
use std::{
    fs::File,
    time::{Duration, Instant},
};
const MAX_BYTES: usize = 512 * 1024;
fn load(directory: &File) -> Result<Journal> {
    let file = match persistence::private_file(
        directory,
        OsStr::new(JOURNAL_NAME),
        MAX_BYTES,
        "agent message journal",
    ) {
        Ok(file) => file,
        Err(PersistenceError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(Journal::default());
        }
        Err(error) => return Err(error.to_string()),
    };
    let bytes = persistence::read_bounded(file, MAX_BYTES, "agent message journal")
        .map_err(|e| e.to_string())?;
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid agent message journal: {e}"))?;
    journal.validate()?;
    Ok(journal)
}
fn lock(directory: &File) -> Result<File> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match persistence::lock_named(directory, LOCK_NAME) {
            Ok(lock) => return Ok(lock),
            Err(PersistenceError::LedgerBusy) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}
pub(super) fn run(
    directory: PathBuf,
    receiver: Receiver<Job>,
    sender: SyncSender<Completion>,
    live: Arc<Mutex<LiveView>>,
) {
    let epoch = Uuid::new_v4();
    let mut deadlines = HashMap::new();
    let mut ephemeral = HashMap::new();
    while let Ok(job) = receiver.recv() {
        let id = match &job.kind {
            JobKind::Request(r) => r.request_id,
            JobKind::Status(id) | JobKind::Reconcile(id) => *id,
        };
        let internal = matches!(job.kind, JobKind::Reconcile(_));
        let mut committing = false;
        let result = (|| -> Result<OperationReceipt> {
            let _guard = job
                .admission
                .as_ref()
                .map(|a| a.check())
                .transpose()
                .map_err(|e| e.to_string())?;
            let directory = persistence::state_directory(&directory).map_err(|e| e.to_string())?;
            let _lock = lock(&directory)?;
            let mut journal = load(&directory)?;
            let (model, closed, cancelled, generation) = {
                let view = live.lock().map_err(|_| "message live view unavailable")?;
                (
                    view.model.clone(),
                    view.closed.clone(),
                    view.cancelled.clone(),
                    view.generation,
                )
            };
            let original =
                persistence::serialize_bounded(&journal, MAX_BYTES, "agent message journal")
                    .map_err(|e| e.to_string())?;
            journal.reconcile(&closed, &deadlines, &cancelled);
            let mut next_deadlines = deadlines.clone();
            let mut next_ephemeral = ephemeral.clone();
            let (value, full) = match &job.kind {
                JobKind::Request(request) => {
                    if !job.full && !request.action.is_read() {
                        return Err("full socket access required".into());
                    }
                    let (result, retained) = journal.apply(
                        request,
                        &job.model,
                        epoch,
                        &mut next_deadlines,
                        &mut next_ephemeral,
                    )?;
                    // A matching, hash-verified retained Send is receipt retrieval. Its
                    // immutable recipient may now be closed; no new admission is occurring.
                    if !retained || !matches!(request.action, MessageAction::Send { .. }) {
                        journal.validate_current(request, &job.model, &model, &result)?;
                    }
                    (Some(result), request.action.requires_full_result())
                }
                JobKind::Reconcile(_) => (None, false),
                JobKind::Status(id) => match journal.status(*id, job.full, &ephemeral)? {
                    Some((result, full)) => (Some(result), full),
                    None => (None, false),
                },
            };
            let reserving = matches!(&job.kind,JobKind::Request(request) if matches!(request.action,MessageAction::Claim{..}));
            let mut handoff_error = None;
            let exposure = if !reserving {
                match value
                    .as_ref()
                    .map(|v| journal.handoff(v, &model, epoch, &next_deadlines))
                    .transpose()
                {
                    Ok(lease) => lease.flatten().map(|lease| {
                        (
                            generation,
                            lease,
                            *next_deadlines
                                .get(&lease)
                                .expect("handoff validated owning lease deadline"),
                        )
                    }),
                    Err(error) => {
                        if let Some(lease) = value
                            .as_ref()
                            .and_then(|v| v.get("lease_id"))
                            .and_then(Value::as_str)
                            .and_then(|id| id.parse().ok())
                        {
                            journal.cancel_lease(lease);
                        }
                        handoff_error = Some(error);
                        None
                    }
                }
            } else {
                None
            };
            journal.validate()?;
            let bytes =
                persistence::serialize_bounded(&journal, MAX_BYTES, "agent message journal")
                    .map_err(|e| e.to_string())?;
            if bytes != original {
                committing = true;
                persistence::atomic_write(&directory, OsStr::new(JOURNAL_NAME), &bytes, true)
                    .map_err(|e| e.to_string())?;
                committing = false;
            }
            journal.retain_runtime_maps(&mut next_deadlines, &mut next_ephemeral);
            deadlines = next_deadlines;
            ephemeral = next_ephemeral;
            {
                let mut view = live.lock().map_err(|_| "message live view unavailable")?;
                view.closed.retain(|id| !closed.contains(id));
                view.cancelled.retain(|id| !cancelled.contains(id));
            }
            if let Some(error) = handoff_error {
                return Ok(OperationReceipt::failed(
                    id,
                    "handoff_rejected",
                    error,
                    false,
                ));
            }
            if exposure.is_some() {
                // A model command may have committed a close/session transition while fsync ran.
                // Revalidate immediately before publishing text; no I/O occurs under the view lock.
                let latest_generation = live
                    .lock()
                    .map_err(|_| "message live view unavailable")?
                    .generation;
                if latest_generation != generation
                    || exposure.is_some_and(|(_, _, deadline)| Instant::now() >= deadline)
                {
                    if let Some((_, lease, _)) = exposure {
                        journal.cancel_lease(lease);
                    }
                    let bytes = persistence::serialize_bounded(
                        &journal,
                        MAX_BYTES,
                        "agent message journal",
                    )
                    .map_err(|e| e.to_string())?;
                    committing = true;
                    persistence::atomic_write(&directory, OsStr::new(JOURNAL_NAME), &bytes, true)
                        .map_err(|e| e.to_string())?;
                    committing = false;
                    return Err("workspace changed or lease expired during claim; exact lease cancelled before context exposure".into());
                }
            }
            Ok(OperationReceipt {
                request_id: id,
                state: if reserving {
                    "awaiting_handoff"
                } else if value.is_some() {
                    "completed"
                } else {
                    "unknown_or_expired"
                }
                .into(),
                result: if reserving { None } else { value },
                error: None,
                requires_full: full,
                exposure,
            })
        })();
        let receipt = result.unwrap_or_else(|e| {
            OperationReceipt::failed(id, "agent_message_rejected", e, committing)
        });
        if sender.send(Completion { receipt, internal }).is_err() {
            break;
        }
    }
}
