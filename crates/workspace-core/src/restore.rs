//! Versioned topology descriptors. No process state, input, filesystem or network I/O.
//!
//! V1 preserves selections; V2 adds browser profiles; V3 adds immutable local tmux
//! descriptors; V4 adds canonical window membership; V5 adds workspace todos; V6 adds attach-only local Zellij identity.
//! Earlier versions retain explicit readers.
//! No version preserves undo-close or WebKit navigation history. Browser query
//! strings/fragments and observed browser titles are deliberately omitted for privacy.
//! Page state and native split ratios need their own future schema version.
use crate::*;
use std::collections::HashSet;
use zmux_remote_runtime::{JumpConfig, SshConnectionConfig};

pub const SESSION_DOCUMENT_VERSION: u32 = 6;
const FORMAT: &str = "zmux.session";
// A starter workspace needs its original four IDs and one checked window ID.
pub const MAX_NEXT_IDENTIFIER: u64 = u64::MAX - 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDocument {
    pub format: String,
    pub version: u32,
    pub identity_namespace: uuid::Uuid,
    pub next_id: u64,
    pub focused_workspace: Option<WorkspaceId>,
    pub workspaces: Vec<SavedWorkspace>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "window_layout"
    )]
    pub window_layout: Option<SavedWindowLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedWindowLayout {
    pub windows: Vec<Window>,
    pub focused_window: Option<WindowId>,
}

// An omitted field belongs to legacy versions; an explicit null is not a layout.
fn window_layout<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<SavedWindowLayout>, D::Error> {
    SavedWindowLayout::deserialize(decoder).map(Some)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedWorkspace {
    pub id: WorkspaceId,
    pub name: String,
    pub focused_pane: PaneId,
    pub root: SavedTree,
    #[serde(default, skip_serializing_if = "SavedTodos::is_missing")]
    pub todos: SavedTodos,
}

/// Omitted in legacy versions; v5 requires an explicit array, never null.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedTodos {
    pub items: Vec<crate::todo::TodoEntry>,
    present: bool,
}
impl SavedTodos {
    fn is_missing(&self) -> bool {
        !self.present
    }
}
impl From<Vec<crate::todo::TodoEntry>> for SavedTodos {
    fn from(items: Vec<crate::todo::TodoEntry>) -> Self {
        Self {
            items,
            present: true,
        }
    }
}
impl Serialize for SavedTodos {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.items.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SavedTodos {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            items: Vec::<crate::todo::TodoEntry>::deserialize(deserializer)?,
            present: true,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SavedTree {
    Leaf {
        pane: SavedPane,
    },
    Split {
        axis: Axis,
        first: Box<Self>,
        second: Box<Self>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPane {
    pub id: PaneId,
    pub selected_surface: SurfaceId,
    pub surfaces: Vec<SavedSurface>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSurface {
    pub id: SurfaceId,
    pub title: String,
    pub content: SavedContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SavedContent {
    LocalZellijTerminal {
        profile_id: uuid::Uuid,
        owner_uuid: uuid::Uuid,
    },
    LocalTmuxTerminal {
        identity: zmux_remote_runtime::local_multiplexer::LocalTmuxIdentity,
    },
    LocalTerminal {
        cwd: Option<String>,
    },
    SshTerminal {
        connection: SavedSshConnection,
    },
    Browser {
        url: String,
        #[serde(default, skip_serializing_if = "SavedBrowserProfile::is_missing")]
        profile_id: SavedBrowserProfile,
    },
}

/// Presence distinguishes a v1 descriptor from a v2 descriptor without discarding
/// an explicit null or UUID. The wire value remains null or a UUID string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedBrowserProfile {
    pub id: Option<uuid::Uuid>,
    present: bool,
}
impl SavedBrowserProfile {
    fn is_missing(&self) -> bool {
        !self.present
    }
}
impl From<Option<uuid::Uuid>> for SavedBrowserProfile {
    fn from(id: Option<uuid::Uuid>) -> Self {
        Self { id, present: true }
    }
}
impl Serialize for SavedBrowserProfile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SavedBrowserProfile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            id: Option::<uuid::Uuid>::deserialize(deserializer)?,
            present: true,
        })
    }
}

/// Explicit allowlist: startup commands, environment and credential contents have no field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSshConnection {
    pub destination: String,
    pub config_file: Option<String>,
    pub port: Option<u16>,
    pub identity: Option<String>,
    pub forward_agent: Option<bool>,
    pub ssh_options: Vec<String>,
    pub jump_hosts: Vec<JumpConfig>,
}

impl From<&SshConnectionConfig> for SavedSshConnection {
    fn from(value: &SshConnectionConfig) -> Self {
        Self {
            destination: value.destination.clone(),
            config_file: value.config_file.clone(),
            port: value.port,
            identity: value.identity.clone(),
            forward_agent: value.forward_agent,
            ssh_options: value.ssh_options.clone(),
            jump_hosts: value.jump_hosts.clone(),
        }
    }
}
impl From<&SavedSshConnection> for SshConnectionConfig {
    fn from(value: &SavedSshConnection) -> Self {
        Self {
            destination: value.destination.clone(),
            config_file: value.config_file.clone(),
            port: value.port,
            identity: value.identity.clone(),
            forward_agent: value.forward_agent,
            ssh_options: value.ssh_options.clone(),
            jump_hosts: value.jump_hosts.clone(),
            command: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RestoreLimits {
    pub terminals: usize,
    pub browsers: usize,
}

#[derive(Clone, Debug)]
pub struct ValidatedSessionDocument {
    document: SessionDocument,
    terminals: usize,
    browsers: usize,
}
impl ValidatedSessionDocument {
    /// The persistence worker reserves this ID with the fresh runtime IDs.
    pub fn requires_window_id(&self) -> bool {
        self.document.version < 4
    }
    pub fn terminal_count(&self) -> usize {
        self.terminals
    }
    pub fn browser_count(&self) -> usize {
        self.browsers
    }
    pub fn next_id(&self) -> u64 {
        self.document.next_id
    }
    pub fn identity_namespace(&self) -> uuid::Uuid {
        self.document.identity_namespace
    }
}

#[derive(Clone, Debug)]
pub struct PreparedSessionRestore {
    pub store: WorkspaceStore,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid session document: {0}")]
pub struct RestoreError(pub String);
fn invalid(message: impl Into<String>) -> RestoreError {
    RestoreError(message.into())
}

struct Validation {
    ids: HashSet<u64>,
    terminals: usize,
    browsers: usize,
    next_id: u64,
    version: u32,
    limits: RestoreLimits,
}
impl Validation {
    fn id(&mut self, id: u64) -> Result<(), RestoreError> {
        if id == 0 || id >= self.next_id || !self.ids.insert(id) {
            return Err(invalid(
                "object IDs must be nonzero, unique and below next_id",
            ));
        }
        Ok(())
    }
    fn tree(
        &mut self,
        tree: &SavedTree,
        depth: usize,
        panes: &mut Vec<PaneId>,
    ) -> Result<(), RestoreError> {
        if depth > MAX_SPLIT_DEPTH {
            return Err(invalid("split depth limit exceeded"));
        }
        match tree {
            SavedTree::Split { first, second, .. } => {
                self.tree(first, depth + 1, panes)?;
                self.tree(second, depth + 1, panes)?;
            }
            SavedTree::Leaf { pane } => {
                if panes.len() >= MAX_PANES_PER_WORKSPACE {
                    return Err(invalid("pane limit exceeded"));
                }
                self.id(pane.id.0)?;
                panes.push(pane.id);
                if pane.surfaces.is_empty() || pane.surfaces.len() > MAX_SURFACES_PER_PANE {
                    return Err(invalid("pane must contain a bounded nonempty surface list"));
                }
                if !pane.surfaces.iter().any(|s| s.id == pane.selected_surface) {
                    return Err(invalid("selected surface does not belong to pane"));
                }
                for surface in &pane.surfaces {
                    self.id(surface.id.0)?;
                    valid_name(surface.title.clone())
                        .map_err(|_| invalid("invalid surface title"))?;
                    match &surface.content {
                        SavedContent::LocalZellijTerminal {
                            profile_id,
                            owner_uuid,
                        } => {
                            if self.version < 6 {
                                return Err(invalid(
                                    "local Zellij descriptors require session version 6",
                                ));
                            }
                            LocalZellijIdentity {
                                profile_id: *profile_id,
                                owner_uuid: *owner_uuid,
                            }
                            .validate()
                            .map_err(|e| invalid(e.to_string()))?;
                            self.terminals += 1;
                        }
                        SavedContent::LocalTmuxTerminal { identity } => {
                            if self.version < 3 {
                                return Err(invalid(
                                    "local tmux descriptors require session version 3",
                                ));
                            }
                            identity.validate().map_err(|e| invalid(e.to_string()))?;
                            self.terminals += 1;
                        }
                        SavedContent::LocalTerminal { cwd } => {
                            if cwd.as_ref().is_some_and(|cwd| {
                                !cwd.starts_with('/')
                                    || cwd.len() > 4096
                                    || cwd.chars().any(char::is_control)
                            }) {
                                return Err(invalid(
                                    "cwd must be a bounded absolute path without controls",
                                ));
                            }
                            self.terminals += 1;
                        }
                        SavedContent::SshTerminal { connection } => {
                            SshConnectionConfig::from(connection)
                                .validate()
                                .map_err(|e| invalid(format!("invalid SSH connection: {e}")))?;
                            self.terminals += 1;
                        }
                        SavedContent::Browser { url, profile_id } => {
                            if (self.version == 1 && profile_id.present)
                                || (self.version >= 2 && !profile_id.present)
                                || profile_id.id.is_some_and(|id| id.is_nil())
                            {
                                return Err(invalid(
                                    "invalid browser profile field for session version",
                                ));
                            }
                            surfaces::validate_browser_url(url)
                                .map_err(|e| invalid(e.to_string()))?;
                            if url.contains(['?', '#']) {
                                return Err(invalid(
                                    "saved browser URLs must omit query strings and fragments",
                                ));
                            }
                            self.browsers += 1;
                        }
                    }
                    if self.terminals > self.limits.terminals
                        || self.browsers > self.limits.browsers
                    {
                        return Err(invalid("runtime surface capacity exceeded"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl SessionDocument {
    /// Byte-length and parse-recursion limits belong to the persistence worker before this call.
    pub fn validate(self, limits: RestoreLimits) -> Result<ValidatedSessionDocument, RestoreError> {
        if self.format != FORMAT || !matches!(self.version, 1..=SESSION_DOCUMENT_VERSION) {
            return Err(invalid("unsupported format or version"));
        }
        if self.identity_namespace.is_nil() {
            return Err(invalid("identity namespace must be nonzero"));
        }
        if self.next_id == 0 || self.next_id > MAX_NEXT_IDENTIFIER {
            return Err(invalid("allocator space exhausted"));
        }
        if self.workspaces.len() > MAX_WORKSPACES {
            return Err(invalid("workspace limit exceeded"));
        }
        if (self.version < 4 && self.workspaces.is_empty() != self.focused_workspace.is_none())
            || self
                .focused_workspace
                .is_some_and(|id| !self.workspaces.iter().any(|w| w.id == id))
        {
            return Err(invalid("focused workspace does not belong to document"));
        }
        for workspace in &self.workspaces {
            if (self.version >= 5) != workspace.todos.present {
                return Err(invalid(
                    "todo arrays are required in session version 5 and later",
                ));
            }
        }
        crate::todo::validate_lists(
            self.workspaces
                .iter()
                .map(|workspace| workspace.todos.items.as_slice()),
        )
        .map_err(|error| invalid(error.to_string()))?;
        let mut validation = Validation {
            ids: HashSet::new(),
            terminals: 0,
            browsers: 0,
            next_id: self.next_id,
            version: self.version,
            limits,
        };
        for workspace in &self.workspaces {
            validation.id(workspace.id.0)?;
            valid_name(workspace.name.clone()).map_err(|_| invalid("invalid workspace name"))?;
            let mut panes = Vec::new();
            validation.tree(&workspace.root, 0, &mut panes)?;
            if !panes.contains(&workspace.focused_pane) {
                return Err(invalid("focused pane does not belong to workspace"));
            }
        }
        match (&self.window_layout, self.version) {
            (Some(layout), version) if version >= 4 => {
                if layout.windows.is_empty() {
                    return Err(invalid("persisted desktop requires a window"));
                }
                let ids: Vec<_> = self
                    .workspaces
                    .iter()
                    .map(|workspace| workspace.id)
                    .collect();
                let projection =
                    windows::validate_layout(&ids, &layout.windows, layout.focused_window)
                        .map_err(invalid)?;
                for window in &layout.windows {
                    validation.id(window.id.0)?;
                }
                if projection != self.focused_workspace {
                    return Err(invalid("legacy focus differs from window selection"));
                }
            }
            (None, 1..=3) => {}
            _ => {
                return Err(invalid(
                    "window layout is required in session version 4 and later",
                ));
            }
        }
        Ok(ValidatedSessionDocument {
            document: self,
            terminals: validation.terminals,
            browsers: validation.browsers,
        })
    }
}

impl SavedTree {
    fn capture(tree: &PaneTree) -> Self {
        match tree {
            PaneTree::Split {
                axis,
                first,
                second,
            } => Self::Split {
                axis: *axis,
                first: Box::new(Self::capture(first)),
                second: Box::new(Self::capture(second)),
            },
            PaneTree::Leaf { pane } => Self::Leaf {
                pane: SavedPane {
                    id: pane.id,
                    selected_surface: pane.selected_surface,
                    surfaces: pane
                        .surfaces
                        .iter()
                        .map(|surface| SavedSurface {
                            id: surface.id,
                            title: if surface.kind == SurfaceKind::Browser {
                                "Browser".into()
                            } else {
                                surface.title.clone()
                            },
                            content: match surface.kind {
                                SurfaceKind::Terminal if surface.local_zellij.is_some() => {
                                    let identity =
                                        surface.local_zellij.as_ref().expect("checked descriptor");
                                    SavedContent::LocalZellijTerminal {
                                        profile_id: identity.profile_id,
                                        owner_uuid: identity.owner_uuid,
                                    }
                                }
                                SurfaceKind::Terminal if surface.local_tmux.is_some() => {
                                    SavedContent::LocalTmuxTerminal {
                                        identity: surface
                                            .local_tmux
                                            .clone()
                                            .expect("checked descriptor"),
                                    }
                                }
                                SurfaceKind::Terminal => match &surface.remote {
                                    Some(remote) => SavedContent::SshTerminal {
                                        connection: remote.into(),
                                    },
                                    None => SavedContent::LocalTerminal {
                                        cwd: surface.cwd.clone(),
                                    },
                                },
                                SurfaceKind::Browser => SavedContent::Browser {
                                    profile_id: surface.browser_profile_id.into(),
                                    url: surface
                                        .url
                                        .as_deref()
                                        .unwrap_or("about:blank")
                                        .split(['?', '#'])
                                        .next()
                                        .unwrap_or("about:blank")
                                        .into(),
                                },
                            },
                        })
                        .collect(),
                },
            },
        }
    }
    fn restore(
        self,
        ids: &mut impl Iterator<Item = SessionId>,
        effects: &mut Vec<Effect>,
    ) -> PaneTree {
        match self {
            Self::Split {
                axis,
                first,
                second,
            } => PaneTree::Split {
                axis,
                first: Box::new(first.restore(ids, effects)),
                second: Box::new(second.restore(ids, effects)),
            },
            Self::Leaf { pane } => {
                let surfaces = pane
                    .surfaces
                    .into_iter()
                    .map(|saved| match saved.content {
                        SavedContent::Browser { url, profile_id } => Surface {
                            browser_profile_id: profile_id.id,
                            id: saved.id,
                            kind: SurfaceKind::Browser,
                            session_id: None,
                            url: Some(url),
                            navigation: 0,
                            title: saved.title,
                            cwd: None,
                            remote: None,
                            local_tmux: None,
                            local_zellij: None,
                        },
                        content => {
                            let session_id = ids.next().expect("validated injected session count");
                            let (cwd, remote, local_tmux, local_zellij) = match content {
                                SavedContent::LocalZellijTerminal {
                                    profile_id,
                                    owner_uuid,
                                } => (
                                    None,
                                    None,
                                    None,
                                    Some(LocalZellijIdentity {
                                        profile_id,
                                        owner_uuid,
                                    }),
                                ),
                                SavedContent::LocalTmuxTerminal { identity } => {
                                    (None, None, Some(identity), None)
                                }
                                SavedContent::LocalTerminal { cwd } => (cwd, None, None, None),
                                SavedContent::SshTerminal { connection } => (
                                    None,
                                    Some(SshConnectionConfig::from(&connection)),
                                    None,
                                    None,
                                ),
                                _ => unreachable!(),
                            };
                            if let Some(identity) = &local_zellij {
                                effects.push(Effect::AttachLocalZellijSession {
                                    pane_id: pane.id,
                                    session_id,
                                    identity: identity.clone(),
                                });
                            } else {
                                effects.push(Effect::StartSession {
                                    pane_id: pane.id,
                                    session_id,
                                    cwd: cwd.clone(),
                                    remote: remote.clone(),
                                    local_tmux: local_tmux.clone().map(Box::new),
                                });
                            }
                            let mut surface = Surface::terminal(saved.id, session_id);
                            surface.title = saved.title;
                            surface.cwd = cwd;
                            surface.remote = remote;
                            surface.local_tmux = local_tmux;
                            surface.local_zellij = local_zellij;
                            surface
                        }
                    })
                    .collect();
                PaneTree::Leaf {
                    pane: Pane {
                        id: pane.id,
                        selected_surface: pane.selected_surface,
                        surfaces,
                    },
                }
            }
        }
    }
}

impl WorkspaceStore {
    /// Runtime allocation reservations are durable; the pure core only exposes the
    /// next unused ID and can skip forward into a newly reserved block.
    pub fn next_identifier(&self) -> u64 {
        self.next_id
    }

    pub fn advance_identifier_floor(&mut self, floor: u64) -> Result<(), RestoreError> {
        if floor == 0 || floor > MAX_NEXT_IDENTIFIER {
            return Err(invalid(
                "allocator floor is outside supported identifier space",
            ));
        }
        self.next_id = self.next_id.max(floor);
        Ok(())
    }

    /// Replacing a live desktop model must invalidate every earlier revision guard.
    /// Document revisions are deliberately not trusted or imported from disk.
    pub fn advance_restore_revision(&mut self, previous_revision: u64) -> Result<(), RestoreError> {
        self.state.revision = self
            .state
            .revision
            .max(previous_revision)
            .checked_add(1)
            .ok_or_else(|| invalid("model revision space exhausted"))?;
        Ok(())
    }

    pub fn session_document(&self) -> Result<SessionDocument, RestoreError> {
        self.validate_terminal_descriptors()
            .map_err(|e| invalid(e.to_string()))?;
        let document = SessionDocument {
            format: FORMAT.into(),
            version: SESSION_DOCUMENT_VERSION,
            identity_namespace: self.state.identity_namespace,
            next_id: self.next_id,
            focused_workspace: self.state.focused_workspace,
            window_layout: Some(SavedWindowLayout {
                windows: self.state.windows.clone(),
                focused_window: self.state.focused_window,
            }),
            workspaces: self
                .state
                .workspaces
                .iter()
                .map(|workspace| SavedWorkspace {
                    id: workspace.id,
                    name: workspace.name.clone(),
                    focused_pane: workspace.focused_pane,
                    root: SavedTree::capture(&workspace.root),
                    todos: workspace.todos.clone().into(),
                })
                .collect(),
        };
        document.clone().validate(RestoreLimits {
            terminals: usize::MAX,
            browsers: usize::MAX,
        })?;
        Ok(document)
    }

    /// Runtime must durably reserve these IDs before launching. The imported source file is
    /// never its reservation ledger; repeated imports must not reuse a prior runtime identity.
    pub fn prepare_session_restore(
        document: ValidatedSessionDocument,
        fresh_session_ids: &[SessionId],
        fresh_window_id: Option<WindowId>,
    ) -> Result<PreparedSessionRestore, RestoreError> {
        if fresh_session_ids.len() != document.terminals {
            return Err(invalid("wrong number of runtime session IDs"));
        }
        let mut unique = HashSet::new();
        let mut next_id = document.document.next_id;
        for id in fresh_session_ids {
            if id.0 < document.document.next_id
                || id.0 >= MAX_NEXT_IDENTIFIER
                || !unique.insert(id.0)
            {
                return Err(invalid(
                    "runtime session IDs must be fresh, unique and leave allocator capacity",
                ));
            }
            next_id = next_id.max(id.0 + 1);
        }
        let layout = if document.requires_window_id() {
            let id = fresh_window_id
                .ok_or_else(|| invalid("legacy restore requires a reserved window ID"))?;
            if id.0 < document.document.next_id
                || id.0 >= MAX_NEXT_IDENTIFIER
                || !unique.insert(id.0)
            {
                return Err(invalid(
                    "window ID must be fresh, unique and leave allocator capacity",
                ));
            }
            next_id = next_id.max(id.0 + 1);
            SavedWindowLayout {
                windows: vec![Window {
                    id,
                    workspace_ids: document
                        .document
                        .workspaces
                        .iter()
                        .map(|workspace| workspace.id)
                        .collect(),
                    selected_workspace: document.document.focused_workspace,
                }],
                focused_window: Some(id),
            }
        } else {
            if fresh_window_id.is_some() {
                return Err(invalid(
                    "version 4 and later restore preserves saved window IDs",
                ));
            }
            document
                .document
                .window_layout
                .clone()
                .expect("validated window layout")
        };
        let mut ids = fresh_session_ids.iter().copied();
        let mut effects = Vec::new();
        let mut source = document.document;
        // The singleton native host still presents the flat catalog; project its
        // order from canonical membership rather than ignoring saved window order.
        if let Some(window) = layout.windows.first() {
            source.workspaces.sort_by_key(|workspace| {
                window
                    .workspace_ids
                    .iter()
                    .position(|id| *id == workspace.id)
            });
        }
        let state = Snapshot {
            identity_namespace: source.identity_namespace,
            model_version: MODEL_VERSION,
            revision: 0,
            focused_workspace: source.focused_workspace,
            closed_items: Vec::new(),
            windows: layout.windows,
            focused_window: layout.focused_window,
            workspaces: source
                .workspaces
                .into_iter()
                .map(|workspace| Workspace {
                    id: workspace.id,
                    name: workspace.name,
                    focused_pane: workspace.focused_pane,
                    root: workspace.root.restore(&mut ids, &mut effects),
                    todos: workspace.todos.items,
                })
                .collect(),
        };
        let mut store = Self {
            state,
            next_id,
            ..Self::default()
        };
        store.project_window_focus();
        store
            .check_window_layout()
            .map_err(|error| invalid(error.to_string()))?;
        store
            .validate_todos()
            .map_err(|error| invalid(error.to_string()))?;
        Ok(PreparedSessionRestore { store, effects })
    }
}
