//! Durable per-UUID operation phase. No website data is stored here.
use super::{LeaseError, LeaseMode, inspect_lock, open_lock};
use crate::persistence::{atomic_write, private_file, read_bounded};
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, fs::File, io::Write, os::unix::fs::MetadataExt, path::PathBuf};
use uuid::Uuid;

pub const MAX_INTENT_BYTES: usize = 16 * 1024;
pub const QUARANTINE_SENTINEL: &[u8] = b"ZMUX_PROFILE_QUARANTINE_V1\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    Clear,
    Delete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationPhase {
    Reserved,
    ClearDispatched,
    ClearCompleted,
    RemoveDispatched,
    RemoveCompleted,
    CatalogCommitting,
    Completed,
    FailedBeforeClear,
    FailedPartial,
}
impl MutationPhase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::FailedBeforeClear | Self::FailedPartial
        )
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::ClearDispatched => "clear_dispatched",
            Self::ClearCompleted => "clear_completed",
            Self::RemoveDispatched => "remove_dispatched",
            Self::RemoveCompleted => "remove_completed",
            Self::CatalogCommitting => "catalog_committing",
            Self::Completed => "completed",
            Self::FailedBeforeClear => "failed_before_clear",
            Self::FailedPartial => "failed_partial",
        }
    }
    pub(super) fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Reserved,
                Self::ClearDispatched | Self::FailedBeforeClear
            ) | (Self::ClearDispatched, Self::ClearCompleted)
                | (
                    Self::ClearCompleted,
                    Self::RemoveDispatched | Self::FailedPartial
                )
                | (
                    Self::RemoveDispatched,
                    Self::RemoveCompleted | Self::FailedPartial
                )
                | (Self::RemoveCompleted, Self::CatalogCommitting)
                | (Self::CatalogCommitting, Self::Completed)
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentRecord {
    pub version: u32,
    pub intent: Uuid,
    pub profile: Uuid,
    pub service_epoch: Uuid,
    pub controller: Uuid,
    pub catalog: PathBuf,
    pub catalog_device: u64,
    pub catalog_inode: u64,
    pub kind: MutationKind,
    pub phase: MutationPhase,
    pub error: Option<String>,
}
impl IntentRecord {
    pub(super) fn validate(&self, profile: Uuid) -> Result<(), LeaseError> {
        if self.version != 1
            || self.profile != profile
            || self.intent.is_nil()
            || self.profile.is_nil()
            || self.service_epoch.is_nil()
            || self.controller.is_nil()
            || !self.catalog.is_absolute()
            || self
                .catalog
                .components()
                .any(|v| matches!(v, std::path::Component::ParentDir))
            || self.error.as_ref().is_some_and(|e| e.len() > 1024)
        {
            return Err(LeaseError::UnsafeIntent);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct IntentStatus {
    pub record: IntentRecord,
    pub original_owner_available: bool,
    pub outcome_unknown: bool,
    pub recovery_required: bool,
    pub retry_native: bool,
    pub physical_cleanup_pending: bool,
}

pub(super) fn name(profile: Uuid) -> String {
    format!("browser-profile-{profile}.intent.json")
}
pub(super) fn admission_name(profile: Uuid) -> String {
    format!("browser-profile-{profile}.admission.lock")
}
pub(super) fn read(directory: &File, profile: Uuid) -> Result<Option<IntentRecord>, LeaseError> {
    let file = match private_file(
        directory,
        OsStr::new(&name(profile)),
        MAX_INTENT_BYTES,
        "browser profile intent",
    ) {
        Ok(file) => file,
        Err(crate::persistence::PersistenceError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(None);
        }
        Err(_) => return Err(LeaseError::UnsafeIntent),
    };
    let data = read_bounded(file, MAX_INTENT_BYTES, "browser profile intent")
        .map_err(|_| LeaseError::UnsafeIntent)?;
    let record: IntentRecord =
        serde_json::from_slice(&data).map_err(|_| LeaseError::UnsafeIntent)?;
    record.validate(profile)?;
    Ok(Some(record))
}
pub(super) fn write(directory: &File, record: &IntentRecord) -> Result<(), LeaseError> {
    record.validate(record.profile)?;
    let data = serde_json::to_vec(record).map_err(|e| LeaseError::Io(e.to_string()))?;
    if data.len() > MAX_INTENT_BYTES {
        return Err(LeaseError::Capacity);
    }
    atomic_write(directory, OsStr::new(&name(record.profile)), &data, true)
        .map_err(|e| LeaseError::Io(e.to_string()))
}
pub(super) fn ensure_unquarantined(directory: &File, profile: Uuid) -> Result<(), LeaseError> {
    if let Some(record) = read(directory, profile)?
        && !record.phase.terminal()
    {
        return Err(LeaseError::Quarantined {
            intent: record.intent,
            phase: record.phase.code(),
        });
    }
    Ok(())
}
pub(super) fn set_sentinel(lock: &mut File) -> Result<(), LeaseError> {
    lock.write_all(QUARANTINE_SENTINEL)
        .and_then(|()| lock.sync_all())
        .map_err(|e| LeaseError::Io(e.to_string()))
}
pub(super) fn clear_sentinel(lock: &File) -> Result<(), LeaseError> {
    lock.set_len(0)
        .and_then(|()| lock.sync_all())
        .map_err(|e| LeaseError::Io(e.to_string()))
}
/// Only known durable terminal state permits this filesystem-only completion.
/// There is no native replay and no expiry-based release.
pub(super) fn recover_terminal(directory: &File, profile: Uuid) -> Result<(), LeaseError> {
    let Some(record) = read(directory, profile)? else {
        return Ok(());
    };
    if !record.phase.terminal() {
        return ensure_unquarantined(directory, profile);
    }
    let _admission = open_lock(
        directory,
        &admission_name(profile),
        LeaseMode::Exclusive,
        false,
    )?;
    let Some(current) = read(directory, profile)? else {
        return Err(LeaseError::UnsafeIntent);
    };
    if current != record || !current.phase.terminal() {
        return Err(LeaseError::UnsafeIntent);
    }
    let lock = super::open_file(directory, &super::lock_name(profile))?;
    inspect_lock(directory, &lock, &super::lock_name(profile), true)?;
    if lock
        .metadata()
        .map_err(|e| LeaseError::Io(e.to_string()))?
        .len()
        == 0
    {
        return Ok(());
    }
    lock.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => LeaseError::Busy,
        std::fs::TryLockError::Error(error) => LeaseError::Io(error.to_string()),
    })?;
    inspect_lock(directory, &lock, &super::lock_name(profile), true)?;
    clear_sentinel(&lock)
}
pub(super) fn catalog_identity(directory: &File) -> Result<(u64, u64), LeaseError> {
    let metadata = directory
        .metadata()
        .map_err(|e| LeaseError::Io(e.to_string()))?;
    Ok((metadata.dev(), metadata.ino()))
}
