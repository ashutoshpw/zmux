//! Real filesystem + PTY lifecycle contracts; pure descriptor validation lives in core.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{
    Command, Desktop, DesktopError, Request, Shell, Snapshot,
    sessions::{OperationState, SessionCommand},
};
use zmux_workspace_core::{Action, Axis, WorkspaceStore};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-session-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn write(&self, bytes: &[u8]) {
        fs::write(self.0.join("session.json"), bytes).unwrap();
        fs::set_permissions(
            self.0.join("session.json"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    fn desktop(&self) -> Desktop {
        Desktop::with_persistence(shell(), self.0.clone()).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn shell() -> Shell {
    Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    }
}
fn fixture() -> WorkspaceStore {
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "Recovered project".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Vertical,
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::CreateBrowser {
            profile_id: None,
            focus: true,
            pane_id: pane,
            url: "https://example.org/path?token=private#fragment".into(),
        })
        .unwrap();
    store
}
fn send(desktop: &Desktop, request_id: u64, payload: Command) {
    desktop
        .submit(Request {
            version: 1,
            request_id,
            payload,
        })
        .unwrap();
}
#[track_caller]
fn wait(desktop: &Desktop, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match desktop.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(_) | Err(DesktopError::Busy) => {}
            Err(error) => panic!("{error}"),
        }
        assert!(
            Instant::now() < deadline,
            "session lifecycle timed out: {}",
            serde_json::to_string(&*desktop.snapshot().unwrap()).unwrap()
        );
        thread::sleep(Duration::from_millis(3));
    }
}
fn ready(snapshot: &Snapshot) -> bool {
    !snapshot.persistence.startup_pending
        && snapshot.pending_launches == 0
        && !snapshot.model.workspaces.is_empty()
        && snapshot.control_frames.iter().all(|f| f.snapshot.is_some())
}

#[test]
fn moved_membership_and_empty_source_persist_without_replacing_live_sessions() {
    use zmux_desktop_runtime::windows::{
        NativeWindowEvent, WindowAction, WindowHostToken, WindowRequest,
    };
    let directory = Directory::new();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let moving = initial.model.workspaces[0].clone();
    send(
        &desktop,
        1,
        Command::Windows {
            request: WindowRequest {
                scope: Default::default(),
                caller: Default::default(),
                expected_model_revision: None,
                action: WindowAction::New { focus: false },
            },
        },
    );
    let two = wait(&desktop, |s| {
        s.last_request_id == 1
            && s.control_frames.len() == 2
            && s.control_frames.iter().all(|f| f.snapshot.is_some())
    });
    for request in &two.window_requests {
        desktop
            .native_window_event(NativeWindowEvent::Complete {
                token: WindowHostToken {
                    epoch: request.epoch,
                    identity_namespace: request.identity_namespace,
                    window_id: request.window_id,
                    host_id: uuid::Uuid::new_v4(),
                },
                operation_id: request.operation_id,
            })
            .unwrap();
    }
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::MoveWorkspaceToWindow {
                workspace_id: moving.id,
                destination_window_id: two.model.windows[1].id,
                index: None,
                focus: false,
            },
        },
    );
    let moved = wait(&desktop, |s| s.last_request_id == 2);
    assert!(moved.model.windows[0].workspace_ids.is_empty());
    assert_eq!(
        moved
            .control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>(),
        two.control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>()
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let document: zmux_workspace_core::SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        document.window_layout.as_ref().unwrap().windows,
        moved.model.windows
    );
    let reopened = directory.desktop();
    let restored = wait(&reopened, ready);
    assert_eq!(restored.model.windows, moved.model.windows);
    assert_eq!(restored.model.focused_window, moved.model.focused_window);
    assert_eq!(restored.model.focused_workspace, None);
    assert_eq!(
        restored.model.identity_namespace,
        moved.model.identity_namespace
    );
    let workspace = restored
        .model
        .workspaces
        .iter()
        .find(|w| w.id == moving.id)
        .unwrap();
    assert_eq!(workspace.root.panes()[0].id, moving.root.panes()[0].id);
    assert_eq!(
        workspace.root.panes()[0].selected_surface,
        moving.root.panes()[0].selected_surface
    );
    assert_ne!(
        workspace.root.panes()[0].session_id(),
        moving.root.panes()[0].session_id()
    );
    reopened.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn empty_focused_window_roundtrip_and_last_close_save_retains_the_terminal_owner() {
    use zmux_desktop_runtime::windows::{NativeWindowEvent, WindowAction, WindowRequest};
    use zmux_workspace_core::targets::TargetScope;

    let directory = Directory::new();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    send(
        &desktop,
        1,
        Command::Windows {
            request: WindowRequest {
                action: WindowAction::New { focus: true },
                scope: TargetScope::default(),
                caller: Default::default(),
                expected_model_revision: None,
            },
        },
    );
    let two = wait(&desktop, |s| {
        s.last_request_id == 1 && s.pending_launches == 0
    });
    let empty_window = two.model.windows[1].id;
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: two.model.windows[1].selected_workspace.unwrap(),
            },
        },
    );
    let empty = wait(&desktop, |s| {
        s.last_request_id == 2 && s.control_frames.len() == 1
    });
    assert_eq!(empty.model.focused_window, Some(empty_window));
    assert_eq!(empty.model.focused_workspace, None);
    desktop.shutdown(Duration::from_secs(5)).unwrap();

    let restarted = directory.desktop();
    let restored = wait(&restarted, ready);
    assert_eq!(restored.model.windows, empty.model.windows);
    assert_eq!(restored.model.focused_window, Some(empty_window));
    assert_eq!(restored.model.focused_workspace, None);
    assert_eq!(
        restored.model.identity_namespace,
        initial.model.identity_namespace
    );
    let close = |window| Command::Windows {
        request: WindowRequest {
            scope: TargetScope {
                window: Some(format!("window:{}", window).parse().unwrap()),
                ..Default::default()
            },
            action: WindowAction::Close,
            caller: Default::default(),
            expected_model_revision: None,
        },
    };
    send(&restarted, 1, close(empty_window.0));
    wait(&restarted, |s| s.last_request_id == 1);
    let retained_window = restored.model.windows[0].id;
    send(&restarted, 2, close(retained_window.0));
    let last = wait(&restarted, |s| s.last_request_id == 2);
    assert_eq!(last.model.windows.len(), 1);
    assert_eq!(last.model.workspaces.len(), 1);
    assert_eq!(last.control_frames.len(), 1);
    let termination = last
        .window_requests
        .iter()
        .find(|r| r.kind == zmux_desktop_runtime::windows::WindowKind::Terminate)
        .unwrap();
    restarted
        .native_window_event(NativeWindowEvent::Complete {
            token: zmux_desktop_runtime::windows::WindowHostToken {
                epoch: termination.epoch,
                identity_namespace: termination.identity_namespace,
                window_id: termination.window_id,
                host_id: uuid::Uuid::new_v4(),
            },
            operation_id: termination.operation_id,
        })
        .unwrap();
    restarted.shutdown(Duration::from_secs(5)).unwrap();
    let disk: zmux_workspace_core::SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        disk.window_layout.as_ref().unwrap().windows,
        last.model.windows
    );
    assert_eq!(disk.workspaces[0].id, last.model.workspaces[0].id);
    let reopened = directory.desktop();
    let live = wait(&reopened, ready);
    assert_eq!(live.model.windows, last.model.windows);
    assert_eq!(live.model.identity_namespace, last.model.identity_namespace);
    assert_ne!(
        live.control_frames[0].session_id,
        last.control_frames[0].session_id
    );
    assert!(live.control_frames[0].process_id.is_some());
    reopened.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn startup_restores_topology_fresh_shells_and_shutdown_flushes_latest_committed_change() {
    let directory = Directory::new();
    let store = fixture();
    let source = store.snapshot();
    directory.write(&serde_json::to_vec(&store.session_document().unwrap()).unwrap());
    let desktop = directory.desktop();
    let restored = wait(&desktop, ready);
    assert_eq!(restored.model.identity_namespace, source.identity_namespace);
    assert_eq!(
        restored.model.workspaces,
        source
            .workspaces
            .iter()
            .cloned()
            .map(|mut w| {
                // Runtime sessions are fresh; topology identities stay stable.
                fn update(
                    tree: &mut zmux_workspace_core::PaneTree,
                    restored: &zmux_workspace_core::PaneTree,
                ) {
                    match (tree, restored) {
                        (
                            zmux_workspace_core::PaneTree::Leaf { pane },
                            zmux_workspace_core::PaneTree::Leaf { pane: other },
                        ) => {
                            for (surface, other) in pane.surfaces.iter_mut().zip(&other.surfaces) {
                                surface.session_id = other.session_id;
                                surface.cwd = other.cwd.clone();
                                if surface.kind == zmux_workspace_core::SurfaceKind::Browser {
                                    surface.url = Some("https://example.org/path".into());
                                }
                            }
                        }
                        (
                            zmux_workspace_core::PaneTree::Split { first, second, .. },
                            zmux_workspace_core::PaneTree::Split {
                                first: f,
                                second: s,
                                ..
                            },
                        ) => {
                            update(first, f);
                            update(second, s);
                        }
                        _ => panic!("restore changed split layout"),
                    }
                }
                update(&mut w.root, &restored.model.workspaces[0].root);
                w
            })
            .collect::<Vec<_>>()
    );
    let first_sessions: Vec<_> = restored
        .control_frames
        .iter()
        .map(|f| f.session_id)
        .collect();
    assert_eq!(first_sessions.len(), 2);
    let old_sessions: Vec<_> = source
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| &p.surfaces)
        .filter_map(|s| s.session_id)
        .collect();
    assert!(first_sessions.iter().all(|id| !old_sessions.contains(id)));
    send(
        &desktop,
        10,
        Command::Input {
            session_id: first_sessions[0],
            text: "printf '\\nINPUT_MUST_NOT_REPLAY\\n'\n".into(),
        },
    );
    wait(&desktop, |s| {
        s.control_frames.iter().any(|f| {
            f.snapshot.as_ref().is_some_and(|v| {
                v.viewport
                    .contents
                    .lines()
                    .any(|l| l == "INPUT_MUST_NOT_REPLAY")
            })
        })
    });
    send(
        &desktop,
        11,
        Command::Action {
            action: Action::RenameWorkspace {
                workspace_id: source.workspaces[0].id,
                name: "Latest committed name".into(),
            },
        },
    );
    wait(&desktop, |s| s.last_request_id == 11);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let bytes = fs::read(directory.0.join("session.json")).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("Latest committed name"));
    assert!(!text.contains("INPUT_MUST_NOT_REPLAY"));
    assert!(!text.contains("token="));
    let restarted = directory.desktop();
    let current = wait(&restarted, ready);
    assert_eq!(current.model.workspaces[0].name, "Latest committed name");
    assert!(
        current
            .control_frames
            .iter()
            .all(|f| !first_sessions.contains(&f.session_id))
    );
    assert!(current.control_frames.iter().all(|f| {
        !f.snapshot
            .as_ref()
            .unwrap()
            .viewport
            .contents
            .contains("INPUT_MUST_NOT_REPLAY")
    }));
    restarted.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn corrupt_unsafe_versioned_and_overcapacity_startup_files_are_retained_and_protected() {
    let store = fixture();
    let document = serde_json::to_value(store.session_document().unwrap()).unwrap();
    let mut future = document.clone();
    future["version"] = 999.into();
    let mut oversized = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    for i in 0..9 {
        oversized
            .apply(Action::CreateWorkspace {
                name: format!("Workspace {i}"),
            })
            .unwrap();
    }
    for (label, bytes, mode) in [
        ("corrupt", b"{incomplete".to_vec(), 0o600),
        ("future", serde_json::to_vec(&future).unwrap(), 0o600),
        ("unsafe", serde_json::to_vec(&document).unwrap(), 0o644),
        (
            "capacity",
            serde_json::to_vec(&oversized.session_document().unwrap()).unwrap(),
            0o600,
        ),
    ] {
        let directory = Directory::new();
        directory.write(&bytes);
        fs::set_permissions(
            directory.0.join("session.json"),
            fs::Permissions::from_mode(mode),
        )
        .unwrap();
        let desktop = directory.desktop();
        let snapshot = wait(&desktop, ready);
        assert!(!snapshot.persistence.autosave_enabled, "{label}");
        assert!(snapshot.persistence.error.is_some(), "{label}");
        assert_eq!(snapshot.model.workspaces[0].name, "Terminal");
        send(
            &desktop,
            2,
            Command::Attention {
                action: zmux_desktop_runtime::attention::AttentionCommand::AppActive {
                    active: true,
                },
            },
        );
        let snapshot = wait(&desktop, |s| s.last_request_id == 2);
        assert!(
            snapshot.persistence.error.is_some(),
            "successful commands must not clear {label} error"
        );
        desktop.shutdown(Duration::from_secs(5)).unwrap();
        assert_eq!(
            fs::read(directory.0.join("session.json")).unwrap(),
            bytes,
            "{label}"
        );
    }
}

#[test]
fn failed_startup_launch_keeps_original_file_and_never_commits_candidate() {
    let directory = Directory::new();
    let mut store = fixture();
    let source = store.snapshot();
    let session = source.workspaces[0].root.panes()[0].session_id().unwrap();
    store.set_session_cwd(
        session,
        directory.0.join("missing-cwd").to_string_lossy().into(),
    );
    let bytes = serde_json::to_vec(&store.session_document().unwrap()).unwrap();
    directory.write(&bytes);
    let desktop = directory.desktop();
    let snapshot = wait(&desktop, |s| ready(s) && s.persistence.error.is_some());
    assert_ne!(snapshot.model.identity_namespace, source.identity_namespace);
    assert_eq!(snapshot.model.workspaces[0].name, "Terminal");
    assert!(!snapshot.persistence.autosave_enabled);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert_eq!(fs::read(directory.0.join("session.json")).unwrap(), bytes);
}

#[test]
fn explicit_exports_report_actual_completion_failures_and_preserve_receipts_across_autosaves() {
    let directory = Directory::new();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let exported = directory.0.join("export.json");
    send(
        &desktop,
        1,
        Command::Session {
            action: SessionCommand::Export {
                path: exported.clone(),
                force: false,
            },
        },
    );
    let completed = wait(&desktop, |s| {
        s.persistence
            .operations
            .first()
            .is_some_and(|o| matches!(o.state, OperationState::Completed))
    });
    let operation_id = completed.persistence.operations[0].operation_id;
    assert!(!fs::read(&exported).unwrap().is_empty());
    send(
        &desktop,
        2,
        Command::Session {
            action: SessionCommand::Export {
                path: exported.clone(),
                force: false,
            },
        },
    );
    let failed = wait(&desktop, |s| {
        s.persistence.operations.len() == 2
            && matches!(s.persistence.operations[1].state, OperationState::Failed)
    });
    assert!(
        failed.persistence.operations[1]
            .error
            .as_ref()
            .unwrap()
            .contains("exists")
    );
    send(
        &desktop,
        3,
        Command::Action {
            action: Action::RenameWorkspace {
                workspace_id: initial.model.workspaces[0].id,
                name: "Autosave is separate".into(),
            },
        },
    );
    let changed = wait(&desktop, |s| {
        s.last_request_id == 3 && s.persistence.last_saved_revision == Some(s.model.revision)
    });
    assert_eq!(changed.persistence.operations[0].operation_id, operation_id);
    assert!(matches!(
        changed.persistence.operations[0].state,
        OperationState::Completed
    ));
    assert!(matches!(
        changed.persistence.operations[1].state,
        OperationState::Failed
    ));
    // A live model cannot be replaced by an import.
    send(
        &desktop,
        4,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(exported.clone()),
            },
        },
    );
    let rejected = wait(&desktop, |s| s.last_request_id == 4);
    assert!(
        rejected
            .last_error
            .as_ref()
            .unwrap()
            .contains("empty desktop")
    );
    assert_eq!(rejected.model.workspaces[0].name, "Autosave is separate");
    send(
        &desktop,
        5,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: initial.model.workspaces[0].id,
            },
        },
    );
    wait(&desktop, |s| {
        s.last_request_id == 5 && s.pending_launches == 0
    });
    send(
        &desktop,
        6,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(exported.clone()),
            },
        },
    );
    let restored = wait(&desktop, |s| {
        s.last_request_id == 6
            && s.persistence.operations.len() == 3
            && matches!(s.persistence.operations[2].state, OperationState::Completed)
    });
    assert_eq!(restored.model.workspaces[0].name, "Terminal");
    for request_id in 10..43 {
        send(
            &desktop,
            request_id,
            Command::Session {
                action: SessionCommand::Export {
                    path: exported.clone(),
                    force: false,
                },
            },
        );
        wait(&desktop, |s| {
            s.last_request_id == request_id
                && s.persistence
                    .operations
                    .last()
                    .is_some_and(|op| matches!(op.state, OperationState::Failed))
        });
    }
    let bounded = desktop.snapshot().unwrap();
    assert_eq!(bounded.persistence.operations.len(), 32);
    assert!(
        !bounded
            .persistence
            .operations
            .iter()
            .any(|op| op.operation_id == operation_id),
        "old completed receipts must expire"
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn reimport_burns_unsaved_ordinary_launch_generations_and_invalidates_earlier_revision_guards() {
    let directory = Directory::new();
    let original = fixture().session_document().unwrap();
    let bytes = serde_json::to_vec(&original).unwrap();
    directory.write(&bytes);
    let imported = directory.0.join("immutable-import.json");
    fs::write(&imported, &bytes).unwrap();
    fs::set_permissions(&imported, fs::Permissions::from_mode(0o600)).unwrap();
    let desktop = directory.desktop();
    let restored = wait(&desktop, ready);
    let first_revision = restored.model.revision;
    let workspace = restored.model.workspaces[0].id;
    let pane = restored.model.workspaces[0].focused_pane;
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::CreateSurface { pane_id: pane },
        },
    );
    let ordinary = wait(&desktop, |s| {
        ready(s) && s.last_request_id == 1 && s.control_frames.len() == 3
    });
    let previous_generations: Vec<_> = ordinary
        .control_frames
        .iter()
        .map(|frame| frame.session_id)
        .collect();
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: workspace,
            },
        },
    );
    let empty = wait(&desktop, |s| {
        s.last_request_id == 2 && s.model.workspaces.is_empty()
    });
    send(
        &desktop,
        3,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(imported.clone()),
            },
        },
    );
    let recovered = wait(&desktop, |s| {
        ready(s)
            && s.last_request_id == 3
            && s.persistence
                .operations
                .last()
                .is_some_and(|op| matches!(op.state, OperationState::Completed))
    });
    assert!(
        recovered.model.revision > empty.model.revision,
        "restore must advance the executor's revision instead of importing revision zero"
    );
    assert!(
        recovered
            .control_frames
            .iter()
            .all(|frame| !previous_generations.contains(&frame.session_id)),
        "an ordinary launch after the previous import must never have its generation reused"
    );
    let stale = zmux_desktop_runtime::targets::TargetRequest {
        expected_model_revision: Some(first_revision),
        ..Default::default()
    };
    assert!(stale.validate_revision(&recovered.model).is_err());
    assert_eq!(
        fs::read(imported).unwrap(),
        bytes,
        "the original import remains immutable across runtime allocations"
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn browser_only_runtime_refills_durable_allocation_blocks_before_crossing_their_bounds() {
    use zmux_desktop_runtime::targets::{TargetOperation, TargetRequest};
    use zmux_workspace_core::targets::TargetAction;
    let directory = Directory::new();
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "Browser only".into(),
        })
        .unwrap();
    let original = store.snapshot();
    let pane = original.workspaces[0].focused_pane;
    let terminal = original.workspaces[0].root.panes()[0].selected_surface;
    store
        .apply(Action::CreateBrowser {
            profile_id: None,
            focus: true,
            pane_id: pane,
            url: "https://example.org/".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface {
            surface_id: terminal,
        })
        .unwrap();
    let source = store.session_document().unwrap();
    directory.write(&serde_json::to_vec(&source).unwrap());
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let mut request_id = 0;
    let mut revision = initial.model.revision;
    let mut allocations = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    // Each pair allocates one browser ID and retires that temporary surface. A
    // durable refill can lag behind admission under filesystem load. Revision
    // guards prevent a rejected create from closing the original browser, and
    // only committed pairs count toward crossing the real 4096-ID boundary.
    while allocations < 4200 {
        assert!(
            Instant::now() < deadline,
            "durable allocator refill did not recover"
        );
        let batch = (4200 - allocations).min(16);
        let mut expected = revision;
        for _ in 0..batch {
            for action in [
                TargetAction::CreateBrowser {
                    profile: None,
                    focus: true,
                    url: "https://example.org/temporary".into(),
                },
                TargetAction::CloseSurface,
            ] {
                request_id += 1;
                send(
                    &desktop,
                    request_id,
                    Command::Targeted {
                        request: TargetRequest {
                            expected_model_revision: Some(expected),
                            operation: TargetOperation::Action { action },
                            ..Default::default()
                        },
                    },
                );
                expected += 1;
            }
        }
        let snapshot = wait(&desktop, |s| s.last_request_id == request_id);
        let committed = snapshot.model.revision - revision;
        assert_eq!(
            committed % 2,
            0,
            "temporary browser must be closed in the same guarded pair"
        );
        assert!(committed <= batch * 2);
        assert!(
            snapshot.persistence.error.is_none(),
            "{:?}",
            snapshot.persistence.error
        );
        assert_eq!(snapshot.model.workspaces.len(), 1);
        assert_eq!(
            snapshot.model.workspaces[0].root.panes()[0].surfaces.len(),
            1
        );
        allocations += committed / 2;
        revision = snapshot.model.revision;
        if committed < batch * 2 {
            thread::sleep(Duration::from_millis(3));
        } else {
            assert!(snapshot.last_error.is_none(), "{:?}", snapshot.last_error);
        }
    }
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let saved: zmux_workspace_core::SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert!(saved.next_id > source.next_id + 4096);
    // Restoring a zero-terminal document must still adopt its new reserved floor.
    let restarted = directory.desktop();
    let restored = wait(&restarted, ready);
    send(
        &restarted,
        1,
        Command::Action {
            action: Action::CreateBrowser {
                profile_id: None,
                focus: true,
                pane_id: pane,
                url: "https://example.org/new".into(),
            },
        },
    );
    let next = wait(&restarted, |s| s.last_request_id == 1);
    let added = next.model.workspaces[0].root.panes()[0].selected_surface;
    assert!(added.0 >= saved.next_id);
    assert!(next.model.revision > restored.model.revision);
    restarted.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn shutdown_reports_a_failed_final_recovery_write_instead_of_claiming_success() {
    let directory = Directory::new();
    let desktop = directory.desktop();
    wait(&desktop, |s| {
        ready(s) && s.persistence.last_saved_revision.is_some()
    });
    let recovery = directory.0.join("session.json");
    fs::remove_file(&recovery).unwrap();
    fs::create_dir(&recovery).unwrap();
    // The committed document has not changed, so the ordinary autosave loop has
    // nothing to submit. This exercises the mandatory final shutdown write.
    let error = desktop.shutdown(Duration::from_secs(5)).unwrap_err();
    assert!(
        error.to_string().contains("session shutdown save failed"),
        "{error}"
    );
    let final_state = desktop.snapshot().unwrap();
    assert!(final_state.closed);
    assert!(final_state.cleanup_error.is_some());
    assert!(final_state.persistence.error.is_some());
    assert!(
        recovery.is_dir(),
        "unsafe recovery destinations must remain untouched"
    );
}

#[test]
fn observed_shell_cwd_is_captured_by_explicit_save_and_final_flush_without_a_topology_action() {
    use zmux_workspace_core::{SavedContent, SavedTree, SessionDocument};
    fn saved_cwd(directory: &Directory) -> Option<String> {
        let document: SessionDocument =
            serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
        let SavedTree::Leaf { pane } = &document.workspaces[0].root else {
            panic!("one terminal pane expected")
        };
        let SavedContent::LocalTerminal { cwd } = &pane.surfaces[0].content else {
            panic!("local terminal expected")
        };
        cwd.clone()
    }
    fn change_cwd(
        desktop: &Desktop,
        session_id: zmux_workspace_core::SessionId,
        path: &std::path::Path,
        request_id: u64,
    ) {
        let escaped = path.to_string_lossy().replace('\'', "'\\''");
        send(
            desktop,
            request_id,
            Command::Input {
                session_id,
                text: format!(
                    "cd '{escaped}'; printf '\\033]7;file://localhost%s\\007' \"$PWD\"\n"
                ),
            },
        );
        wait(desktop, |s| {
            s.control_frames.iter().any(|f| {
                f.session_id == session_id
                    && f.snapshot
                        .as_ref()
                        .is_some_and(|v| v.viewport.working_directory.as_deref() == path.to_str())
            })
        });
    }
    let directory = Directory::new();
    let explicit_cwd = directory.0.join("saved-cwd");
    let final_cwd = directory.0.join("quit-cwd");
    fs::create_dir(&explicit_cwd).unwrap();
    fs::create_dir(&final_cwd).unwrap();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let session_id = initial.control_frames[0].session_id;
    change_cwd(&desktop, session_id, &explicit_cwd, 1);
    send(
        &desktop,
        2,
        Command::Session {
            action: SessionCommand::Save,
        },
    );
    let saved = wait(&desktop, |s| {
        s.persistence
            .operations
            .last()
            .is_some_and(|op| matches!(op.state, OperationState::Completed))
    });
    assert_eq!(
        saved.model.revision, initial.model.revision,
        "saving observed metadata must not act on topology"
    );
    assert_eq!(saved_cwd(&directory).as_deref(), explicit_cwd.to_str());
    change_cwd(&desktop, session_id, &final_cwd, 3);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert_eq!(saved_cwd(&directory).as_deref(), final_cwd.to_str());
    let restarted = directory.desktop();
    let restored = wait(&restarted, ready);
    let session_id = restored.control_frames[0].session_id;
    // Deliberately use a real interactive prompt with tty echo disabled. Bare
    // pwd output then shares its line with the prompt, as in the aggregate run.
    send(
        &restarted,
        4,
        Command::Input {
            session_id,
            text: "stty -echo; PS1='ZMUX_RESTORE> '; printf '\\n'\n".into(),
        },
    );
    wait(&restarted, |s| {
        s.control_frames[0].snapshot.as_ref().is_some_and(|v| {
            v.viewport
                .contents
                .lines()
                .any(|line| line.trim_end() == "ZMUX_RESTORE>")
        })
    });
    send(
        &restarted,
        5,
        Command::Input {
            session_id,
            // Frame actual pwd output independently of prompt/echo timing.
            text: "printf '\\nZMUX_CWD_BEGIN\\n'; pwd; printf 'ZMUX_CWD_END\\n'\n".into(),
        },
    );
    wait(&restarted, |s| {
        s.control_frames[0].snapshot.as_ref().is_some_and(|v| {
            let lines: Vec<_> = v.viewport.contents.lines().collect();
            lines.windows(3).any(|output| {
                output[0] == "ZMUX_CWD_BEGIN"
                    && Some(output[1]) == final_cwd.to_str()
                    && output[2] == "ZMUX_CWD_END"
            })
        })
    });
    restarted.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn unknown_browser_profile_restore_fails_before_any_pty_launch_or_topology_publish() {
    let directory = Directory::new();
    let mut store = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "must not launch".into(),
        })
        .unwrap();
    let pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.org/".into(),
            focus: true,
            profile_id: Some(uuid::Uuid::new_v4()),
        })
        .unwrap();
    let source = serde_json::to_vec(&store.session_document().unwrap()).unwrap();
    let path = directory.0.join("unknown-profile.json");
    fs::write(&path, &source).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    send(
        &desktop,
        1,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: initial.model.workspaces[0].id,
            },
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.model.workspaces.is_empty() && snapshot.pending_launches == 0
    });
    send(
        &desktop,
        2,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(path.clone()),
            },
        },
    );
    let rejected = wait(&desktop, |snapshot| {
        snapshot.persistence.operations.iter().any(|op| {
            matches!(op.state, OperationState::Failed)
                && op
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("unknown browser profile"))
        })
    });
    assert!(rejected.model.workspaces.is_empty());
    assert_eq!(rejected.pending_launches, 0);
    assert!(rejected.control_frames.is_empty());
    assert_eq!(fs::read(&path).unwrap(), source);
    desktop.shutdown(Duration::from_secs(3)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), source);
}

#[test]
fn reordered_membership_survives_real_save_restart_without_replacing_live_ptys() {
    use zmux_desktop_runtime::workspace_order::{OrderCommand, OrderPlacement, OrderRequest};
    let directory = Directory::new();
    let desktop = directory.desktop();
    wait(&desktop, ready);
    for (id, name) in [(1, "B"), (2, "C")] {
        send(
            &desktop,
            id,
            Command::Action {
                action: Action::CreateWorkspace { name: name.into() },
            },
        );
        wait(&desktop, |s| {
            s.last_request_id == id
                && ready(s)
                && s.control_frames.len() == (id + 1) as usize
                && s.control_frames.iter().all(|f| f.process_id.is_some())
        });
    }
    let before = desktop.snapshot().unwrap();
    let ids = before.model.windows[0].workspace_ids.clone();
    let request = |dry_run| Command::WorkspaceOrder {
        request: OrderRequest {
            window: None,
            caller: Default::default(),
            expected_model_revision: Some(before.model.revision),
            dry_run,
            order: OrderCommand::Single {
                workspace: ids[0].0.to_string().parse().unwrap(),
                placement: OrderPlacement::Index { index: 99 },
            },
        },
    };
    send(&desktop, 3, request(true));
    let preview = wait(&desktop, |s| s.last_request_id == 3);
    assert_eq!(preview.model, before.model);
    assert!(preview.last_error.is_none());
    send(&desktop, 4, request(false));
    let ordered = wait(&desktop, |s| s.last_request_id == 4);
    assert!(ordered.last_error.is_none(), "{:?}", ordered.last_error);
    assert_eq!(
        ordered.model.windows[0].workspace_ids,
        vec![ids[1], ids[2], ids[0]]
    );
    assert_eq!(
        ordered.model.focused_workspace,
        before.model.focused_workspace
    );
    assert_eq!(
        ordered
            .control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>(),
        before
            .control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect::<Vec<_>>()
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let document: zmux_workspace_core::SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        document.window_layout.unwrap().windows,
        ordered.model.windows
    );
    let restarted = directory.desktop();
    let restored = wait(&restarted, |s| {
        ready(s)
            && s.control_frames.len() == 3
            && s.control_frames.iter().all(|f| f.process_id.is_some())
    });
    assert_eq!(restored.model.windows, ordered.model.windows);
    assert_eq!(
        restored.model.focused_workspace,
        ordered.model.focused_workspace
    );
    assert!(restored.control_frames.iter().all(|f| {
        !before
            .control_frames
            .iter()
            .any(|old| old.session_id == f.session_id)
    }));
    restarted.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn surface_order_keeps_live_pty_identity_and_persists_exact_tab_order_with_fresh_restored_sessions()
{
    use zmux_desktop_runtime::surface_order::{OrderPlacement, OrderRequest};
    let directory = Directory::new();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let pane = initial.model.workspaces[0].root.panes()[0].id;
    for id in 1..=2 {
        send(
            &desktop,
            id,
            Command::Action {
                action: Action::CreateSurface { pane_id: pane },
            },
        );
        wait(&desktop, |s| {
            s.last_request_id == id
                && s.pending_launches == 0
                && s.control_frames.len() == id as usize + 1
                && s.control_frames.iter().all(|f| f.process_id.is_some())
        });
    }
    let before = desktop.snapshot().unwrap();
    let pane_before = before.model.workspaces[0].root.panes()[0];
    let ids: Vec<_> = pane_before.surfaces.iter().map(|s| s.id).collect();
    let order = OrderRequest {
        scope: zmux_workspace_core::targets::TargetScope {
            surface: Some(ids[0].0.to_string().parse().unwrap()),
            pane: Some(pane.0.to_string().parse().unwrap()),
            ..Default::default()
        },
        caller: Default::default(),
        identity_namespace: Some(before.model.identity_namespace),
        expected_model_revision: Some(before.model.revision),
        focus: false,
        placement: OrderPlacement::Index { index: i64::MAX },
    };
    send(
        &desktop,
        3,
        Command::SurfaceOrder {
            request: order.clone(),
        },
    );
    let after = wait(&desktop, |s| s.last_request_id == 3);
    assert!(after.last_error.is_none(), "{:?}", after.last_error);
    assert_eq!(
        after
            .control_frames
            .iter()
            .map(|f| (f.session_id.0, f.process_id))
            .collect::<std::collections::BTreeMap<_, _>>(),
        before
            .control_frames
            .iter()
            .map(|f| (f.session_id.0, f.process_id))
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    assert_eq!(
        after.model.workspaces[0].root.panes()[0]
            .surfaces
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![ids[1], ids[2], ids[0]]
    );
    // Stale captured native views and observed indices must reject before order/focus.
    let mut wrong_namespace = order.clone();
    wrong_namespace.identity_namespace = Some(uuid::Uuid::new_v4());
    wrong_namespace.expected_model_revision = None;
    send(
        &desktop,
        4,
        Command::SurfaceOrder {
            request: wrong_namespace,
        },
    );
    let rejected = wait(&desktop, |s| s.last_request_id == 4);
    assert!(rejected.last_error.as_ref().unwrap().contains("namespace"));
    assert_eq!(rejected.model, after.model);
    send(&desktop, 5, Command::SurfaceOrder { request: order });
    let stale = wait(&desktop, |s| s.last_request_id == 5);
    assert!(stale.last_error.is_some());
    assert_eq!(stale.model, after.model);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let reopened = directory.desktop();
    let restored = wait(&reopened, |s| {
        ready(s)
            && s.control_frames.len() == 3
            && s.control_frames.iter().all(|f| f.process_id.is_some())
    });
    let pane_restored = restored.model.workspaces[0].root.panes()[0];
    assert_eq!(
        pane_restored
            .surfaces
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        vec![ids[1], ids[2], ids[0]]
    );
    assert_eq!(pane_restored.selected_surface, pane_before.selected_surface);
    assert!(restored.control_frames.iter().all(|f| {
        before
            .control_frames
            .iter()
            .all(|old| old.session_id != f.session_id)
    }));
    reopened.shutdown(Duration::from_secs(5)).unwrap();
}
