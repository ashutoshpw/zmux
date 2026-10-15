//! Owner-side wire validation, bounded retention and workspace lifecycle contracts.
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{
    Command, Desktop, Request, Shell, Snapshot,
    metadata::{LogLevel, MAX_LOG_ENTRIES, MAX_STATUS_ENTRIES, MAX_TEXT_BYTES, MetadataAction},
    targets::{TargetOperation, TargetRequest},
};
use zmux_workspace_core::{
    Action, WorkspaceId,
    targets::{CallerContext, TargetScope},
};
struct App {
    desktop: Desktop,
    next: u64,
}
impl App {
    fn new() -> Self {
        let app = Self {
            desktop: Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: None,
            })
            .unwrap(),
            next: 0,
        };
        app.wait(|s| !s.model.workspaces.is_empty());
        app
    }
    fn wait(&self, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(s) = self.desktop.snapshot()
                && predicate(&s)
            {
                return s;
            }
            assert!(
                Instant::now() < deadline,
                "desktop metadata command did not settle"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn send(&mut self, payload: Command) -> Arc<Snapshot> {
        self.next += 1;
        self.desktop
            .submit(Request {
                version: 1,
                request_id: self.next,
                payload,
            })
            .unwrap();
        self.wait(|s| s.last_request_id == self.next)
    }
    fn metadata(&mut self, w: WorkspaceId, action: MetadataAction) -> Arc<Snapshot> {
        self.send(Command::Targeted {
            request: TargetRequest {
                scope: TargetScope {
                    workspace: Some(w.0.to_string().parse().unwrap()),
                    ..Default::default()
                },
                operation: TargetOperation::Metadata { action },
                ..Default::default()
            },
        })
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}
fn status(key: String, value: &str, priority: i32) -> MetadataAction {
    MetadataAction::SetStatus {
        key,
        value: value.into(),
        priority,
        icon: None,
        color: None,
    }
}
#[test]
fn owner_bounds_status_and_log_records_without_losing_existing_keys_or_newest_messages() {
    let mut a = App::new();
    let w = a.desktop.snapshot().unwrap().model.workspaces[0].id;
    for n in 0..MAX_STATUS_ENTRIES {
        let s = a.metadata(w, status(format!("key-{n:02}"), "original", 0));
        assert!(s.last_error.is_none(), "{:?}", s.last_error);
    }
    let before = a.desktop.snapshot().unwrap();
    let rejected = a.metadata(w, status("overflow".into(), "rejected", 100));
    assert!(rejected.last_error.as_ref().unwrap().contains("capacity"));
    assert_eq!(
        serde_json::to_value(&before.workspace_metadata).unwrap(),
        serde_json::to_value(&rejected.workspace_metadata).unwrap()
    );
    let replaced = a.metadata(w, status("key-01".into(), "replaced at capacity", 1));
    assert!(replaced.last_error.is_none());
    let rows = &replaced.workspace_metadata[0].statuses;
    assert_eq!(rows.len(), MAX_STATUS_ENTRIES);
    assert_eq!(rows[0].key, "key-01");
    assert_eq!(rows[1].key, "key-00");
    for n in 0..MAX_LOG_ENTRIES + 3 {
        let s = a.metadata(
            w,
            MetadataAction::Log {
                message: format!("message-{n}"),
                level: LogLevel::Info,
                source: None,
            },
        );
        assert!(s.last_error.is_none());
    }
    let s = a.desktop.snapshot().unwrap();
    let metadata = &s.workspace_metadata[0];
    assert_eq!(metadata.logs.len(), MAX_LOG_ENTRIES);
    assert_eq!(metadata.logs_dropped, 3);
    assert_eq!(metadata.logs[0].message, "message-3");
    assert_eq!(
        metadata.logs.last().unwrap().message,
        format!("message-{}", MAX_LOG_ENTRIES + 2)
    );
    assert!(
        metadata
            .logs
            .windows(2)
            .all(|p| p[0].sequence < p[1].sequence)
    );
    let rejected = a.metadata(
        w,
        MetadataAction::Log {
            message: "x".repeat(MAX_TEXT_BYTES + 1),
            level: LogLevel::Info,
            source: None,
        },
    );
    assert!(rejected.last_error.is_some());
    assert_eq!(
        serde_json::to_value(&s.workspace_metadata).unwrap(),
        serde_json::to_value(&rejected.workspace_metadata).unwrap()
    );
}
#[test]
fn wire_targets_reject_scope_mismatch_and_stale_revision_and_close_prunes_owned_state() {
    let mut a = App::new();
    let w = a.desktop.snapshot().unwrap().model.workspaces[0].id;
    a.metadata(w, status("build".into(), "retained", 0));
    let other = a.send(Command::Action {
        action: Action::CreateWorkspace {
            name: "other".into(),
        },
    });
    let foreign = other
        .model
        .workspaces
        .iter()
        .find(|item| item.id != w)
        .unwrap();
    let surface = foreign.root.panes()[0].selected_surface;
    let before = serde_json::to_value(&other.workspace_metadata).unwrap();
    for request in [
        TargetRequest {
            scope: TargetScope {
                workspace: Some(w.0.to_string().parse().unwrap()),
                surface: Some(surface.0.to_string().parse().unwrap()),
                ..Default::default()
            },
            operation: TargetOperation::Metadata {
                action: MetadataAction::ClearStatus {
                    key: "build".into(),
                },
            },
            ..Default::default()
        },
        TargetRequest {
            scope: TargetScope {
                workspace: Some(w.0.to_string().parse().unwrap()),
                ..Default::default()
            },
            expected_model_revision: Some(other.model.revision + 1),
            operation: TargetOperation::Metadata {
                action: MetadataAction::ClearStatus {
                    key: "build".into(),
                },
            },
            caller: CallerContext::default(),
        },
    ] {
        let rejected = a.send(Command::Targeted { request });
        assert!(rejected.last_error.is_some());
        assert_eq!(
            serde_json::to_value(&rejected.workspace_metadata).unwrap(),
            before
        );
    }
    let closed = a.send(Command::Action {
        action: Action::CloseWorkspace { workspace_id: w },
    });
    assert!(closed.last_error.is_none());
    assert!(closed.workspace_metadata.is_empty());
    let next = a.send(Command::Action {
        action: Action::CreateWorkspace {
            name: "replacement".into(),
        },
    });
    assert!(next.last_error.is_none());
    assert!(next.workspace_metadata.is_empty());
}

#[test]
fn close_then_restore_does_not_resurrect_transient_metadata() {
    use std::{fs, os::unix::fs::PermissionsExt};
    use zmux_desktop_runtime::sessions::{OperationState, SessionCommand};
    use zmux_workspace_core::WorkspaceStore;
    let directory = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("zmux-metadata-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "restored workspace".into(),
        })
        .unwrap();
    let path = directory.join("import.json");
    fs::write(
        &path,
        serde_json::to_vec(&store.session_document().unwrap()).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    {
        let mut a = App {
            desktop: Desktop::with_persistence(
                Shell {
                    program: "/bin/sh".into(),
                    args: vec!["-i".into()],
                    cwd: None,
                },
                directory.join("state"),
            )
            .unwrap(),
            next: 0,
        };
        let initial = a.wait(|s| {
            !s.persistence.startup_pending
                && !s.model.workspaces.is_empty()
                && s.pending_launches == 0
        });
        let namespace = initial.model.identity_namespace;
        let w = initial.model.workspaces[0].id;
        a.metadata(w, status("ephemeral".into(), "do not restore", 0));
        a.send(Command::Action {
            action: Action::CloseWorkspace { workspace_id: w },
        });
        a.wait(|s| s.pending_launches == 0 && s.model.workspaces.is_empty());
        let admission = a.send(Command::Session {
            action: SessionCommand::Restore { path: Some(path) },
        });
        assert!(admission.last_error.is_none(), "{:?}", admission.last_error);
        let restored = a.wait(|s| {
            s.persistence
                .operations
                .iter()
                .any(|o| o.kind == "restore" && !matches!(o.state, OperationState::Pending))
        });
        assert_ne!(restored.model.identity_namespace, namespace);
        assert_eq!(
            restored.model.identity_namespace,
            store.snapshot().identity_namespace
        );
        assert!(!restored.model.workspaces.is_empty());
        assert!(restored.workspace_metadata.is_empty());
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn process_byte_capacity_is_shared_across_workspaces_and_released_without_cross_owner_eviction() {
    use zmux_desktop_runtime::metadata::{MAX_LOG_MESSAGE_BYTES, MAX_RETAINED_BYTES};
    let mut a = App::new();
    let first = a.desktop.snapshot().unwrap().model.workspaces[0].id;
    let created = a.send(Command::Action {
        action: Action::CreateWorkspace {
            name: "second metadata owner".into(),
        },
    });
    let second = created.model.focused_workspace.unwrap();
    let text = "\\".repeat(MAX_TEXT_BYTES);
    for workspace in [first, second] {
        for key in 0..16 {
            let s = a.metadata(workspace, status(format!("key-{key}"), &text, 0));
            assert!(s.last_error.is_none());
        }
    }
    let mut rejected = false;
    for n in 0..256 {
        let before = a.desktop.snapshot().unwrap();
        let workspace = if n % 2 == 0 { first } else { second };
        let after = a.metadata(
            workspace,
            MetadataAction::Log {
                message: "\\".repeat(MAX_LOG_MESSAGE_BYTES),
                level: LogLevel::Info,
                source: None,
            },
        );
        assert!(serde_json::to_vec(&after.workspace_metadata).unwrap().len() <= MAX_RETAINED_BYTES);
        if let Some(error) = &after.last_error {
            assert!(error.contains("byte capacity"), "{error}");
            assert_eq!(
                serde_json::to_value(&before.workspace_metadata).unwrap(),
                serde_json::to_value(&after.workspace_metadata).unwrap()
            );
            assert!(
                after
                    .workspace_metadata
                    .iter()
                    .all(|w| w.statuses.len() == 16 && w.logs.len() < MAX_LOG_ENTRIES)
            );
            rejected = true;
            break;
        }
    }
    assert!(rejected, "aggregate capacity was never enforced");
    let cleared = a.metadata(first, MetadataAction::ClearLog);
    assert!(cleared.last_error.is_none());
    let foreign_before = serde_json::to_value(
        cleared
            .workspace_metadata
            .iter()
            .find(|w| w.workspace_id == second)
            .unwrap(),
    )
    .unwrap();
    let admitted = a.metadata(
        first,
        MetadataAction::Log {
            message: "fits after clear".into(),
            level: LogLevel::Success,
            source: None,
        },
    );
    assert!(admitted.last_error.is_none());
    assert_eq!(
        serde_json::to_value(
            admitted
                .workspace_metadata
                .iter()
                .find(|w| w.workspace_id == second)
                .unwrap()
        )
        .unwrap(),
        foreign_before
    );
}
