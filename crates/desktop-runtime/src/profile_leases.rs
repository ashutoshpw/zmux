//! Native-store lifetime locks, independent of catalog/runtime shutdown.
//!
//! The native bridge must retain one service for the process lifetime and retain
//! each shared token until every store, host and download/conversion reference is
//! physically retired. This primitive does not admit clear/delete operations.
pub mod journal;
mod mutations;
pub use mutations::{ConstructionPermit, MutationRequest, MutationToken, NativeMutationOutcome};

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
pub const MAX_CONSTRUCTION_PERMITS: usize = 32;
pub const MAX_MUTATIONS: usize = 8;
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
    #[error(
        "browser profile intent is corrupt, unsafe or inconsistent; profile remains quarantined"
    )]
    UnsafeIntent,
    #[error(
        "browser profile operation {intent} is unresolved at {phase}; query the original operation; do not retry or reopen"
    )]
    Quarantined { intent: Uuid, phase: &'static str },
    #[error("browser profile lease I/O: {0}")]
    Io(String),
}

type Reply<T = Option<LeaseToken>> = SyncSender<Result<T, LeaseError>>;
enum Request {
    RecoverTerminal {
        profile: Uuid,
        intent: Uuid,
        reply: Reply<journal::IntentStatus>,
    },
    ConstructionStatus {
        construction: Uuid,
        reply: Reply<Option<ConstructionPermit>>,
    },
    ReleaseUnconstructed {
        token: LeaseToken,
        reply: Reply<()>,
    },
    Reuse {
        directory: PathBuf,
        token: LeaseToken,
        construction: Uuid,
        reply: Reply<ConstructionPermit>,
    },
    ConstructionDone {
        permit: ConstructionPermit,
        reply: Reply<()>,
    },
    MutationStart {
        request: MutationRequest,
        reply: Reply<MutationToken>,
    },
    MutationAdvance {
        token: MutationToken,
        phase: journal::MutationPhase,
        reply: Reply<()>,
    },
    MutationFinish {
        token: MutationToken,
        outcome: NativeMutationOutcome,
        reply: Reply<()>,
    },
    IntentStatus {
        profile: Uuid,
        intent: Uuid,
        reply: Reply<journal::IntentStatus>,
    },
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
    Constructed {
        token: LeaseToken,
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
                    constructions: HashMap::new(),
                    mutations: HashMap::new(),
                };
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::RecoverTerminal {
                            profile,
                            intent,
                            reply,
                        } => {
                            let result = registry.recover_intent_terminal(profile, intent);
                            let _ = reply.try_send(result);
                        }
                        Request::ConstructionStatus {
                            construction,
                            reply,
                        } => {
                            let _ = reply.try_send(Ok(registry
                                .constructions
                                .get(&construction)
                                .map(|v| v.permit)));
                        }
                        Request::ReleaseUnconstructed { token, reply } => {
                            let result = if registry
                                .leases
                                .get(&token.id)
                                .is_some_and(|v| v.token == token && !v.constructed)
                            {
                                registry.release(token)
                            } else {
                                Err(LeaseError::StaleToken)
                            };
                            let _ = reply.try_send(result);
                        }
                        Request::Acquire {
                            directory,
                            owner,
                            profile,
                            mode,
                            reply,
                        } => {
                            let _ = reply.try_send(
                                registry.acquire(directory, owner, profile, mode).map(Some),
                            );
                        }
                        Request::Status { owner, reply } => {
                            let _ = reply.try_send(Ok(registry
                                .leases
                                .values()
                                .find(|lease| lease.token.owner == owner)
                                .map(|lease| lease.token)));
                        }
                        Request::Constructed { token, reply } => {
                            let _ =
                                reply.try_send(registry.constructed(token).map(|()| Some(token)));
                        }
                        Request::Release { token, reply } => {
                            let _ = reply.try_send(registry.release(token).map(|()| None));
                        }
                        Request::Reuse {
                            directory,
                            token,
                            construction,
                            reply,
                        } => {
                            let _ = reply.try_send(registry.reuse(directory, token, construction));
                        }
                        Request::ConstructionDone { permit, reply } => {
                            let _ = reply.try_send(registry.construction_done(permit));
                        }
                        Request::MutationStart { request, reply } => {
                            let _ = reply.try_send(registry.mutation_start(request));
                        }
                        Request::MutationAdvance {
                            token,
                            phase,
                            reply,
                        } => {
                            let _ = reply.try_send(registry.mutation_advance(token, phase));
                        }
                        Request::MutationFinish {
                            token,
                            outcome,
                            reply,
                        } => {
                            let _ = reply.try_send(registry.mutation_finish(token, outcome));
                        }
                        Request::IntentStatus {
                            profile,
                            intent,
                            reply,
                        } => {
                            let _ = reply.try_send(registry.intent_status(profile, intent));
                        }
                    }
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
    /// Actual native construction has completed under this exact permit.
    /// Repeating this acknowledgment never constructs a store or changes owner.
    pub fn constructed(&self, token: LeaseToken) -> Result<(), LeaseError> {
        self.call(|reply| Request::Constructed { token, reply })
            .map(|_| ())
    }
    pub fn status(&self, owner: LeaseOwner) -> Result<Option<LeaseToken>, LeaseError> {
        self.call(|reply| Request::Status { owner, reply })
    }
    pub fn release(&self, token: LeaseToken) -> Result<(), LeaseError> {
        self.call(|reply| Request::Release { token, reply })
            .map(|_| ())
    }
    pub fn construction_status(
        &self,
        construction: Uuid,
    ) -> Result<Option<ConstructionPermit>, LeaseError> {
        self.call(|reply| Request::ConstructionStatus {
            construction,
            reply,
        })
    }
    pub fn release_unconstructed(&self, token: LeaseToken) -> Result<(), LeaseError> {
        self.call(|reply| Request::ReleaseUnconstructed { token, reply })
    }
    pub fn reuse(
        &self,
        directory: PathBuf,
        token: LeaseToken,
        construction: Uuid,
    ) -> Result<ConstructionPermit, LeaseError> {
        self.call(|reply| Request::Reuse {
            directory,
            token,
            construction,
            reply,
        })
    }
    pub fn construction_done(&self, permit: ConstructionPermit) -> Result<(), LeaseError> {
        self.call(|reply| Request::ConstructionDone { permit, reply })
    }
    pub fn mutation_start(&self, request: MutationRequest) -> Result<MutationToken, LeaseError> {
        self.call(|reply| Request::MutationStart { request, reply })
    }
    pub fn mutation_advance(
        &self,
        token: MutationToken,
        phase: journal::MutationPhase,
    ) -> Result<(), LeaseError> {
        self.call(|reply| Request::MutationAdvance {
            token,
            phase,
            reply,
        })
    }
    /// Known callback failure, or final metadata commit after known removal.
    /// Unknown callbacks must retain this owner and query status instead.
    pub fn mutation_finish(
        &self,
        token: MutationToken,
        outcome: NativeMutationOutcome,
    ) -> Result<(), LeaseError> {
        self.call(|reply| Request::MutationFinish {
            token,
            outcome,
            reply,
        })
    }
    pub fn recover_terminal(
        &self,
        profile: Uuid,
        intent: Uuid,
    ) -> Result<journal::IntentStatus, LeaseError> {
        self.call(|reply| Request::RecoverTerminal {
            profile,
            intent,
            reply,
        })
    }
    pub fn intent_status(
        &self,
        profile: Uuid,
        intent: Uuid,
    ) -> Result<journal::IntentStatus, LeaseError> {
        self.call(|reply| Request::IntentStatus {
            profile,
            intent,
            reply,
        })
    }
    fn call<T>(&self, request: impl FnOnce(Reply<T>) -> Request) -> Result<T, LeaseError> {
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
    constructions: HashMap<Uuid, mutations::PhysicalConstruction>,
    mutations: HashMap<Uuid, mutations::PhysicalMutation>,
}
struct PhysicalLease {
    token: LeaseToken,
    _lock: File,
    admission: Option<File>,
    constructed: bool,
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
        if self.mutations.values().any(|v| v.token.profile == profile) {
            return Err(LeaseError::Busy);
        }
        if self.construction_count() >= MAX_CONSTRUCTION_PERMITS {
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
        journal::recover_terminal(&self.directory, profile)?;
        let admission = open_lock(
            &self.directory,
            &journal::admission_name(profile),
            mode,
            false,
        )?;
        journal::ensure_unquarantined(&self.directory, profile)?;
        let lock = open_lock(&self.directory, &lock_name(profile), mode, false)?;
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
        self.leases.insert(
            token.id,
            PhysicalLease {
                token,
                _lock: lock,
                admission: Some(admission),
                constructed: false,
            },
        );
        Ok(token)
    }
    fn constructed(&mut self, token: LeaseToken) -> Result<(), LeaseError> {
        let lease = self
            .leases
            .get_mut(&token.id)
            .filter(|lease| lease.token == token && token.mode == LeaseMode::Shared)
            .ok_or(LeaseError::StaleToken)?;
        lease.constructed = true;
        lease.admission = None;
        Ok(())
    }
    fn release(&mut self, token: LeaseToken) -> Result<(), LeaseError> {
        if self
            .mutations
            .values()
            .any(|v| v.token.profile == token.profile)
            || self.constructions.values().any(|v| v.permit.lease == token)
        {
            return Err(LeaseError::Busy);
        }
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

fn open_file(directory: &File, name: &str) -> Result<File, LeaseError> {
    fs::openat(
        directory,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map(File::from)
    .map_err(|error| LeaseError::Io(error.to_string()))
}
fn open_lock(
    directory: &File,
    name: &str,
    mode: LeaseMode,
    allow_sentinel: bool,
) -> Result<File, LeaseError> {
    let lock = open_file(directory, name)?;
    inspect_lock(directory, &lock, name, allow_sentinel)?;
    match mode {
        LeaseMode::Shared => lock.try_lock_shared(),
        LeaseMode::Exclusive => lock.try_lock(),
    }
    .map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => LeaseError::Busy,
        std::fs::TryLockError::Error(error) => LeaseError::Io(error.to_string()),
    })?;
    inspect_lock(directory, &lock, name, allow_sentinel)?;
    Ok(lock)
}
fn inspect_lock(
    directory: &File,
    lock: &File,
    name: &str,
    allow_sentinel: bool,
) -> Result<(), LeaseError> {
    check_file(
        lock,
        journal::QUARANTINE_SENTINEL.len(),
        "browser profile lease",
    )
    .map_err(|_| LeaseError::UnsafeLock)?;
    let descriptor = fs::fstat(lock).map_err(|error| LeaseError::Io(error.to_string()))?;
    let entry = fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| LeaseError::Io(error.to_string()))?;
    if descriptor.st_nlink != 1
        || entry.st_nlink != 1
        || descriptor.st_dev != entry.st_dev
        || descriptor.st_ino != entry.st_ino
        || descriptor.st_mode != entry.st_mode
        || descriptor.st_uid != entry.st_uid
        || descriptor.st_size != entry.st_size
    {
        return Err(LeaseError::UnsafeLock);
    }
    if descriptor.st_size != 0 {
        use std::os::unix::fs::FileExt;
        let mut bytes = [0; journal::QUARANTINE_SENTINEL.len()];
        if !allow_sentinel
            || descriptor.st_size != bytes.len() as i64
            || lock
                .read_at(&mut bytes, 0)
                .map_err(|error| LeaseError::Io(error.to_string()))?
                != bytes.len()
            || bytes != journal::QUARANTINE_SENTINEL
        {
            return Err(LeaseError::UnsafeLock);
        }
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
    lower.strip_prefix("browser-profile-").is_some_and(|value| {
        [".admission.lock", ".intent.json", ".lock"]
            .iter()
            .any(|suffix| {
                value
                    .strip_suffix(suffix)
                    .is_some_and(|uuid| Uuid::parse_str(uuid).is_ok())
            })
    })
}
