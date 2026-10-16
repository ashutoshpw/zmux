//! Real filesystem + PTY lifecycle contracts; pure descriptor validation lives in core.
#[path = "support/desktop_snapshot.rs"]
mod desktop_snapshot;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
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
            serde_json::to_string(&*desktop_snapshot::read_until(desktop, deadline)).unwrap()
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
fn workspace_todos_survive_real_save_shutdown_and_fresh_reopen_without_transient_channels() {
    use zmux_desktop_runtime::{
        metadata::{MetadataAction, PullRequest, TodoOrigin, TodoSelector, TodoState},
        targets::{TargetOperation, TargetRequest},
    };
    let directory = Directory::new();
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let workspace = initial.model.workspaces[0].id;
    let metadata = |action| Command::Targeted {
        request: TargetRequest {
            scope: zmux_workspace_core::targets::TargetScope {
                workspace: Some(workspace.0.to_string().parse().unwrap()),
                ..Default::default()
            },
            operation: TargetOperation::Metadata { action },
            ..Default::default()
        },
    };
    let actions = [
        MetadataAction::AddTodo {
            text: "first pending item".into(),
            state: TodoState::Pending,
            origin: TodoOrigin::User,
        },
        MetadataAction::AddTodo {
            text: "second active item".into(),
            state: TodoState::InProgress,
            origin: TodoOrigin::Agent,
        },
        MetadataAction::AddTodo {
            text: "third completed item".into(),
            state: TodoState::Completed,
            origin: TodoOrigin::User,
        },
        MetadataAction::SetStatus {
            key: "temporary".into(),
            value: "do not persist".into(),
            icon: None,
            color: None,
            priority: 0,
        },
        MetadataAction::SetProgress {
            value: 0.5,
            label: None,
        },
        MetadataAction::Log {
            message: "transient log".into(),
            level: Default::default(),
            source: None,
        },
        MetadataAction::SetPullRequest {
            pull_request: PullRequest::parse("https://github.com/acme/project/pull/17").unwrap(),
        },
    ];
    for (index, action) in actions.into_iter().enumerate() {
        let id = 100 + index as u64;
        send(&desktop, id, metadata(action));
        let applied = wait(&desktop, |snapshot| snapshot.last_request_id == id);
        assert!(applied.last_error.is_none(), "{:?}", applied.last_error);
    }
    let before = desktop_snapshot::read(&desktop);
    let expected = serde_json::to_value(&before.workspace_metadata[0].todos).unwrap();
    assert_eq!(expected.as_array().unwrap().len(), 3);
    send(
        &desktop,
        200,
        Command::Session {
            action: SessionCommand::Save,
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.persistence.operations.iter().any(|operation| {
            operation.kind == "save" && matches!(operation.state, OperationState::Completed)
        })
    });
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let reopened = directory.desktop();
    let restored = wait(&reopened, ready);
    assert_eq!(restored.model.workspaces[0].id, workspace);
    assert_eq!(
        restored.model.identity_namespace,
        before.model.identity_namespace
    );
    let restored_todos = restored
        .workspace_metadata
        .iter()
        .find(|row| row.workspace_id == workspace)
        .map(|row| serde_json::to_value(&row.todos).unwrap());
    assert_eq!(
        restored_todos,
        Some(expected),
        "saved checklist identities/order/state/origin must survive fresh process ownership"
    );
    let row = restored
        .workspace_metadata
        .iter()
        .find(|row| row.workspace_id == workspace)
        .unwrap();
    assert!(
        row.statuses.is_empty()
            && row.progress.is_none()
            && row.logs.is_empty()
            && row.pull_request.is_none()
    );
    let first_id = row.todos[0].id;
    send(
        &reopened,
        201,
        metadata(MetadataAction::SetTodoState {
            item: TodoSelector::Id { id: first_id },
            state: TodoState::Completed,
        }),
    );
    let autosaved = wait(&reopened, |snapshot| {
        if snapshot.last_request_id != 201 {
            return false;
        }
        let disk: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
        disk["workspaces"][0]["todos"][0]["state"] == "completed"
    });
    assert_eq!(
        autosaved.model.revision, restored.model.revision,
        "todo metadata must not replace native rows by advancing topology revision"
    );
    assert!(
        autosaved.revision > restored.revision,
        "metadata still publishes a new desktop revision"
    );
    let changed = serde_json::to_value(&autosaved.workspace_metadata[0].todos).unwrap();
    assert_eq!(changed[0]["id"], first_id.to_string());
    assert_eq!(changed[0]["state"], "completed");
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        disk["version"],
        zmux_workspace_core::SESSION_DOCUMENT_VERSION
    );
    assert_eq!(
        disk["workspaces"][0]["todos"], changed,
        "todo-only mutation must autosave without another topology action"
    );
    let export = directory.0.join("todo-export.json");
    send(
        &reopened,
        202,
        Command::Session {
            action: SessionCommand::Export {
                path: export.clone(),
                force: false,
            },
        },
    );
    wait(&reopened, |snapshot| {
        snapshot.persistence.operations.iter().any(|operation| {
            operation.kind == "export" && matches!(operation.state, OperationState::Completed)
        })
    });
    let exported = fs::read(&export).unwrap();
    let export_document: serde_json::Value = serde_json::from_slice(&exported).unwrap();
    assert_eq!(export_document["workspaces"][0]["todos"], changed);
    for (offset, action) in [
        MetadataAction::SetStatus {
            key: "live".into(),
            value: "preserve on rejection".into(),
            icon: None,
            color: None,
            priority: 0,
        },
        MetadataAction::SetProgress {
            value: 0.75,
            label: None,
        },
        MetadataAction::Log {
            message: "live transient".into(),
            level: Default::default(),
            source: None,
        },
        MetadataAction::SetPullRequest {
            pull_request: PullRequest::parse("https://github.com/acme/project/pull/18").unwrap(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let request_id = 210 + offset as u64;
        send(&reopened, request_id, metadata(action));
        wait(&reopened, |snapshot| snapshot.last_request_id == request_id);
    }
    let live = desktop_snapshot::read(&reopened);
    let live_metadata = serde_json::to_value(&live.workspace_metadata).unwrap();
    send(
        &reopened,
        214,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(export.clone()),
            },
        },
    );
    let rejected = wait(&reopened, |snapshot| snapshot.last_request_id == 214);
    assert!(
        rejected
            .last_error
            .as_deref()
            .unwrap()
            .contains("empty desktop")
    );
    assert_eq!(
        serde_json::to_value(&rejected.workspace_metadata).unwrap(),
        live_metadata,
        "failed same-namespace admission must preserve current todos and transient channels"
    );
    send(&reopened, 220, metadata(MetadataAction::ClearTodos {}));
    wait(&reopened, |snapshot| {
        if snapshot.last_request_id != 220 {
            return false;
        }
        let disk: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
        disk["workspaces"][0]["todos"] == serde_json::json!([])
    });
    reopened.shutdown(Duration::from_secs(5)).unwrap();
    let cleared = directory.desktop();
    let after_clear = wait(&cleared, ready);
    assert!(after_clear.workspace_metadata.is_empty());
    send(
        &cleared,
        221,
        Command::Action {
            action: Action::CloseWorkspace {
                workspace_id: workspace,
            },
        },
    );
    let empty = wait(&cleared, |snapshot| {
        snapshot.model.workspaces.is_empty()
            && snapshot.pending_launches == 0
            && snapshot.control_frames.is_empty()
    });
    let empty_model = empty.model.clone();
    let mut invalid_import = export_document.clone();
    invalid_import["workspaces"][0]["todos"][1]["id"] =
        invalid_import["workspaces"][0]["todos"][0]["id"].clone();
    let corrupt = directory.0.join("invalid-todos.json");
    let corrupt_bytes = serde_json::to_vec(&invalid_import).unwrap();
    fs::write(&corrupt, &corrupt_bytes).unwrap();
    fs::set_permissions(&corrupt, fs::Permissions::from_mode(0o600)).unwrap();
    send(
        &cleared,
        222,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(corrupt.clone()),
            },
        },
    );
    let failed = wait(&cleared, |snapshot| {
        snapshot.persistence.operations.iter().any(|operation| {
            operation.kind == "restore" && matches!(operation.state, OperationState::Failed)
        })
    });
    assert_eq!(failed.model, empty_model);
    assert!(failed.workspace_metadata.is_empty());
    assert_eq!(fs::read(&corrupt).unwrap(), corrupt_bytes);
    send(
        &cleared,
        223,
        Command::Session {
            action: SessionCommand::Restore {
                path: Some(export.clone()),
            },
        },
    );
    let imported = wait(&cleared, |snapshot| {
        ready(snapshot)
            && snapshot.persistence.operations.iter().any(|operation| {
                operation.kind == "restore" && matches!(operation.state, OperationState::Completed)
            })
    });
    assert_eq!(
        imported.model.identity_namespace,
        before.model.identity_namespace
    );
    assert_eq!(
        serde_json::to_value(&imported.workspace_metadata[0].todos).unwrap(),
        changed
    );
    let row = &imported.workspace_metadata[0];
    assert!(
        row.statuses.is_empty()
            && row.progress.is_none()
            && row.logs.is_empty()
            && row.pull_request.is_none()
    );
    assert_eq!(
        fs::read(&export).unwrap(),
        exported,
        "import must not rewrite its source document"
    );
    cleared.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn final_surface_move_keeps_live_pty_and_persists_only_the_destination_checklist() {
    use zmux_workspace_core::todo::{TodoEntry, TodoOrigin, TodoState};
    let directory = Directory::new();
    let mut seed = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    for name in ["retiring source", "surviving destination"] {
        seed.apply(Action::CreateWorkspace { name: name.into() })
            .unwrap();
    }
    let source = seed.snapshot().workspaces[0].clone();
    let destination = seed.snapshot().workspaces[1].clone();
    seed.replace_workspace_todos(
        source.id,
        vec![TodoEntry {
            id: uuid::Uuid::from_u128(1),
            text: "retire with the source owner".into(),
            state: TodoState::Pending,
            origin: TodoOrigin::User,
        }],
    )
    .unwrap();
    let destination_todos = vec![
        TodoEntry {
            id: uuid::Uuid::from_u128(101),
            text: "destination first".into(),
            state: TodoState::InProgress,
            origin: TodoOrigin::Agent,
        },
        TodoEntry {
            id: uuid::Uuid::from_u128(102),
            text: "destination second".into(),
            state: TodoState::Completed,
            origin: TodoOrigin::User,
        },
    ];
    seed.replace_workspace_todos(destination.id, destination_todos.clone())
        .unwrap();
    directory.write(&serde_json::to_vec(&seed.session_document().unwrap()).unwrap());
    let desktop = directory.desktop();
    let initial = wait(&desktop, ready);
    let moving = initial.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    let session_id = moving.session_id.unwrap();
    let processes: std::collections::BTreeMap<_, _> = initial
        .control_frames
        .iter()
        .map(|frame| (frame.session_id.0, frame.process_id.unwrap()))
        .collect();
    assert_eq!(processes.len(), 2, "fixture must launch two real PTYs");
    send(
        &desktop,
        1,
        Command::Input {
            session_id,
            text: "export ZMUX_FINAL_MOVE_VALUE=retained; printf '\\nFINAL_MOVE_READY\\n'\r".into(),
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == session_id
                && frame.snapshot.as_ref().is_some_and(|terminal| {
                    terminal
                        .viewport
                        .contents
                        .lines()
                        .any(|line| line == "FINAL_MOVE_READY")
                })
        })
    });
    send(
        &desktop,
        2,
        Command::Action {
            action: Action::MoveSurface {
                surface_id: moving.id,
                pane_id: destination.focused_pane,
                index: 1,
            },
        },
    );
    let moved = wait(&desktop, |snapshot| snapshot.last_request_id == 2);
    assert!(moved.last_error.is_none(), "{:?}", moved.last_error);
    assert_eq!(moved.model.workspaces.len(), 1);
    assert_eq!(moved.model.workspaces[0].id, destination.id);
    assert_eq!(
        moved.model.workspaces[0].root.panes()[0].selected_surface,
        moving.id
    );
    assert!(moved.model.closed_items.is_empty());
    assert_eq!(moved.workspace_metadata.len(), 1);
    assert_eq!(moved.workspace_metadata[0].workspace_id, destination.id);
    assert_eq!(moved.workspace_metadata[0].todos, destination_todos);
    assert_eq!(
        moved
            .control_frames
            .iter()
            .map(|frame| (frame.session_id.0, frame.process_id.unwrap()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        processes,
        "moving the final surface must preserve both real PTY processes"
    );
    send(
        &desktop,
        3,
        Command::Input {
            session_id,
            text: "printf '\\nFINAL_MOVE_%s\\n' \"$ZMUX_FINAL_MOVE_VALUE\"\r".into(),
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.control_frames.iter().any(|frame| {
            frame.session_id == session_id
                && frame.snapshot.as_ref().is_some_and(|terminal| {
                    terminal
                        .viewport
                        .contents
                        .lines()
                        .any(|line| line == "FINAL_MOVE_retained")
                })
        })
    });
    send(
        &desktop,
        4,
        Command::Session {
            action: SessionCommand::Save,
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.persistence.operations.iter().any(|operation| {
            operation.kind == "save" && matches!(operation.state, OperationState::Completed)
        })
    });
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(
        disk["version"],
        zmux_workspace_core::SESSION_DOCUMENT_VERSION
    );
    assert_eq!(disk["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(disk["workspaces"][0]["id"], destination.id.0);
    assert_eq!(
        disk["workspaces"][0]["todos"],
        serde_json::to_value(&destination_todos).unwrap()
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let reopened = directory.desktop();
    let restored = wait(&reopened, ready);
    assert_eq!(restored.model.workspaces.len(), 1);
    assert_eq!(restored.model.workspaces[0].id, destination.id);
    assert_eq!(restored.workspace_metadata.len(), 1);
    assert_eq!(restored.workspace_metadata[0].todos, destination_todos);
    let surfaces = &restored.model.workspaces[0].root.panes()[0].surfaces;
    assert_eq!(surfaces.len(), 2);
    let moved = surfaces
        .iter()
        .find(|surface| surface.id == moving.id)
        .unwrap();
    assert_ne!(
        moved.session_id,
        Some(session_id),
        "fresh reopen creates a new shell generation"
    );
    assert!(restored.model.closed_items.is_empty());
    reopened.shutdown(Duration::from_secs(5)).unwrap();
}

#[test]
fn unconfigured_zellij_restore_retains_v6_source_and_never_launches_it_as_an_ordinary_shell() {
    use zmux_workspace_core::{
        LocalZellijIdentity,
        todo::{TodoEntry, TodoOrigin, TodoState},
    };
    let directory = Directory::new();
    let mut seed = WorkspaceStore::with_identity_namespace(uuid::Uuid::new_v4());
    seed.apply(Action::CreateWorkspace {
        name: "requires exact Zellij owner".into(),
    })
    .unwrap();
    let owner = seed.snapshot().workspaces[0].clone();
    seed.replace_workspace_todos(
        owner.id,
        vec![TodoEntry {
            id: uuid::Uuid::from_u128(1),
            text: "must remain in protected source".into(),
            state: TodoState::Completed,
            origin: TodoOrigin::Agent,
        }],
    )
    .unwrap();
    let ticket = seed
        .reserve_local_zellij_surface(owner.focused_pane)
        .unwrap();
    seed = seed
        .prepare_local_zellij_surface(
            &ticket,
            LocalZellijIdentity {
                profile_id: uuid::Uuid::from_u128(10),
                owner_uuid: uuid::Uuid::from_u128(20),
            },
            true,
        )
        .unwrap()
        .into_store();
    let source = serde_json::to_vec(&seed.session_document().unwrap()).unwrap();
    directory.write(&source);
    let desktop = directory.desktop();
    let failed = wait(&desktop, |snapshot| {
        ready(snapshot) && snapshot.persistence.error.is_some()
    });
    assert!(
        failed
            .persistence
            .error
            .as_deref()
            .unwrap()
            .contains("local Zellij installation is not configured")
    );
    assert!(!failed.persistence.autosave_enabled);
    assert_eq!(
        failed.model.workspaces.len(),
        1,
        "only S01's explicit recovery starter may be present"
    );
    assert_ne!(
        failed.model.identity_namespace,
        seed.snapshot().identity_namespace
    );
    assert!(
        failed.workspace_metadata.is_empty(),
        "failed candidate todos cannot be published"
    );
    assert_eq!(failed.control_frames.len(), 1);
    assert_eq!(fs::read(directory.0.join("session.json")).unwrap(), source);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    assert_eq!(fs::read(directory.0.join("session.json")).unwrap(), source);
}

#[test]
fn todo_file_capacity_uses_enriched_live_cwd_before_admission_without_latching_a_failed_save() {
    use zmux_desktop_runtime::{
        metadata::MetadataAction,
        targets::{TargetOperation, TargetRequest},
    };
    let directory = Directory::new();
    let desktop = directory.desktop();
    wait(&desktop, ready);
    for request_id in 1..8 {
        send(
            &desktop,
            request_id,
            Command::Action {
                action: Action::CreateWorkspace {
                    name: format!("bounded owner {request_id}"),
                },
            },
        );
        wait(&desktop, |snapshot| {
            ready(snapshot) && snapshot.last_request_id == request_id
        });
    }
    let before_observation = desktop_snapshot::read(&desktop);
    let expected_cwd = format!("/{}", "0".repeat(3500));
    for (index, frame) in before_observation.control_frames.iter().enumerate() {
        send(
            &desktop,
            100 + index as u64,
            Command::Input {
                session_id: frame.session_id,
                text: "printf '\\033]7;file:///%03500d\\007' 0\n".into(),
            },
        );
    }
    let observed = wait(&desktop, |snapshot| {
        snapshot.control_frames.len() == 8
            && snapshot.control_frames.iter().all(|frame| {
                frame.snapshot.as_ref().is_some_and(|terminal| {
                    terminal.viewport.working_directory.as_deref() == Some(expected_cwd.as_str())
                })
            })
    });
    assert_eq!(observed.model.revision, before_observation.model.revision);
    assert!(
        observed
            .model
            .workspaces
            .iter()
            .all(
                |workspace| workspace.root.panes()[0].selected().cwd.as_deref()
                    != Some(expected_cwd.as_str())
            ),
        "the preflight must use enriched capture, not already-copied model cwd"
    );
    let owners: Vec<_> = observed
        .model
        .workspaces
        .iter()
        .map(|workspace| workspace.id)
        .collect();
    let mut accepted = 0;
    let mut refused = false;
    for index in 0..200 {
        let before = desktop_snapshot::read(&desktop);
        let request_id = 1000 + index as u64;
        send(
            &desktop,
            request_id,
            Command::Targeted {
                request: TargetRequest {
                    scope: zmux_workspace_core::targets::TargetScope {
                        workspace: Some(owners[index / 50].0.to_string().parse().unwrap()),
                        ..Default::default()
                    },
                    operation: TargetOperation::Metadata {
                        action: MetadataAction::AddTodo {
                            text: "\\".repeat(2048),
                            state: Default::default(),
                            origin: Default::default(),
                        },
                    },
                    ..Default::default()
                },
            },
        );
        let result = wait(&desktop, |snapshot| snapshot.last_request_id == request_id);
        if let Some(error) = &result.last_error {
            assert!(
                error.contains("session document exceeds its byte limit"),
                "candidate must fail the full enriched document bound before a later metadata bound: {error}"
            );
            assert_eq!(result.model, before.model);
            assert_eq!(
                serde_json::to_value(&result.workspace_metadata).unwrap(),
                serde_json::to_value(&before.workspace_metadata).unwrap()
            );
            assert!(
                result.persistence.error.is_none(),
                "rejected admission must not create a failed autosave"
            );
            refused = true;
            break;
        }
        accepted += 1;
    }
    assert!(refused);
    send(
        &desktop,
        5000,
        Command::Session {
            action: SessionCommand::Save,
        },
    );
    wait(&desktop, |snapshot| {
        snapshot.persistence.operations.iter().any(|operation| {
            operation.kind == "save" && matches!(operation.state, OperationState::Completed)
        })
    });
    let bytes = fs::read(directory.0.join("session.json")).unwrap();
    assert!(bytes.len() <= zmux_desktop_runtime::persistence::MAX_DOCUMENT_BYTES);
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        saved["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|workspace| workspace["todos"].as_array().unwrap().len())
            .sum::<usize>(),
        accepted
    );
    assert!(saved["workspaces"].as_array().unwrap().iter().all(
        |workspace| workspace["root"]["pane"]["surfaces"][0]["content"]["cwd"] == expected_cwd
    ));
    desktop.shutdown(Duration::from_secs(5)).unwrap();
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

fn with_checklist(mut store: WorkspaceStore) -> WorkspaceStore {
    let workspace = store.snapshot().workspaces[0].id;
    store
        .replace_workspace_todos(
            workspace,
            vec![zmux_workspace_core::todo::TodoEntry {
                id: uuid::Uuid::from_u128(321),
                text: "validated durable item".into(),
                state: Default::default(),
                origin: Default::default(),
            }],
        )
        .unwrap();
    store
}

#[test]
fn corrupt_unsafe_versioned_and_overcapacity_startup_files_are_retained_and_protected() {
    let store = with_checklist(fixture());
    let document = serde_json::to_value(store.session_document().unwrap()).unwrap();
    let mut invalid_todo = document.clone();
    invalid_todo["workspaces"][0]["todos"][0]["id"] = serde_json::json!(uuid::Uuid::nil());
    let mut missing_todos = document.clone();
    missing_todos["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("todos");
    let mut duplicated = store.clone();
    duplicated
        .apply(Action::CreateWorkspace {
            name: "other owner".into(),
        })
        .unwrap();
    let mut duplicate_document =
        serde_json::to_value(duplicated.session_document().unwrap()).unwrap();
    duplicate_document["workspaces"][1]["todos"] =
        duplicate_document["workspaces"][0]["todos"].clone();
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
        (
            "invalid todo",
            serde_json::to_vec(&invalid_todo).unwrap(),
            0o600,
        ),
        (
            "missing todos",
            serde_json::to_vec(&missing_todos).unwrap(),
            0o600,
        ),
        (
            "duplicate across owners",
            serde_json::to_vec(&duplicate_document).unwrap(),
            0o600,
        ),
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
    let mut store = with_checklist(fixture());
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
    assert!(
        snapshot.workspace_metadata.is_empty(),
        "failed candidate must not publish its durable todos"
    );
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
    let bounded = desktop_snapshot::read(&desktop);
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
    let final_state = desktop_snapshot::read(&desktop);
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
    let before = desktop_snapshot::read(&desktop);
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
    let before = desktop_snapshot::read(&desktop);
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

#[test]
fn workspace_groups_preserve_live_children_and_survive_actual_save_shutdown_and_fresh_restore() {
    use zmux_workspace_core::workspace_groups::GroupAction;
    let directory = Directory::new();
    let desktop = directory.desktop();
    wait(&desktop, ready);
    for (index, name) in ["B", "C", "D"].iter().enumerate() {
        let request = 100 + index as u64;
        send(
            &desktop,
            request,
            Command::Action {
                action: Action::CreateWorkspace {
                    name: (*name).into(),
                },
            },
        );
        let current = wait(&desktop, |s| s.last_request_id == request && ready(s));
        assert!(current.last_error.is_none(), "{:?}", current.last_error);
    }
    let before = desktop_snapshot::read(&desktop);
    let window = before.model.windows[0].clone();
    let ids = window.workspace_ids.clone();
    let children = |s: &Snapshot| {
        let mut rows: Vec<_> = s
            .control_frames
            .iter()
            .map(|f| (f.session_id, f.process_id))
            .collect();
        rows.sort_by_key(|row| row.0.0);
        rows
    };
    let original_children = children(&before);
    assert_eq!(original_children.len(), 4);
    assert!(original_children.iter().all(|(_, pid)| pid.is_some()));
    send(
        &desktop,
        110,
        Command::Action {
            action: Action::WorkspaceGroup {
                command: GroupAction::Create {
                    window_id: window.id,
                    name: "Persistent group".into(),
                    workspace_ids: vec![ids[0], ids[2]],
                    external_id: Some("stable-controller".into()),
                },
            },
        },
    );
    let created = wait(&desktop, |s| s.last_request_id == 110);
    assert!(created.last_error.is_none(), "{:?}", created.last_error);
    let group_id = created.model.workspace_groups[0].id;
    send(
        &desktop,
        111,
        Command::Action {
            action: Action::WorkspaceGroup {
                command: GroupAction::SetCollapsed {
                    group_id,
                    collapsed: true,
                },
            },
        },
    );
    let grouped = wait(&desktop, |s| s.last_request_id == 111);
    assert!(grouped.last_error.is_none(), "{:?}", grouped.last_error);
    assert_eq!(
        children(&grouped),
        original_children,
        "group metadata/order cannot replace live session children"
    );
    assert_eq!(
        grouped.model.focused_workspace,
        before.model.focused_workspace
    );
    assert_eq!(grouped.model.workspaces, before.model.workspaces);
    let expected_groups = grouped.model.workspace_groups.clone();
    let expected_windows = grouped.model.windows.clone();
    send(
        &desktop,
        112,
        Command::Session {
            action: SessionCommand::Save,
        },
    );
    wait(&desktop, |s| {
        s.last_request_id == 112
            && s.persistence
                .operations
                .iter()
                .any(|op| op.kind == "save" && matches!(op.state, OperationState::Completed))
    });
    let disk: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(disk["version"], 9);
    assert_eq!(
        disk["workspace_groups"],
        serde_json::to_value(&expected_groups).unwrap()
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let reopened = directory.desktop();
    let restored = wait(&reopened, ready);
    assert_eq!(restored.model.workspace_groups, expected_groups);
    assert_eq!(restored.model.windows, expected_windows);
    assert_eq!(
        restored.model.identity_namespace,
        before.model.identity_namespace
    );
    let restored_children = children(&restored);
    assert_eq!(restored_children.len(), 4);
    assert!(
        restored_children
            .iter()
            .all(|(session, _)| !original_children.iter().any(|(old, _)| old == session)),
        "restart must reserve fresh runtime Session IDs while preserving group identity"
    );
    reopened.shutdown(Duration::from_secs(5)).unwrap();
}
