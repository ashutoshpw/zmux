//! Installation-local browser profile metadata with immutable identities. WebKit owns website
//! data. All filesystem operations run through PersistenceWorker, never the executor.
use crate::{
    DesktopError,
    persistence::{PersistenceOutcome, PersistenceWorker},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use thiserror::Error;
use uuid::Uuid;
use zmux_workspace_core::{RestoreLimits, Snapshot};

pub const MAX_PROFILES: usize = 32;
pub const MAX_PROFILE_BYTES: usize = 16 * 1024;
pub const CATALOG_NAME: &str = "browser-profiles.json";

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("native browser profile owner is unavailable; no website data or catalog was changed")]
    NativeUnavailable,
}
impl ProfileError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NativeUnavailable => "native_profile_owner_unavailable",
        }
    }
}
fn failure(value: impl ToString) -> DesktopError {
    DesktopError::Failure(value.to_string())
}
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || name.len() > 128
        || name.chars().any(char::is_control)
        || Uuid::parse_str(name).is_ok()
    {
        return Err("profile name must be trimmed, nonempty, at most 128 UTF-8 bytes, contain no controls and not be a UUID".into());
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub profiles: Vec<Profile>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: Vec::new(),
        }
    }
}
impl Catalog {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.profiles.len() > MAX_PROFILES {
            return Err("unsupported or oversized browser profile catalog".into());
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for profile in &self.profiles {
            validate_name(&profile.name)?;
            if profile.id.is_nil() || !ids.insert(profile.id) || !names.insert(&profile.name) {
                return Err("duplicate or invalid browser profile identity/name".into());
            }
        }
        Ok(())
    }
    pub fn rename(&mut self, selector: &str, name: String) -> Result<Profile, String> {
        validate_name(&name)?;
        let index = self
            .profiles
            .iter()
            .position(|profile| {
                profile.name == selector || Uuid::parse_str(selector).ok() == Some(profile.id)
            })
            .ok_or("unknown browser profile")?;
        if self
            .profiles
            .iter()
            .enumerate()
            .any(|(other, profile)| other != index && profile.name == name)
        {
            return Err("browser profile name already exists".into());
        }
        self.profiles[index].name = name;
        Ok(self.profiles[index].clone())
    }
    pub fn add(&mut self, name: String) -> Result<Profile, String> {
        validate_name(&name)?;
        if self.profiles.len() >= MAX_PROFILES {
            return Err("browser profile capacity reached (32)".into());
        }
        if self.profiles.iter().any(|profile| profile.name == name) {
            return Err("browser profile name already exists".into());
        }
        let profile = Profile {
            id: Uuid::new_v4(),
            name,
        };
        self.profiles.push(profile.clone());
        Ok(profile)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileCommand {
    Add { name: String },
    Rename { profile: String, name: String },
    Clear { profile: String },
    Delete { profile: String },
}
#[derive(Clone, Debug, Serialize)]
pub struct ProfileReceipt {
    pub epoch: Uuid,
    pub operation_id: u64,
    pub intent: Option<Uuid>,
    pub state: String,
    pub profile: Option<Profile>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct ProfileSnapshot {
    pub epoch: Uuid,
    pub enabled: bool,
    pub loading: bool,
    pub profiles: Vec<Profile>,
    pub operations: Vec<ProfileReceipt>,
    pub error: Option<String>,
}
pub(crate) struct ProfileRuntime {
    io: Option<PersistenceWorker>,
    pub snapshot: ProfileSnapshot,
    pending: HashMap<u64, (u64, Option<String>)>,
    next_id: u64,
    shutting_down: bool,
    native: Option<Arc<crate::profile_native::NativeProfileOperations>>,
    native_pending: HashMap<Uuid, (u64, bool)>,
}
impl ProfileRuntime {
    pub fn start(
        directory: Option<PathBuf>,
        native: Option<Arc<crate::profile_native::NativeProfileOperations>>,
    ) -> Result<Self, DesktopError> {
        let mut runtime = Self {
            io: None,
            snapshot: ProfileSnapshot::default(),
            pending: HashMap::new(),
            next_id: 1,
            shutting_down: false,
            native,
            native_pending: HashMap::new(),
        };
        runtime.snapshot.epoch = Uuid::new_v4();
        if let Some(directory) = directory {
            let io = PersistenceWorker::start(
                directory,
                RestoreLimits {
                    terminals: 0,
                    browsers: 0,
                },
            )
            .map_err(failure)?;
            let operation = io.submit_profiles(None).map_err(failure)?;
            runtime.pending.insert(operation.0, (0, None));
            runtime.io = Some(io);
            runtime.snapshot.enabled = true;
            runtime.snapshot.loading = true;
        }
        Ok(runtime)
    }
    pub fn admit(&mut self, command: ProfileCommand) -> Result<u64, DesktopError> {
        if self.snapshot.loading {
            return Err(failure("browser profile catalog is loading"));
        }
        if let Some(error) = &self.snapshot.error {
            return Err(failure(error));
        }
        if let ProfileCommand::Clear { profile } | ProfileCommand::Delete { profile } = &command {
            let profile_id = self.resolve(profile)?;
            let native = self
                .native
                .as_ref()
                .ok_or(ProfileError::NativeUnavailable)?;
            if self.snapshot.operations.len() == 16
                && self
                    .snapshot
                    .operations
                    .iter()
                    .all(|v| v.state == "pending")
            {
                return Err(failure("browser profile receipts are full"));
            }
            let id = self.next_id;
            let next = id
                .checked_add(1)
                .ok_or_else(|| failure("browser profile operation IDs exhausted"))?;
            let kind = if matches!(&command, ProfileCommand::Clear { .. }) {
                crate::profile_leases::journal::MutationKind::Clear
            } else {
                crate::profile_leases::journal::MutationKind::Delete
            };
            let intent = native.admit(profile_id, kind).map_err(failure)?;
            if self.snapshot.operations.len() == 16 {
                let index = self
                    .snapshot
                    .operations
                    .iter()
                    .position(|v| v.state != "pending")
                    .unwrap();
                self.snapshot.operations.remove(index);
            }
            self.next_id = next;
            self.native_pending.insert(intent, (id, false));
            self.snapshot.operations.push(ProfileReceipt {
                epoch: self.snapshot.epoch,
                operation_id: id,
                intent: Some(intent),
                state: "pending".into(),
                profile: self
                    .snapshot
                    .profiles
                    .iter()
                    .find(|v| v.id == profile_id)
                    .cloned(),
                error: None,
            });
            return Ok(id);
        }
        let io = self
            .io
            .as_ref()
            .ok_or_else(|| failure("browser profiles require durable runtime state"))?;
        let name = match &command {
            ProfileCommand::Add { name } => name,
            ProfileCommand::Rename { profile, name } => {
                if profile.is_empty()
                    || profile.len() > 128
                    || profile.chars().any(char::is_control)
                {
                    return Err(failure("invalid browser profile selector"));
                }
                name
            }
            ProfileCommand::Clear { .. } | ProfileCommand::Delete { .. } => {
                return Err(ProfileError::NativeUnavailable.into());
            }
        };
        validate_name(name).map_err(failure)?;
        let name = name.clone();
        if self.snapshot.operations.len() == 16 {
            let index = self
                .snapshot
                .operations
                .iter()
                .position(|op| op.state != "pending")
                .ok_or_else(|| failure("browser profile receipts are full"))?;
            self.snapshot.operations.remove(index);
        }
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or_else(|| failure("browser profile operation IDs exhausted"))?;
        let operation = io.submit_profile_command(command).map_err(failure)?;
        self.next_id = next;
        self.pending.insert(operation.0, (id, Some(name)));
        self.snapshot.operations.push(ProfileReceipt {
            epoch: self.snapshot.epoch,
            operation_id: id,
            intent: None,
            state: "pending".into(),
            profile: None,
            error: None,
        });
        Ok(id)
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(native) = &self.native {
            for (intent, result) in native.outcomes() {
                let Some((id, refreshing)) = self.native_pending.get_mut(&intent) else {
                    native.retire(intent);
                    continue;
                };
                if *refreshing {
                    continue;
                }
                match result {
                    Ok(()) => match self.io.as_ref().map(|io| io.submit_profiles(None)) {
                        Some(Ok(operation)) => {
                            self.pending.insert(operation.0, (*id, None));
                            *refreshing = true;
                        }
                        Some(Err(error)) => {
                            if let Some(op) = self
                                .snapshot
                                .operations
                                .iter_mut()
                                .find(|v| v.operation_id == *id)
                            {
                                op.error = Some(format!(
                                    "native completion known; catalog refresh pending: {error}"
                                ));
                                changed = true;
                            }
                        }
                        None => {
                            if let Some(op) = self
                                .snapshot
                                .operations
                                .iter_mut()
                                .find(|v| v.operation_id == *id)
                            {
                                op.error = Some("native completion known; profile catalog worker unavailable for refresh".into());
                                changed = true;
                            }
                        }
                    },
                    Err(error) => {
                        if let Some(op) = self
                            .snapshot
                            .operations
                            .iter_mut()
                            .find(|v| v.operation_id == *id)
                        {
                            op.state = "failed".into();
                            op.error = Some(error);
                        }
                        self.native_pending.remove(&intent);
                        native.retire(intent);
                        changed = true;
                    }
                }
            }
        }
        loop {
            let completion = match self.io.as_ref().map(|io| io.try_completion()) {
                Some(Ok(Some(completion))) => completion,
                Some(Err(_)) if self.shutting_down => break,
                Some(Err(error)) => {
                    let message = error.to_string();
                    let changed =
                        self.snapshot.error.as_ref() != Some(&message) || self.snapshot.loading;
                    self.snapshot.error = Some(message.clone());
                    self.snapshot.loading = false;
                    for op in &mut self.snapshot.operations {
                        if op.state == "pending" {
                            op.state = "failed".into();
                            op.error = Some(format!("{message}; metadata outcome may be unknown"));
                        }
                    }
                    self.pending.clear();
                    return changed;
                }
                _ => break,
            };
            changed = true;
            let Some((id, name)) = self.pending.remove(&completion.operation_id.0) else {
                continue;
            };
            match completion.result {
                Ok(PersistenceOutcome::Profiles(catalog)) => {
                    if let Some(op) = self
                        .snapshot
                        .operations
                        .iter_mut()
                        .find(|op| op.operation_id == id)
                    {
                        if op.intent.is_none() {
                            op.profile = catalog
                                .profiles
                                .iter()
                                .find(|profile| Some(&profile.name) == name.as_ref())
                                .cloned();
                        }
                        op.state = "completed".into();
                    }
                    self.snapshot.profiles = catalog.profiles;
                    if let Some(intent) = self
                        .snapshot
                        .operations
                        .iter()
                        .find(|v| v.operation_id == id)
                        .and_then(|v| v.intent)
                    {
                        self.native_pending.remove(&intent);
                        if let Some(native) = &self.native {
                            native.retire(intent);
                        }
                    }
                }
                result => {
                    let error = match result {
                        Err(error) => error.to_string(),
                        _ => "invalid browser profile worker completion".into(),
                    };
                    if id == 0 {
                        self.snapshot.error = Some(error)
                    } else if let Some(op) = self
                        .snapshot
                        .operations
                        .iter_mut()
                        .find(|op| op.operation_id == id)
                    {
                        if let Some(intent) = op.intent {
                            op.state = "pending".into();
                            op.error = Some(format!(
                                "native completion known; catalog refresh required: {error}"
                            ));
                            if let Some((_, refreshing)) = self.native_pending.get_mut(&intent) {
                                *refreshing = false;
                            }
                        } else {
                            op.state = "failed".into();
                            op.error = Some(error);
                        }
                    }
                }
            }
            if id == 0 {
                self.snapshot.loading = false;
            }
        }
        changed
    }
    pub(crate) fn poll_shutdown(&mut self) -> Result<bool, crate::persistence::PersistenceError> {
        self.shutting_down = true;
        if let Some(native) = &self.native {
            native.cancel_queued();
        }
        self.poll();
        if self.native.as_ref().is_some_and(|v| v.pending() > 0) {
            return Ok(false);
        }
        let Some(io) = &mut self.io else {
            return Ok(true);
        };
        io.request_shutdown();
        if !io.is_finished() {
            return Ok(false);
        }
        self.shutdown(std::time::Instant::now())?;
        Ok(true)
    }
    pub fn shutdown(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<(), crate::persistence::PersistenceError> {
        self.shutting_down = true;
        let result = self
            .io
            .as_mut()
            .map(|io| io.join_until(deadline))
            .unwrap_or(Ok(()));
        if let Some(io) = &self.io {
            io.request_shutdown();
        }
        self.poll();
        for op in &mut self.snapshot.operations {
            if op.state == "pending" {
                op.state = "failed".into();
                op.error=Some("desktop shutdown; durable profile metadata outcome may be unknown; operation was not replayed".into());
            }
        }
        self.pending.clear();
        result
    }
    pub fn resolve(&self, selector: &str) -> Result<Uuid, DesktopError> {
        if self.snapshot.loading {
            return Err(failure("browser profile catalog is loading"));
        }
        if let Some(error) = &self.snapshot.error {
            return Err(failure(error));
        }
        self.snapshot
            .profiles
            .iter()
            .find(|profile| {
                profile.name == selector || Uuid::parse_str(selector).ok() == Some(profile.id)
            })
            .map(|profile| profile.id)
            .ok_or_else(|| failure("unknown browser profile"))
            .and_then(|id| {
                if self.native.as_ref().is_some_and(|v| v.fenced(id)) {
                    Err(failure("browser profile has a pending native operation"))
                } else {
                    Ok(id)
                }
            })
    }
    pub fn validate_model(&self, model: &Snapshot) -> Result<(), DesktopError> {
        for id in model
            .workspaces
            .iter()
            .flat_map(|v| v.root.panes())
            .flat_map(|v| &v.surfaces)
            .filter_map(|v| v.browser_profile_id)
        {
            self.resolve(&id.to_string())?;
        }
        Ok(())
    }
    pub fn validate_transition(
        &self,
        before: &Snapshot,
        model: &Snapshot,
    ) -> Result<(), DesktopError> {
        for surface in model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
        {
            let Some(id) = surface.browser_profile_id else {
                continue;
            };
            if !self.snapshot.profiles.iter().any(|v| v.id == id) {
                return Err(failure("unknown browser profile"));
            }
            let unchanged = before.identity_namespace == model.identity_namespace
                && before
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|p| &p.surfaces)
                    .any(|old| old.id == surface.id && old.browser_profile_id == Some(id));
            if !unchanged {
                self.resolve(&id.to_string())?;
            }
        }
        Ok(())
    }
}
