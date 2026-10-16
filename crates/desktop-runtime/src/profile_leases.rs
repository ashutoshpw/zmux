//! Native-store lifetime locks, independent of catalog/runtime shutdown.
//!
//! The native bridge must retain one service for the process lifetime and retain
//! each shared token until every store, host and download/conversion reference is
//! physically retired. This primitive does not admit clear/delete operations.
use crate::persistence::{check_file, profile_exists, state_directory};
use rustix::fs::{self, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    path::PathBuf,
    sync::mpsc::{self, SyncSender, TrySendError},
    thread,
    time::Duration,
};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_LEASES: usize = 64;
pub const MAX_PENDING_REQUESTS: usize = 8;
const ACKNOWLEDGEMENT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMode {
    Shared,
    Exclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseOwner {
    pub epoch: Uuid,
    pub store: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseToken {
    pub service_epoch: Uuid,
    pub id: u64,
    pub owner: LeaseOwner,
    pub profile: Uuid,
    pub mode: LeaseMode,
}

#[derive(Debug, Error)]
pub enum LeaseError {
    #[error("browser profile lease queue is full; request was not accepted")]
    QueueFull,
    #[error("browser profile lease worker is closed; request was not accepted")]
    Closed,
    #[error(
        "browser profile lease acknowledgement is unknown; query the original owner, never replay acquisition"
    )]
    OutcomeUnknown,
    #[error("browser profile lease owner and profile UUIDs must be nonnil")]
    InvalidIdentity,
    #[error("browser profile already has a lease for this owner")]
    AlreadyOwned,
    #[error("browser profile lease capacity reached")]
    Capacity,
    #[error("browser profile lease token space exhausted")]
    Exhausted,
    #[error("unknown browser profile")]
    UnknownProfile,
    #[error("browser profile is busy in another store owner")]
    Busy,
    #[error("browser profile lease file is unsafe or its anchored inode changed")]
    UnsafeLock,
    #[error("browser profile lease token does not match its physical owner")]
    StaleToken,
    #[error("browser profile lease I/O: {0}")]
    Io(String),
}

type Reply = SyncSender<Result<Option<LeaseToken>, LeaseError>>;
enum Request {
    Acquire {
        directory: PathBuf,
        owner: LeaseOwner,
        profile: Uuid,
        mode: LeaseMode,
        reply: Reply,
    },
    Status {
        owner: LeaseOwner,
        reply: Reply,
    },
    Release {
        token: LeaseToken,
        reply: Reply,
    },
}

/// Startup validates the fixed root with filesystem I/O. Startup and requests
/// must run off the native UI thread and core executor. Request acknowledgements
/// are bounded; a timeout does not drop a physical lock.
pub struct LeaseService {
    sender: SyncSender<Request>,
}
impl LeaseService {
    pub fn start(lease_directory: PathBuf) -> Result<Self, LeaseError> {
        // Resolve and retain one namespace root for the service lifetime. Catalog
        // locations may differ; they must never choose independent lock roots.
        let directory =
            state_directory(&lease_directory).map_err(|error| LeaseError::Io(error.to_string()))?;
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_REQUESTS);
        thread::Builder::new()
            .name("zmux-profile-leases".into())
            .spawn(move || {
                let mut registry = Registry {
                    directory,
                    epoch: Uuid::new_v4(),
                    next: 1,
                    leases: HashMap::new(),
                };
                while let Ok(request) = receiver.recv() {
                    let (result, reply) = match request {
                        Request::Acquire {
                            directory,
                            owner,
                            profile,
                            mode,
                            reply,
                        } => (
                            registry.acquire(directory, owner, profile, mode).map(Some),
                            reply,
                        ),
                        Request::Status { owner, reply } => (
                            Ok(registry
                                .leases
                                .values()
                                .find(|lease| lease.token.owner == owner)
                                .map(|lease| lease.token)),
                            reply,
                        ),
                        Request::Release { token, reply } => {
                            (registry.release(token).map(|()| None), reply)
                        }
                    };
                    // A lost acknowledgement retains the acquired lease in the
                    // bounded registry. Status recovers by its original owner.
                    let _ = reply.try_send(result);
                }
            })
            .map_err(|error| LeaseError::Io(error.to_string()))?;
        Ok(Self { sender })
    }
    pub fn acquire(
        &self,
        directory: PathBuf,
        owner: LeaseOwner,
        profile: Uuid,
        mode: LeaseMode,
    ) -> Result<LeaseToken, LeaseError> {
        self.call(|reply| Request::Acquire {
            directory,
            owner,
            profile,
            mode,
            reply,
        })?
        .ok_or(LeaseError::OutcomeUnknown)
    }
    pub fn status(&self, owner: LeaseOwner) -> Result<Option<LeaseToken>, LeaseError> {
        self.call(|reply| Request::Status { owner, reply })
    }
    pub fn release(&self, token: LeaseToken) -> Result<(), LeaseError> {
        self.call(|reply| Request::Release { token, reply })
            .map(|_| ())
    }
    fn call(
        &self,
        request: impl FnOnce(Reply) -> Request,
    ) -> Result<Option<LeaseToken>, LeaseError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.sender
            .try_send(request(sender))
            .map_err(|error| match error {
                TrySendError::Full(_) => LeaseError::QueueFull,
                TrySendError::Disconnected(_) => LeaseError::Closed,
            })?;
        receiver
            .recv_timeout(ACKNOWLEDGEMENT)
            .map_err(|_| LeaseError::OutcomeUnknown)?
    }
}

struct Registry {
    directory: File,
    epoch: Uuid,
    next: u64,
    leases: HashMap<u64, PhysicalLease>,
}
struct PhysicalLease {
    token: LeaseToken,
    _lock: File,
}
impl Registry {
    fn acquire(
        &mut self,
        directory: PathBuf,
        owner: LeaseOwner,
        profile: Uuid,
        mode: LeaseMode,
    ) -> Result<LeaseToken, LeaseError> {
        if owner.epoch.is_nil() || owner.store.is_nil() || profile.is_nil() {
            return Err(LeaseError::InvalidIdentity);
        }
        if self.leases.values().any(|lease| lease.token.owner == owner) {
            return Err(LeaseError::AlreadyOwned);
        }
        if self.leases.len() >= MAX_LEASES {
            return Err(LeaseError::Capacity);
        }
        let next = self.next.checked_add(1).ok_or(LeaseError::Exhausted)?;
        let directory =
            state_directory(&directory).map_err(|error| LeaseError::Io(error.to_string()))?;
        if !profile_exists(&directory, profile)
            .map_err(|error| LeaseError::Io(error.to_string()))?
        {
            return Err(LeaseError::UnknownProfile);
        }
        let name = lock_name(profile);
        let lock = File::from(
            fs::openat(
                &self.directory,
                name.as_str(),
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|error| LeaseError::Io(error.to_string()))?,
        );
        inspect(&self.directory, &lock, &name)?;
        match mode {
            LeaseMode::Shared => lock.try_lock_shared(),
            LeaseMode::Exclusive => lock.try_lock(),
        }
        .map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => LeaseError::Busy,
            std::fs::TryLockError::Error(error) => LeaseError::Io(error.to_string()),
        })?;
        inspect(&self.directory, &lock, &name)?;
        // Exclusive deletion cannot race this second identity check once all
        // native store construction and destructive owners use these leases.
        if !profile_exists(&directory, profile)
            .map_err(|error| LeaseError::Io(error.to_string()))?
        {
            return Err(LeaseError::UnknownProfile);
        }
        let token = LeaseToken {
            service_epoch: self.epoch,
            id: self.next,
            owner,
            profile,
            mode,
        };
        self.next = next;
        self.leases
            .insert(token.id, PhysicalLease { token, _lock: lock });
        Ok(token)
    }
    fn release(&mut self, token: LeaseToken) -> Result<(), LeaseError> {
        if self
            .leases
            .get(&token.id)
            .is_none_or(|lease| lease.token != token)
        {
            return Err(LeaseError::StaleToken);
        }
        // File close/unlock happens here on the lease worker, never UI/executor.
        self.leases.remove(&token.id);
        Ok(())
    }
}

fn inspect(directory: &File, lock: &File, name: &str) -> Result<(), LeaseError> {
    check_file(lock, 0, "browser profile lease").map_err(|_| LeaseError::UnsafeLock)?;
    let descriptor = fs::fstat(lock).map_err(|error| LeaseError::Io(error.to_string()))?;
    let entry = fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| LeaseError::Io(error.to_string()))?;
    if descriptor.st_nlink != 1
        || entry.st_nlink != 1
        || descriptor.st_dev != entry.st_dev
        || descriptor.st_ino != entry.st_ino
        || descriptor.st_mode != entry.st_mode
        || descriptor.st_uid != entry.st_uid
        || descriptor.st_size != 0
        || entry.st_size != 0
    {
        return Err(LeaseError::UnsafeLock);
    }
    Ok(())
}
fn lock_name(profile: Uuid) -> String {
    format!("browser-profile-{profile}.lock")
}
pub(crate) fn is_reserved_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower
        .strip_prefix("browser-profile-")
        .and_then(|value| value.strip_suffix(".lock"))
        .is_some_and(|value| Uuid::parse_str(value).is_ok())
}
