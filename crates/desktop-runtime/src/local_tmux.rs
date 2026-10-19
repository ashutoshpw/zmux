//! Opt-in local tmux operations. Profile I/O and processes belong to background
//! workers; the workspace executor only admits routes and commits typed effects.
use crate::DesktopError;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::local_multiplexer::{
    LocalTmux, LocalTmuxIdentity, SessionSelector, SessionStatus,
};
use zmux_workspace_core::{
    PaneId, SessionId, WorkspaceId,
    targets::{CallerContext, TargetRef},
};

const MAX_PENDING: usize = 4;
const MAX_RECEIPTS: usize = 32;
const DEADLINE: Duration = Duration::from_secs(20);

/// Installation profile selection for embedded/native hosts. Resolution and
/// validation of the environment form happen on the operation/launch worker.
#[derive(Clone, Default)]
pub enum Profile {
    #[default]
    Environment,
    Explicit(LocalTmux),
}
impl Profile {
    pub(crate) fn resolve(&self, cancel: Arc<AtomicBool>) -> Result<LocalTmux, String> {
        let profile = match self {
            Self::Environment => LocalTmux::from_environment(),
            Self::Explicit(profile) => Ok(profile.clone()),
        }
        .map_err(|e| e.to_string())?;
        Ok(profile.with_cancellation(cancel))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Start {
        name: String,
        cwd: PathBuf,
        command: Option<String>,
    },
    Attach {
        selector: SessionSelector,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Operation,
    #[serde(default)]
    pub workspace: Option<TargetRef>,
    #[serde(default)]
    pub caller: CallerContext,
    #[serde(default = "default_focus")]
    pub focus: bool,
    #[serde(default)]
    pub new_client: bool,
}
fn default_focus() -> bool {
    true
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub epoch: Uuid,
    pub operation_id: u64,
    pub state: String,
    pub identity: Option<LocalTmuxIdentity>,
    pub owner_outcome: String,
    pub workspace: Option<Uuid>,
    pub surface: Option<Uuid>,
    pub reused: bool,
    pub error: Option<String>,
}
#[derive(Clone)]
pub(crate) struct Route {
    pub namespace: Uuid,
    pub workspace: Option<WorkspaceId>,
    pub pane: Option<PaneId>,
    pub explicit: bool,
}
pub(crate) struct ResolvedOwner {
    pub status: SessionStatus,
    pub created: bool,
}
pub(crate) struct Resolution {
    pub id: u64,
    pub route: Route,
    pub focus: bool,
    pub new_client: bool,
    pub result: Result<ResolvedOwner, String>,
}
pub(crate) struct Remembered {
    id: u64,
    binding: zmux_remote_runtime::local_multiplexer::GuiBinding,
    result: Result<(), String>,
}
struct Job {
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    handle: JoinHandle<()>,
}
pub(crate) struct Runtime {
    pub epoch: Uuid,
    pub reserved: HashSet<u64>,
    pub profile: Profile,
    pub receipts: Vec<Receipt>,
    next_id: u64,
    dirty: bool,
    tx: SyncSender<Resolution>,
    rx: Receiver<Resolution>,
    remember_tx: SyncSender<Remembered>,
    remember_rx: Receiver<Remembered>,
    queued_bindings: Vec<(u64, zmux_remote_runtime::local_multiplexer::GuiBinding)>,
    jobs: HashMap<u64, Job>,
    pub waiting: HashMap<SessionId, u64>,
}
impl Runtime {
    pub fn new(profile: Profile) -> Self {
        let (tx, rx) = mpsc::sync_channel(MAX_PENDING);
        let (remember_tx, remember_rx) = mpsc::sync_channel(MAX_PENDING);
        Self {
            epoch: Uuid::new_v4(),
            reserved: HashSet::new(),
            profile,
            receipts: Vec::new(),
            next_id: 1,
            dirty: false,
            tx,
            rx,
            remember_tx,
            remember_rx,
            queued_bindings: Vec::new(),
            jobs: HashMap::new(),
            waiting: HashMap::new(),
        }
    }
    pub fn pending(&self) -> usize {
        self.receipts
            .iter()
            .filter(|r| r.state == "pending")
            .count()
    }
    pub fn admit(&mut self, request: Request, route: Route) -> Result<u64, DesktopError> {
        if self.pending() >= MAX_PENDING || self.jobs.len() >= MAX_PENDING {
            return Err(DesktopError::Failure(
                "four local tmux operations are already pending".into(),
            ));
        }
        if self.receipts.len() == MAX_RECEIPTS {
            let index = self
                .receipts
                .iter()
                .position(|r| r.state != "pending")
                .ok_or_else(|| DesktopError::Failure("local tmux receipts are full".into()))?;
            self.receipts.remove(index);
        }
        let creates_owner = matches!(request.operation, Operation::Start { .. });
        let id = self.next_id;
        let next = id
            .checked_add(1)
            .ok_or_else(|| DesktopError::Failure("local tmux operation IDs exhausted".into()))?;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let profile = self.profile.clone();
        let sender = self.tx.clone();
        let handle = thread::Builder::new()
            .name(format!("zmux-tmux-{id}"))
            .spawn(move || {
                let result = (|| {
                    let owner = profile.resolve(worker_cancel.clone())?;
                    let (created, status) = match request.operation {
                        Operation::Start { name, cwd, command } => (
                            true,
                            owner
                                .start(&name, &cwd, command.as_deref())
                                .map_err(|e| e.to_string())?,
                        ),
                        Operation::Attach { selector } => {
                            let status = owner.status(&selector).map_err(|e| e.to_string())?;
                            if status.state != "live" {
                                return Err("local tmux binding is stale".into());
                            }
                            (false, status)
                        }
                    };
                    Ok(ResolvedOwner { status, created })
                })();
                let _ = sender.send(Resolution {
                    id,
                    route,
                    focus: request.focus,
                    new_client: request.new_client,
                    result,
                });
            })
            .map_err(|e| DesktopError::Failure(format!("start local tmux worker: {e}")))?;
        self.next_id = next;
        if creates_owner {
            self.reserved.insert(id);
        }
        self.jobs.insert(
            id,
            Job {
                cancel,
                deadline: Instant::now() + DEADLINE,
                handle,
            },
        );
        self.receipts.push(Receipt {
            epoch: self.epoch,
            operation_id: id,
            state: "pending".into(),
            identity: None,
            owner_outcome: "pending".into(),
            workspace: None,
            surface: None,
            reused: false,
            error: None,
        });
        Ok(id)
    }
    pub(crate) fn drain(&mut self) -> (Vec<Resolution>, Vec<Remembered>) {
        for job in self.jobs.values() {
            if Instant::now() >= job.deadline {
                job.cancel.store(true, Ordering::Release);
            }
        }
        let finished: Vec<_> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.handle.is_finished())
            .map(|(id, _)| *id)
            .collect();
        for id in finished {
            if self
                .jobs
                .remove(&id)
                .expect("finished worker")
                .handle
                .join()
                .is_err()
            {
                self.finish(
                    id,
                    Err("local tmux worker panicked; owner outcome may be unknown".into()),
                );
            }
        }
        let queued = std::mem::take(&mut self.queued_bindings);
        for (id, binding) in queued {
            if self.jobs.contains_key(&id) || self.jobs.len() >= MAX_PENDING {
                self.queued_bindings.push((id, binding));
                continue;
            }
            let Some(identity) = self
                .receipts
                .iter()
                .find(|r| r.operation_id == id && r.state == "pending")
                .and_then(|r| r.identity.clone())
            else {
                continue;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let profile = self.profile.clone();
            let sender = self.remember_tx.clone();
            match thread::Builder::new()
                .name(format!("zmux-tmux-binding-{id}"))
                .spawn(move || {
                    let result = profile.resolve(worker_cancel).and_then(|owner| {
                        owner
                            .remember_gui(&identity, binding.clone())
                            .map_err(|e| e.to_string())
                    });
                    let _ = sender.send(Remembered {
                        id,
                        binding,
                        result,
                    });
                }) {
                Ok(handle) => {
                    self.jobs.insert(
                        id,
                        Job {
                            cancel,
                            deadline: Instant::now() + DEADLINE,
                            handle,
                        },
                    );
                }
                Err(error) => self.finish(id, Err(format!("start tmux binding worker: {error}"))),
            }
        }
        let mut results = Vec::new();
        while let Ok(resolution) = self.rx.try_recv() {
            if self
                .receipts
                .iter()
                .any(|r| r.operation_id == resolution.id && r.state == "pending")
            {
                results.push(resolution);
            }
        }
        (results, self.remember_rx.try_iter().collect())
    }
    fn remember(&mut self, id: u64, binding: zmux_remote_runtime::local_multiplexer::GuiBinding) {
        self.queued_bindings.push((id, binding));
    }
    pub fn resolved(&mut self, id: u64, owner: &ResolvedOwner) {
        if let Some(r) = self.receipts.iter_mut().find(|r| r.operation_id == id) {
            r.identity = Some(owner.status.identity());
            r.owner_outcome = if owner.created { "created" } else { "existing" }.into();
        }
    }
    pub fn finish(&mut self, id: u64, result: Result<(Uuid, Uuid, bool), String>) {
        self.dirty = true;
        self.reserved.remove(&id);
        if let Some(r) = self.receipts.iter_mut().find(|r| r.operation_id == id) {
            match result {
                Ok((workspace, surface, reused)) => {
                    r.state = "completed".into();
                    r.workspace = Some(workspace);
                    r.surface = Some(surface);
                    r.reused = reused;
                }
                Err(error) => {
                    r.state = "failed".into();
                    r.error = Some(error.chars().take(1024).collect());
                    if r.owner_outcome == "pending" {
                        r.owner_outcome = "unknown".into();
                    }
                }
            }
        }
    }
    pub fn cancel_all(&mut self) {
        for job in self.jobs.values() {
            job.cancel.store(true, Ordering::Release);
        }
        for receipt in &mut self.receipts {
            if receipt.state == "pending" {
                receipt.state = "failed".into();
                if receipt.owner_outcome == "pending" {
                    receipt.owner_outcome = "unknown".into();
                }
                receipt.error=Some("desktop shutdown cancelled client attachment; inspect local-tmux list for durable owner state".into());
            }
        }
        self.waiting.clear();
        self.reserved.clear();
        self.queued_bindings.clear();
    }
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
    pub fn worker_count(&self) -> usize {
        self.jobs.len()
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub(crate) struct ClientLaunchFailure {
    pub error: crate::SessionError,
    pub cleanup: Option<Box<crate::Session>>,
}
impl From<crate::SessionError> for ClientLaunchFailure {
    fn from(error: crate::SessionError) -> Self {
        Self {
            error,
            cleanup: None,
        }
    }
}

/// Run only on a launch worker. A spawned process is not yet an attached client:
/// confirm its PID against the immutable managed session before publishing success.
pub(crate) fn launch_client(
    profile: Profile,
    identity: LocalTmuxIdentity,
    environment: &[(String, String)],
    binding: zmux_remote_runtime::local_multiplexer::GuiBinding,
    cancel: Arc<AtomicBool>,
    terminal_engine: Option<zmux_session_runtime::EngineOptions>,
) -> Result<crate::Session, ClientLaunchFailure> {
    use crate::SessionError;
    let failure = |e: String| {
        SessionError::Failure(format!(
            "local tmux client: {e}; durable owner was not terminated"
        ))
    };
    let owner = profile.resolve(cancel.clone()).map_err(failure)?;
    let process = owner
        .attach_identity(&identity)
        .map_err(|e| failure(e.to_string()))?;
    let removals: Vec<_> = std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| {
            name.to_str().is_some_and(|name| {
                name.starts_with("ZMUX_") || name == "TMUX" || name == "TMUX_PANE"
            })
        })
        .collect();
    let mut context = environment.to_vec();
    context.push(("ZMUX_LOCAL_TMUX".into(), "1".into()));
    let session = crate::terminal_engine::spawn(
        zmux_session_runtime::Launch {
            program: process.program,
            args: process.args,
            cwd: None,
            size: zmux_session_runtime::default_size(),
        },
        &context,
        &removals,
        terminal_engine,
        (),
    )?;
    let confirmation = (|| -> Result<(), SessionError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(failure("attachment was cancelled".into()));
            }
            let status = owner
                .status_identity(&identity)
                .map_err(|e| failure(e.to_string()))?;
            if status.state != "live" {
                return Err(failure("managed session is stale".into()));
            }
            if status
                .clients
                .iter()
                .any(|c| c.process_id == session.process_id())
            {
                break;
            }
            if let Ok(snapshot) = session.snapshot()
                && snapshot.state != zmux_session_runtime::State::Running
            {
                return Err(failure("tmux client exited before attachment".into()));
            }
            if Instant::now() >= deadline {
                return Err(failure("attachment confirmation timed out".into()));
            }
            thread::sleep(Duration::from_millis(10));
        }
        owner
            .remember_gui(&identity, binding)
            .map_err(|e| failure(e.to_string()))?;
        Ok(())
    })();
    match confirmation {
        Ok(()) => Ok(session),
        Err(error) => Err(ClientLaunchFailure {
            error,
            cleanup: Some(Box::new(session)),
        }),
    }
}

impl crate::Worker {
    pub(crate) fn local_tmux_command(&mut self, request: Request) -> Result<(), DesktopError> {
        use zmux_workspace_core::targets::TargetScope;
        let model = self.model.snapshot();
        if let Operation::Start { name, cwd, command } = &request.operation {
            SessionSelector::name(name).map_err(|e| DesktopError::Failure(e.to_string()))?;
            if !cwd.is_absolute()
                || cwd
                    .to_str()
                    .is_none_or(|s| s.len() > 4096 || s.chars().any(char::is_control))
                || command
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty() || s.len() > 8192 || s.contains('\0'))
            {
                return Err(DesktopError::Failure(
                    "invalid bounded local tmux start command/cwd".into(),
                ));
            }
            let reserved = self.local_tmux.reserved.len();
            let owned = self.pending_launches
                + self.retired.len()
                + self.slots.values().filter(|s| s.session.is_some()).count();
            if self.slots.len() + reserved + 1 > crate::MAX_TERMINALS
                || owned + reserved + 1 > crate::MAX_TERMINALS
            {
                return Err(DesktopError::Failure(
                    "terminal capacity reached before local tmux owner creation".into(),
                ));
            }
        }
        let explicit = request.workspace.is_some()
            || request.caller.workspace.is_some()
            || request.caller.surface.is_some()
            || request.caller.local_tmux.is_some();
        let target = TargetScope {
            workspace: request.workspace.clone(),
            ..Default::default()
        }
        .resolve(&model, &request.caller)?;
        let route = Route {
            namespace: model.identity_namespace,
            workspace: Some(target.workspace),
            pane: Some(target.pane),
            explicit,
        };
        self.tmux_operation_id = Some(self.local_tmux.admit(request, route)?);
        Ok(())
    }
    pub(crate) fn poll_local_tmux(&mut self) -> bool {
        let (results, remembered) = self.local_tmux.drain();
        let changed = !results.is_empty() || !remembered.is_empty();
        for completion in remembered {
            let model = self.model.snapshot();
            let binding = completion.binding;
            let live = model.identity_namespace == binding.identity_namespace
                && model.workspaces.iter().any(|w| {
                    model.object_uuid(zmux_workspace_core::targets::TargetKind::Workspace, w.id.0)
                        == binding.workspace
                        && w.root.panes().iter().any(|p| {
                            p.surfaces.iter().any(|s| {
                                model.object_uuid(
                                    zmux_workspace_core::targets::TargetKind::Surface,
                                    s.id.0,
                                ) == binding.surface
                            })
                        })
                });
            let result=completion.result.and_then(|()|if live {Ok((binding.workspace,binding.surface,true))}else{Err("reused surface moved or closed before binding completion; durable owner was not terminated".into())});
            self.local_tmux.finish(completion.id, result);
        }
        for resolution in results {
            let id = resolution.id;
            match resolution.result {
                Err(error) => self.local_tmux.finish(id, Err(error)),
                Ok(owner) => {
                    self.local_tmux.resolved(id, &owner);
                    self.local_tmux.reserved.remove(&id);
                    if let Err(error) = self.commit_local_tmux(
                        id,
                        resolution.route,
                        resolution.focus,
                        resolution.new_client,
                        &owner.status,
                    ) {
                        self.local_tmux.finish(
                            id,
                            Err(format!("{error}; durable owner was not terminated")),
                        );
                    }
                }
            }
        }
        self.local_tmux.take_dirty() || changed
    }
    fn commit_local_tmux(
        &mut self,
        id: u64,
        route: Route,
        focus: bool,
        new_client: bool,
        status: &SessionStatus,
    ) -> Result<(), DesktopError> {
        use zmux_workspace_core::{Action, targets::TargetKind};
        let model = self.model.snapshot();
        if model.identity_namespace != route.namespace {
            return Err(DesktopError::Failure(
                "target desktop identity changed while tmux operation was pending".into(),
            ));
        }
        let identity = status.identity();
        let (mut workspace, mut pane) = (route.workspace, route.pane);
        if !route.explicit
            && let Some(binding) = &status.gui_binding
        {
            if binding.identity_namespace != model.identity_namespace {
                return Err(DesktopError::Failure(
                    "saved tmux workspace is stale; pass --workspace explicitly".into(),
                ));
            }
            let live_host = model.workspaces.iter().find_map(|w| {
                w.root.panes().into_iter().find_map(|p| {
                    p.surfaces
                        .iter()
                        .find(|surface| {
                            model.object_uuid(TargetKind::Surface, surface.id.0) == binding.surface
                                && surface.local_tmux.as_ref() == Some(&identity)
                        })
                        .map(|_| (w.id, p.id))
                })
            });
            if let Some((current_workspace, current_pane)) = live_host {
                workspace = Some(current_workspace);
                pane = Some(current_pane);
            } else {
                let target = model
                    .workspaces
                    .iter()
                    .find(|w| model.object_uuid(TargetKind::Workspace, w.id.0) == binding.workspace)
                    .ok_or_else(|| {
                        DesktopError::Failure(
                            "saved tmux workspace is stale; pass --workspace explicitly".into(),
                        )
                    })?;
                workspace = Some(target.id);
                pane = Some(target.focused_pane);
            }
        }
        let workspace = workspace.ok_or_else(|| {
            DesktopError::Failure("tmux attachment needs a live workspace".into())
        })?;
        let pane =
            pane.ok_or_else(|| DesktopError::Failure("tmux attachment needs a live pane".into()))?;
        let target = model
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .filter(|w| w.root.panes().iter().any(|p| p.id == pane))
            .ok_or_else(|| {
                DesktopError::Failure(
                    "target workspace/pane closed while tmux operation was pending".into(),
                )
            })?;
        if !new_client {
            let matches: Vec<_> = target
                .root
                .panes()
                .into_iter()
                .flat_map(|p| &p.surfaces)
                .filter(|s| s.local_tmux.as_ref() == Some(&identity))
                .collect();
            let remembered = status.gui_binding.as_ref().and_then(|binding| {
                matches
                    .iter()
                    .copied()
                    .find(|s| model.object_uuid(TargetKind::Surface, s.id.0) == binding.surface)
            });
            let existing = if let Some(remembered) = remembered {
                Some(remembered)
            } else if matches.len() > 1 {
                return Err(DesktopError::Failure(
                    "multiple client surfaces match; use --new-client explicitly".into(),
                ));
            } else {
                matches.first().copied()
            };
            if let Some(surface) = existing {
                let attached = surface
                    .session_id
                    .and_then(|id| self.slots.get(&id))
                    .and_then(|slot| slot.session.as_ref())
                    .is_some_and(|session| {
                        status
                            .clients
                            .iter()
                            .any(|client| client.process_id == session.process_id())
                    });
                if !attached {
                    return Err(DesktopError::Failure(
                        "saved client surface is not attached; use --new-client for a replacement"
                            .into(),
                    ));
                }
                if focus {
                    self.action(Action::SelectSurface {
                        surface_id: surface.id,
                    })?;
                }
                self.local_tmux.remember(
                    id,
                    zmux_remote_runtime::local_multiplexer::GuiBinding {
                        identity_namespace: model.identity_namespace,
                        workspace: model.object_uuid(TargetKind::Workspace, workspace.0),
                        surface: model.object_uuid(TargetKind::Surface, surface.id.0),
                    },
                );
                return Ok(());
            }
        }
        let before: Vec<_> = model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .map(|s| s.id)
            .collect();
        self.action(Action::CreateLocalTmuxSurface {
            pane_id: pane,
            identity,
            focus,
        })?;
        let current = self.model.snapshot();
        let session = current
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|s| !before.contains(&s.id))
            .and_then(|s| s.session_id)
            .expect("created tmux terminal owns session");
        self.local_tmux.waiting.insert(session, id);
        Ok(())
    }
    pub(crate) fn complete_local_tmux(&mut self, completion: &crate::Completion) {
        use zmux_workspace_core::targets::TargetKind;
        let Some(id) = self.local_tmux.waiting.remove(&completion.id) else {
            return;
        };
        let model = self.model.snapshot();
        let result=match &completion.result {
            Err(error)=>Err(error.to_string()),
            Ok(_)=>model.workspaces.iter().find_map(|w|w.root.panes().into_iter().find_map(|p|p.surfaces.iter().find(|s|s.session_id==Some(completion.id)).map(|s|(model.object_uuid(TargetKind::Workspace,w.id.0),model.object_uuid(TargetKind::Surface,s.id.0),false)))).ok_or_else(||"target surface closed before attachment completed; durable owner was not terminated".into()),
        };
        self.local_tmux.finish(id, result);
    }
}
