//! Small, private, crash-safe scheduler store.

use super::{
    MAX_PENDING_JOBS, MAX_RETAINED_HISTORY, RECEIPT_RETENTION_SECONDS, STORE_LOCK_TIMEOUT_MILLIS,
    store_state::{StoreState, StoredReceipt},
};
use nix::unistd::geteuid;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

pub(super) const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_RECEIPTS: usize = 1_024;

#[derive(Debug, Error)]
pub(super) enum StoreError {
    #[error("scheduler store is busy")]
    Busy,
    #[error("scheduler store is unsafe: {0}")]
    Unsafe(&'static str),
    #[error("scheduler store is corrupt: {0}")]
    Corrupt(String),
    #[error("scheduler store commit may have completed: {0}")]
    CommitUncertain(String),
    #[error("scheduler store I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug)]
pub(super) struct Store {
    root: PathBuf,
}

impl Store {
    pub(super) fn open(root: PathBuf) -> Result<Self, StoreError> {
        if !root.is_absolute() {
            return Err(StoreError::Unsafe("root must be an absolute path"));
        }
        let root_was_missing = match fs::symlink_metadata(&root) {
            Ok(_) => false,
            Err(error) if error.kind() == io::ErrorKind::NotFound => true,
            Err(error) => return Err(StoreError::Io(error)),
        };
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        if root_was_missing {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(StoreError::Unsafe(
                "root must be a real directory owned by this user with mode 0700",
            ));
        }
        let store = Self { root };
        store.with_locked_state(|state| {
            let _ = state;
            Ok(((), true))
        })?;
        Ok(store)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn read<T>(
        &self,
        operation: impl FnOnce(&StoreState) -> T,
    ) -> Result<T, StoreError> {
        let (_lock, state) = self.lock_state()?;
        Ok(operation(&state))
    }

    /// Run one serialized durable transaction. The closure's `changed` flag means
    /// the state is replaced atomically and synced before its result is returned.
    pub(super) fn transact<T>(
        &self,
        operation: impl FnOnce(&mut StoreState) -> Result<(T, bool), StoreError>,
    ) -> Result<T, StoreError> {
        let (_lock, mut state) = self.lock_state()?;
        let (result, changed) = operation(&mut state)?;
        if changed {
            trim_state(&mut state);
            validate_state(&state)?;
            self.save(&state)?;
        }
        Ok(result)
    }

    pub(super) fn receipt(state: &StoreState, request_id: Uuid) -> Option<&StoredReceipt> {
        let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
        state
            .receipts
            .iter()
            .find(|item| item.receipt.request_id == request_id && item.expires_at_unix_nanos > now)
    }

    pub(super) fn remember_receipt(state: &mut StoreState, receipt: StoredReceipt) {
        state.receipts.push(receipt);
        prune_expired(state);
    }

    pub(super) fn prune_expired_receipts(state: &mut StoreState) {
        prune_expired(state);
    }

    /// Count pending-job cancellation reservations alongside retained receipts.
    /// `additional_capacity_units` covers new receipts and, for creates, the new
    /// pending cancellation reservation created by the same mutation.
    pub(super) fn receipt_capacity_available(
        state: &StoreState,
        additional_capacity_units: usize,
    ) -> bool {
        let pending_cancellations = state
            .jobs
            .iter()
            .filter(|job| job.state == super::JobState::Pending)
            .count();
        state
            .receipts
            .len()
            .saturating_add(pending_cancellations)
            .saturating_add(additional_capacity_units)
            <= MAX_RECEIPTS
    }

    fn with_locked_state<T>(
        &self,
        operation: impl FnOnce(&mut StoreState) -> Result<(T, bool), StoreError>,
    ) -> Result<T, StoreError> {
        self.transact(operation)
    }

    fn lock_state(&self) -> Result<(File, StoreState), StoreError> {
        let lock_path = self.root.join("scheduler.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(lock_path)?;
        validate_file(&lock, "lock")?;
        let until = Instant::now() + Duration::from_millis(STORE_LOCK_TIMEOUT_MILLIS);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) if Instant::now() < until => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(fs::TryLockError::WouldBlock) => return Err(StoreError::Busy),
                Err(fs::TryLockError::Error(error)) => return Err(StoreError::Io(error)),
            }
        }
        let state = self.load_or_initialize()?;
        Ok((lock, state))
    }

    fn load_or_initialize(&self) -> Result<StoreState, StoreError> {
        let path = self.root.join("state.json");
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let state = StoreState::new();
                validate_state(&state)?;
                self.save(&state)?;
                return Ok(state);
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        validate_file(&file, "state")?;
        if file.metadata()?.len() > MAX_STORE_BYTES as u64 {
            return Err(StoreError::Unsafe("state exceeds its 16 MiB bound"));
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_STORE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Unsafe("state exceeds its 16 MiB bound"));
        }
        let state: StoreState = serde_json::from_slice(&bytes)
            .map_err(|error| StoreError::Corrupt(error.to_string()))?;
        validate_state(&state)?;
        Ok(state)
    }

    fn save(&self, state: &StoreState) -> Result<(), StoreError> {
        let bytes =
            serde_json::to_vec(state).map_err(|error| StoreError::Corrupt(error.to_string()))?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Unsafe(
                "serialized state exceeds its 16 MiB bound",
            ));
        }
        let temporary = self.root.join(format!("state-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, self.root.join("state.json"))?;
            Ok::<_, io::Error>(())
        })();
        if result.is_err()
            && let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("zmux scheduler temporary cleanup failed: {error}");
        }
        result.map_err(StoreError::Io)?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| StoreError::CommitUncertain(error.to_string()))
    }
}

fn validate_file(file: &File, kind: &'static str) -> Result<(), StoreError> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(StoreError::Unsafe(match kind {
            "lock" => "lock must be a private, singly-linked file owned by this user",
            _ => "state must be a private, singly-linked file owned by this user",
        }));
    }
    Ok(())
}

fn validate_state(state: &StoreState) -> Result<(), StoreError> {
    if state.version != 1 || state.helper_instance_id.is_nil() {
        return Err(StoreError::Unsafe(
            "unsupported state version or helper identity",
        ));
    }
    if state.jobs.len() > MAX_PENDING_JOBS + MAX_RETAINED_HISTORY
        || state
            .jobs
            .iter()
            .filter(|job| {
                matches!(
                    job.state,
                    super::JobState::Pending | super::JobState::Dispatching
                )
            })
            .count()
            > MAX_PENDING_JOBS
    {
        return Err(StoreError::Unsafe("job count exceeds its configured bound"));
    }
    if state.receipts.len() > MAX_RECEIPTS {
        return Err(StoreError::Unsafe(
            "receipt count exceeds its configured bound",
        ));
    }
    let pending_cancellations = state
        .jobs
        .iter()
        .filter(|job| job.state == super::JobState::Pending)
        .count();
    if state.receipts.len().saturating_add(pending_cancellations) > MAX_RECEIPTS {
        return Err(StoreError::Unsafe(
            "receipt count exceeds pending-job cancellation reservations",
        ));
    }
    let mut job_ids = std::collections::HashSet::new();
    for job in &state.jobs {
        if job.id.is_nil() || job.version == 0 || !job_ids.insert(job.id) {
            return Err(StoreError::Unsafe("duplicate or invalid job identity"));
        }
        job.target
            .validate()
            .map_err(|_| StoreError::Unsafe("invalid target identity in store"))?;
        super::validate_message(&job.message)
            .map_err(|_| StoreError::Unsafe("invalid message in store"))?;
    }
    let mut request_ids = std::collections::HashSet::new();
    for entry in &state.receipts {
        if entry.receipt.request_id.is_nil()
            || !request_ids.insert(entry.receipt.request_id)
            || entry.fingerprint.len() != 64
            || !entry.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
            || entry.response.request_id != Some(entry.receipt.request_id)
        {
            return Err(StoreError::Unsafe("invalid request receipt in store"));
        }
    }
    let _ = OffsetDateTime::now_utc();
    Ok(())
}

fn trim_state(state: &mut StoreState) {
    let mut terminal: Vec<_> = state
        .jobs
        .iter()
        .filter(|job| {
            !matches!(
                job.state,
                super::JobState::Pending | super::JobState::Dispatching
            )
        })
        .map(|job| (job.updated_at_utc.clone(), job.id))
        .collect();
    terminal.sort_by(|left, right| left.0.cmp(&right.0));
    let excess = terminal.len().saturating_sub(MAX_RETAINED_HISTORY);
    for (timestamp, id) in terminal.into_iter().take(excess) {
        state.jobs.retain(|job| job.id != id);
        state.pruned_before_utc = Some(timestamp);
    }
    prune_expired(state);
}

fn prune_expired(state: &mut StoreState) {
    let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
    state
        .receipts
        .retain(|entry| entry.expires_at_unix_nanos > now);
}

pub(super) fn receipt_expiry(now: OffsetDateTime) -> i128 {
    now.saturating_add(time::Duration::seconds(RECEIPT_RETENTION_SECONDS as i64))
        .unix_timestamp_nanos()
}
