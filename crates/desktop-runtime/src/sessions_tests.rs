//! Synthetic coordinator receipts exercise correlation fences; filesystem assertions
//! below use the actual S01 FIFO writer. These are not CLI/Zellij process evidence.
use super::*;
use crate::{
    local_zellij,
    zellij_coordinator::{self, Job, LaunchCertificate, LaunchKind, Receipt},
    *,
};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use uuid::Uuid;
use zmux_workspace_core::targets::TargetKind;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-checkpoint-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn completion(io: &PersistenceWorker) -> crate::persistence::PersistenceCompletion {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(c) = io.try_completion().unwrap() {
            return c;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
fn worker(directory: &Directory) -> Worker {
    let mut model = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    model
        .apply(Action::CreateWorkspace {
            name: "canonical".into(),
        })
        .unwrap();
    let mut p = SessionPersistence::start(directory.0.clone()).unwrap();
    let initial = completion(&p.io);
    p.pending.remove(&initial.operation_id.0);
    assert!(matches!(initial.result, Err(PersistenceError::NotFound)));
    let reservation =
        p.io.submit_reserve(model.snapshot().identity_namespace, model.next_identifier())
            .unwrap();
    let reserved = completion(&p.io);
    assert_eq!(reserved.operation_id, reservation);
    let PersistenceOutcome::Reserved(reserved) = reserved.result.unwrap() else {
        panic!("reservation")
    };
    p.allocation_end = reserved.end;
    p.snapshot.startup_pending = false;
    let persistence = Some(p);
    let persistence_snapshot = persistence.as_ref().unwrap().snapshot.clone();
    let browser = browser::BrowserRuntime::default();
    let downloads = downloads::DownloadRuntime::default();
    let windows = windows::WindowRuntime::default();
    let profiles = profiles::ProfileRuntime::start(None, None).unwrap();
    let local_tmux = local_tmux::Runtime::new(local_tmux::Profile::default());
    let local_zellij = zellij_coordinator::Runtime::new(None);
    let attention = Attention::default();
    let shell = Shell {
        program: "/definitely/missing-zmux-shell".into(),
        args: vec![],
        cwd: None,
    };
    let terminal_engine = None;
    let (completion_tx, completion_rx) = mpsc::sync_channel(MAX_TERMINALS);
    let files = Arc::new(crate::files::FileService::start(Default::default()).unwrap());
    let worker_shared = Arc::new(Shared {
        files: Arc::clone(&files),
        profile_native: None,
        stop: AtomicBool::new(false),
        shutdown: Mutex::new(None),
        terminal_sharing: RwLock::new(None),
        snapshot: Mutex::new(Arc::new(Snapshot {
            revision: 0,
            model: model.snapshot(),
            frames: Vec::new(),
            notifications: Vec::new(),
            workspace_metadata: Vec::new(),
            notifications_dropped: 0,
            notification_cursor: 0,
            native_notifications: attention.native_snapshot(&model.snapshot()),
            uploads: Vec::new(),
            persistence: persistence_snapshot,
            browser_profiles: profiles.snapshot.clone(),
            local_tmux_epoch: local_tmux.epoch,
            local_tmux_operations: Vec::new(),
            local_zellij_epoch: local_zellij.epoch,
            local_zellij_operations: Vec::new(),
            browser_epoch: browser.epoch(),
            browser_operations: Vec::new(),
            workspace_identities: Vec::new(),
            window_epoch: windows.epoch,
            window_requests: Vec::new(),
            window_hosts: Vec::new(),
            window_operations: Vec::new(),
            observed_key_window: None,
            download_epoch: downloads.epoch,
            downloads: Vec::new(),
            download_waits: Vec::new(),
            files: files.snapshot(Instant::now()),
            browser_pending_requests: 0,
            last_request_id: 0,
            last_error: None,
            closed: false,
            pending_launches: 0,
            cleanup_error: None,
            control_frames: Vec::new(),
        })),
    });
    Worker {
        agent_messages: agent_messages::Runtime::start(None).unwrap(),
        agent_message_result: None,
        local_tmux,
        local_zellij,
        launch_certificates: HashMap::new(),
        tmux_operation_id: None,
        model,
        browser,
        downloads,
        windows,
        window_result: None,
        workspace_order_result: None,
        workspace_group_result: None,
        surface_order_result: None,
        open_result: None,
        download_result: None,
        profiles,
        profile_operation_id: None,
        browser_operation_id: None,
        browser_dispatch: None,
        persistence,
        restoring: None,
        session_operation_id: None,
        attention,
        metadata: metadata::MetadataStore::default(),
        metadata_result: None,
        notification_mutation: None,
        notification_navigation: None,
        notification_reply: None,
        pending_replies: Vec::new(),
        transfers: Vec::new(),
        next_transfer_id: 1,
        app_active: false,
        slots: HashMap::new(),
        retired: Vec::new(),
        automatic_open_history: VecDeque::new(),
        shell,
        terminal_engine,
        shared: worker_shared,
        completion_tx,
        completion_rx,
        pending_launches: 0,
        launch_workers: Vec::new(),
        next_launch_id: 1,
        last_request_id: 0,
        last_error: None,
        cleanup_error: None,
        revision: 0,
    }
}
fn pending_start(
    worker: &mut Worker,
) -> (
    local_zellij::Correlation,
    Uuid,
    mpsc::Receiver<local_zellij::Authorization>,
) {
    let pane = worker.model.snapshot().workspaces[0].focused_pane;
    let ticket = worker.model.reserve_local_zellij_surface(pane).unwrap();
    worker.ensure_reserved(&worker.model).unwrap();
    let session = ticket.session_id();
    let surface = ticket.surface_id();
    let namespace = worker.model.snapshot().identity_namespace;
    let correlation = local_zellij::Correlation {
        installation_epoch: Uuid::new_v4(),
        profile_id: Uuid::new_v4(),
        operation_id: 1,
        identity_namespace: namespace,
        surface_id: worker
            .model
            .snapshot()
            .object_uuid(TargetKind::Surface, surface.0),
        session_id: Uuid::new_v5(&namespace, format!("session:{}", session.0).as_bytes()),
        launch_generation: 500,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let (authorization, rx) = mpsc::sync_channel(1);
    worker.launch_certificates.insert(
        (session, 500),
        LaunchCertificate {
            namespace,
            surface: correlation.surface_id,
            session,
            kind: LaunchKind::LocalZellij(None),
            cancellation: cancel.clone(),
        },
    );
    worker.pending_launches += 1;
    worker.local_zellij.jobs.insert(
        1,
        Job {
            correlation: correlation.clone(),
            session,
            surface,
            launch_id: 500,
            cancel,
            deadline: Instant::now() + Duration::from_secs(30),
            reservation: Some(ticket),
            identity: None,
            installed: false,
            checkpoint: None,
            focus: true,
            authorization,
            completion_deadline: None,
            completion_digest: None,
        },
    );
    worker.local_zellij.receipts.push(Receipt {
        epoch: worker.local_zellij.epoch,
        operation_id: 1,
        state: "starting".into(),
        profile_id: correlation.profile_id,
        owner_uuid: None,
        surface: correlation.surface_id,
        checkpoint_revision: None,
        frontend_pid: None,
        error: None,
        final_frame_received: false,
        monitor_joined: false,
        namespace_cleaned: false,
        session_joined: false,
        frontend_exit_code: None,
        workload_exit: None,
        execution_contract_version: None,
        execution_observation: None,
        completion_saved_revision: None,
        receipt_digest: None,
    });
    (correlation, Uuid::new_v4(), rx)
}
fn saved(worker: &mut Worker) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while worker
        .persistence
        .as_ref()
        .unwrap()
        .checkpoint_inflight
        .is_some()
    {
        worker.poll_sessions();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
fn close_writer(worker: &mut Worker) {
    worker
        .persistence
        .as_mut()
        .unwrap()
        .io
        .join_until(Instant::now() + Duration::from_secs(5))
        .unwrap();
}
fn install_ssh_surface(worker: &mut Worker) -> (Uuid, SurfaceId, SessionId) {
    let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let closed_port = probe.local_addr().unwrap().port();
    drop(probe);
    let namespace = Uuid::new_v4();
    let mut model = WorkspaceStore::with_identity_namespace(namespace);
    model
        .apply(Action::CreateSshWorkspace {
            name: "Reconnect fixture".into(),
            connection: zmux_remote_runtime::SshConnectionConfig {
                destination: "127.0.0.1".into(),
                port: Some(closed_port),
                identity: None,
                config_file: None,
                forward_agent: None,
                ssh_options: vec!["BatchMode=yes".into(), "ConnectTimeout=1".into()],
                jump_hosts: Vec::new(),
                command: Some(vec!["printf".into(), "STARTUP_MARKER".into()]),
            },
            focus: true,
        })
        .unwrap();
    worker.model = model;
    let model = worker.model.snapshot();
    let surface = model.workspaces[0].root.panes()[0].selected().clone();
    let session_id = surface.session_id.unwrap();
    worker.slots.insert(
        session_id,
        Slot {
            cancellation: Arc::new(AtomicBool::new(false)),
            launch_id: 1,
            session: None,
            connection: None,
            cached: None,
            starting: true,
            reconnectable: false,
            failure: None,
            resize: None,
            last_notice: 0,
        },
    );
    (namespace, surface.id, session_id)
}
fn wait_for_session_exit(session: &Session) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.snapshot().unwrap().state != zmux_session_runtime::State::Exited {
        assert!(Instant::now() < deadline, "fixture terminal did not exit");
        thread::sleep(Duration::from_millis(2));
    }
}
fn flush_shutdown(worker: &mut Worker) {
    worker.shared.stop.store(true, Ordering::Release);
    worker.local_zellij.cancel_all();
    worker.prepare_shutdown_persistence();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut submitted = false;
    while !worker.poll_shutdown_persistence(&mut submitted, deadline) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn checkpoint_actual_saved_is_required_and_newer_canonical_edits_survive() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let (correlation, owner, authorization) = pending_start(&mut worker);
    worker
        .zellij_owner_reserved(correlation.clone(), owner)
        .unwrap();
    assert!(
        authorization.try_recv().is_err(),
        "submission is not durable Saved evidence"
    );
    let workspace = worker.model.snapshot().workspaces[0].id;
    worker
        .action(Action::RenameWorkspace {
            workspace_id: workspace,
            name: "newer canonical edit".into(),
        })
        .unwrap();
    let todo = zmux_workspace_core::todo::TodoEntry {
        id: Uuid::new_v4(),
        text: "todo added during checkpoint".into(),
        state: zmux_workspace_core::todo::TodoState::Completed,
        origin: zmux_workspace_core::todo::TodoOrigin::Agent,
    };
    worker
        .model
        .replace_workspace_todos(workspace, vec![todo.clone()])
        .unwrap();
    saved(&mut worker);
    let local_zellij::Authorization::Reservation(accepted) = authorization.try_recv().unwrap()
    else {
        panic!("expected reservation authorization")
    };
    assert_eq!(accepted.correlation, correlation);
    assert_eq!(accepted.owner_uuid, owner);
    let document: SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert!(
        serde_json::to_string(&document)
            .unwrap()
            .contains(&owner.to_string())
    );
    flush_shutdown(&mut worker);
    let document: SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(document.workspaces[0].name, "newer canonical edit");
    assert_eq!(document.workspaces[0].todos.items, vec![todo]);
}
#[test]
fn checkpoint_queue_refusal_installs_nothing_and_accepted_failure_retains_descriptor() {
    {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        let document = worker.model.session_document().unwrap();
        for index in 0..crate::persistence::MAX_PENDING_OPERATIONS {
            worker
                .persistence
                .as_ref()
                .unwrap()
                .io
                .submit_save(
                    directory.0.join(format!("export-{index}.json")),
                    document.clone(),
                    true,
                )
                .unwrap();
        }
        let (correlation, owner, authorization) = pending_start(&mut worker);
        let before = worker.model.snapshot();
        assert!(
            worker
                .zellij_owner_reserved(correlation, owner)
                .unwrap_err()
                .to_string()
                .contains("queue is full")
        );
        assert_eq!(
            serde_json::to_value(worker.model.snapshot()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        assert!(authorization.try_recv().is_err());
        assert!(worker.slots.is_empty());
        close_writer(&mut worker);
    }
    {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        fs::create_dir(directory.0.join("session.json")).unwrap();
        let (correlation, owner, authorization) = pending_start(&mut worker);
        worker.zellij_owner_reserved(correlation, owner).unwrap();
        saved(&mut worker);
        assert!(
            !worker
                .persistence
                .as_ref()
                .unwrap()
                .snapshot
                .autosave_enabled
        );
        assert!(authorization.try_recv().is_err());
        assert!(worker.local_zellij.jobs[&1].cancel.load(Ordering::Acquire));
        assert!(
            serde_json::to_string(&worker.model.session_document().unwrap())
                .unwrap()
                .contains(&owner.to_string())
        );
        flush_shutdown(&mut worker);
        assert!(directory.0.join("session.json").is_dir());
    }
}
#[test]
fn checkpoint_authorization_correlation_fences_deny_stale_or_revoked_completion() {
    // These are synthetic negative state-machine controls, not durability proof.
    for fence in [
        "operation",
        "generation",
        "io",
        "target",
        "cancel",
        "deadline",
        "shutdown",
    ] {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        let (correlation, owner, authorization) = pending_start(&mut worker);
        worker.zellij_owner_reserved(correlation, owner).unwrap();
        let mut seal = worker
            .persistence
            .as_ref()
            .unwrap()
            .pending
            .values()
            .find_map(|op| op.checkpoint.as_ref().map(|op| op.seal.clone()))
            .unwrap();
        match fence {
            "operation" => seal.correlation.operation_id += 1,
            "generation" => worker.slots.get_mut(&seal.session).unwrap().launch_id += 1,
            "io" => seal.io_id += 1,
            "target" => {
                worker
                    .model
                    .apply(Action::CloseSurface {
                        surface_id: seal.surface,
                    })
                    .unwrap();
            }
            "cancel" => worker.local_zellij.jobs[&1]
                .cancel
                .store(true, Ordering::Release),
            "deadline" => worker.local_zellij.jobs.get_mut(&1).unwrap().deadline = Instant::now(),
            "shutdown" => worker.shared.stop.store(true, Ordering::Release),
            _ => unreachable!(),
        }
        worker.authorize_zellij_checkpoint(seal);
        assert!(authorization.try_recv().is_err(), "{fence}");
        close_writer(&mut worker);
    }
}
#[test]
fn mixed_pending_ordinary_close_then_shutdown_never_resurrects_zellij_target() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let pane = worker.model.snapshot().workspaces[0].focused_pane;
    worker
        .action(Action::SplitPane {
            pane_id: pane,
            axis: zmux_workspace_core::Axis::Vertical,
        })
        .unwrap();
    assert_eq!(worker.pending_launches, 1);
    let (correlation, owner, authorization) = pending_start(&mut worker);
    worker.zellij_owner_reserved(correlation, owner).unwrap();
    let closed_surface = worker.local_zellij.jobs[&1].surface;
    worker
        .action(Action::CloseSurface {
            surface_id: closed_surface,
        })
        .unwrap();
    let expected = worker
        .model_with_session_metadata()
        .session_document()
        .unwrap();
    assert_eq!(
        worker.pending_launches, 2,
        "ordinary launch remains pending at shutdown"
    );
    flush_shutdown(&mut worker);
    assert!(
        authorization.try_recv().is_err(),
        "shutdown must never authorize even an actual late Saved"
    );
    let actual: SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(actual, expected);
    assert!(
        !serde_json::to_string(&actual)
            .unwrap()
            .contains(&owner.to_string())
    );
    // The real ordinary worker was held only at coordinator completion delivery.
    let completion = worker
        .completion_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    worker.complete(completion);
    for thread in worker.launch_workers.drain(..) {
        thread.join().unwrap();
    }
}
#[test]
fn failed_ordinary_launch_keeps_canonical_failed_tile_in_checkpoint_lineage() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let pane = worker.model.snapshot().workspaces[0].focused_pane;
    worker
        .action(Action::SplitPane {
            pane_id: pane,
            axis: zmux_workspace_core::Axis::Vertical,
        })
        .unwrap();
    let ordinary = worker.model.snapshot().workspaces[0]
        .root
        .panes()
        .into_iter()
        .find(|p| p.id != pane)
        .unwrap()
        .surfaces[0]
        .clone();
    let (correlation, owner, _) = pending_start(&mut worker);
    worker.zellij_owner_reserved(correlation, owner).unwrap();
    let completion = worker
        .completion_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(completion.result.is_err());
    worker.complete(completion);
    assert!(
        worker.slots[&ordinary.session_id.unwrap()]
            .failure
            .is_some()
    );
    assert!(
        worker
            .model
            .snapshot()
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .any(|s| s.id == ordinary.id)
    );
    let expected = worker
        .model_with_session_metadata()
        .session_document()
        .unwrap();
    flush_shutdown(&mut worker);
    let actual: SessionDocument =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    assert_eq!(actual, expected);
    for thread in worker.launch_workers.drain(..) {
        thread.join().unwrap();
    }
}
fn configure(worker: &mut Worker, directory: &Directory) -> Uuid {
    let id = Uuid::new_v4();
    worker.local_zellij.configuration = Some(local_zellij::Configuration {
        installation: local_zellij::Installation {
            epoch: Uuid::new_v4(),
            cli: directory.0.join("missing-cli"),
            zellij: directory.0.join("missing-zellij"),
        },
        profiles: vec![local_zellij::Profile {
            id,
            root: directory.0.join("profile"),
        }],
    });
    id
}
#[test]
fn attach_and_start_setup_refusal_publish_no_surface_and_burn_without_leaking_tickets() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let profile_id = configure(&mut worker, &directory);
    let pane_id = worker.model.snapshot().workspaces[0].focused_pane;
    for operation_id in 1..=32 {
        worker.local_zellij.receipts.push(Receipt {
            epoch: worker.local_zellij.epoch,
            operation_id,
            state: "starting".into(),
            profile_id,
            owner_uuid: None,
            surface: Uuid::new_v4(),
            checkpoint_revision: None,
            frontend_pid: None,
            error: None,
            final_frame_received: false,
            monitor_joined: false,
            namespace_cleaned: false,
            session_joined: false,
            frontend_exit_code: None,
            workload_exit: None,
            execution_contract_version: None,
            execution_observation: None,
            completion_saved_revision: None,
            receipt_digest: None,
        });
    }
    let topology = serde_json::to_value(worker.model.snapshot().workspaces).unwrap();
    let initial = worker.model.next_identifier();
    for index in 0..12 {
        let request = if index % 2 == 0 {
            zellij_coordinator::Request::Start {
                profile_id,
                pane_id,
                name: "start".into(),
                cwd: directory.0.clone(),
                command: None,
                focus: true,
            }
        } else {
            zellij_coordinator::Request::Attach {
                profile_id,
                owner_uuid: Uuid::new_v4(),
                pane_id,
                focus: true,
            }
        };
        assert!(
            worker
                .local_zellij_command(request)
                .unwrap_err()
                .to_string()
                .contains("receipt queue is full")
        );
    }
    assert_eq!(worker.model.next_identifier(), initial + 24);
    assert!(worker.slots.is_empty());
    assert_eq!(worker.pending_launches, 0);
    assert_eq!(
        serde_json::to_value(worker.model.snapshot().workspaces).unwrap(),
        topology
    );
    close_writer(&mut worker);
}

#[test]
fn ssh_reconnect_at_terminal_cap_replaces_exited_process_and_reaps_retired_worker() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let (namespace, surface_id, session_id) = install_ssh_surface(&mut worker);
    let target = Session::spawn(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "exit 7".into()],
        cwd: None,
        size: Size { rows: 24, cols: 80 },
    })
    .unwrap();
    wait_for_session_exit(&target);
    assert!(
        !target.is_finished(),
        "exited sessions retain their history worker until explicitly closed"
    );
    let target_snapshot = target.snapshot().unwrap();
    worker.slots.get_mut(&session_id).unwrap().starting = false;
    worker.slots.get_mut(&session_id).unwrap().cached = Some(target_snapshot);
    worker.slots.get_mut(&session_id).unwrap().session = Some(target);

    for index in 0..(MAX_TERMINALS - 1) {
        worker.slots.insert(
            SessionId(9_000_000 + index as u64),
            Slot {
                cancellation: Arc::new(AtomicBool::new(false)),
                launch_id: index as u64 + 2,
                session: None,
                connection: None,
                cached: None,
                starting: true,
                reconnectable: false,
                failure: None,
                resize: None,
                last_notice: 0,
            },
        );
    }
    worker.pending_launches = MAX_TERMINALS - 1;
    worker
        .action(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id,
            expected_session_id: session_id,
        })
        .unwrap();

    let model = worker.model.snapshot();
    let replacement = model.workspaces[0].root.panes()[0].selected();
    let replacement_id = replacement.session_id.unwrap();
    assert_eq!(replacement.id, surface_id);
    assert_ne!(replacement_id, session_id);
    assert_eq!(worker.slots.len(), MAX_TERMINALS);
    assert_eq!(worker.pending_launches, MAX_TERMINALS);
    assert_eq!(
        worker.retired.len(),
        1,
        "the old worker is retired asynchronously"
    );
    assert!(worker.slots[&replacement_id].starting);

    worker.retired[0]
        .session
        .shutdown(Duration::from_secs(2))
        .unwrap();
    assert!(worker.poll_retired_sessions());
    assert!(
        worker.retired.is_empty(),
        "retired workers are joined by polling"
    );
    close_writer(&mut worker);
}
#[test]
fn reopening_zellij_cannot_bypass_retained_launch_capacity_or_consume_closed_history() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let profile_id = configure(&mut worker, &directory);
    let pane = worker.model.snapshot().workspaces[0].focused_pane;
    let ticket = worker.model.reserve_local_zellij_surface(pane).unwrap();
    let surface = ticket.surface_id();
    worker.model = worker
        .model
        .prepare_local_zellij_surface(
            &ticket,
            zmux_workspace_core::LocalZellijIdentity {
                profile_id,
                owner_uuid: Uuid::new_v4(),
            },
            true,
        )
        .unwrap()
        .into_store();
    worker
        .action(Action::CloseSurface {
            surface_id: surface,
        })
        .unwrap();
    let closed_id = worker.model.snapshot().closed_items[0].id;
    let before = serde_json::to_value(worker.model.snapshot()).unwrap();
    // Correlation-only capacity control: no real processes are represented here.
    worker.pending_launches = MAX_TERMINALS;
    assert!(
        worker
            .action(Action::ReopenClosed {
                closed_id: Some(closed_id)
            })
            .unwrap_err()
            .to_string()
            .contains("at most")
    );
    assert_eq!(
        serde_json::to_value(worker.model.snapshot()).unwrap(),
        before
    );
    assert!(worker.local_zellij.jobs.is_empty());
    assert!(worker.launch_workers.is_empty());
    close_writer(&mut worker);
}
fn completion_receipt(
    owner: Uuid,
) -> zmux_remote_runtime::local_zellij::completion::TerminalReceipt {
    use zmux_remote_runtime::local_zellij::completion::{
        CommittedBinding, DOMAIN, TerminalReceiptV1, WorkloadExit,
    };
    let identity = |pid, parent_pid, image: &str| zmux_session_runtime::child::ProcessIdentity {
        pid,
        parent_pid,
        uid: 1000,
        start: [u64::from(pid), 0],
        boot: Uuid::from_u128(99).to_string(),
        executable: image.into(),
    };
    // Synthetic terminal facts test S01's matching Saved contract, not live
    // authentication. The actual supervisor/CLI regression owns that proof.
    let binding = CommittedBinding {
        version: 1,
        owner_uuid: owner,
        owner_generation: Uuid::new_v4(),
        namespace: serde_json::from_value(serde_json::json!({"root":[1,11],"socket":[1,12]}))
            .unwrap(),
        completion_socket: (1, 13),
        server: identity(101, 1, "/opt/zellij"),
        supervisor: identity(102, 101, "/opt/zmux"),
        workload: identity(103, 102, "/bin/sh"),
    };
    let binding_digest = binding.digest().unwrap();
    TerminalReceiptV1 {
        version: 1,
        domain: DOMAIN.into(),
        binding,
        binding_digest,
        terminal_sequence: 1,
        workload_exit: WorkloadExit::Exited(7),
        child_reaped: true,
    }
    .into()
}
fn admitted_completion_receipt(
    owner: Uuid,
    observation: zmux_remote_runtime::local_zellij::completion::ExecutionObservation,
) -> zmux_remote_runtime::local_zellij::completion::TerminalReceipt {
    use zmux_remote_runtime::local_zellij::{
        admission::{AuthorizedInvocation, ChildLifetime, EXECUTION_DOMAIN, TERMINAL_DOMAIN},
        completion::{ExecutionBindingV2, TerminalReceipt, TerminalReceiptV2},
    };
    let TerminalReceipt::V1(legacy) = completion_receipt(owner) else {
        unreachable!()
    };
    let old = legacy.binding;
    let mut birth = old.workload;
    birth.executable = old.supervisor.executable.clone();
    let lifetime = ChildLifetime::from(&birth);
    let binding = ExecutionBindingV2 {
        version: 2,
        domain: EXECUTION_DOMAIN.into(),
        owner_uuid: owner,
        owner_generation: old.owner_generation,
        namespace: old.namespace,
        completion_socket: old.completion_socket,
        server: old.server,
        supervisor: old.supervisor,
        workload_birth: birth,
        lifetime,
        invocation: AuthorizedInvocation {
            shell: "/bin/sh".into(),
            invocation_digest: "a".repeat(64),
        },
        admission_id: Uuid::new_v4(),
    };
    let binding_digest = binding.digest().unwrap();
    TerminalReceipt::V2(TerminalReceiptV2 {
        version: 2,
        domain: TERMINAL_DOMAIN.into(),
        binding,
        binding_digest,
        terminal_sequence: 1,
        workload_exit: zmux_remote_runtime::local_zellij::completion::WorkloadExit::Exited(127),
        child_reaped: true,
        execution_observation: observation,
    })
}
fn active_completion(
    worker: &mut Worker,
) -> (
    crate::zellij_coordinator::PendingCompletion,
    mpsc::Receiver<local_zellij::Authorization>,
) {
    let (correlation, owner, authorization) = pending_start(worker);
    worker
        .zellij_owner_reserved(correlation.clone(), owner)
        .unwrap();
    saved(worker);
    assert!(matches!(
        authorization.try_recv().unwrap(),
        local_zellij::Authorization::Reservation(_)
    ));
    let receipt = completion_receipt(owner);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut job = worker.local_zellij.jobs.remove(&1).unwrap();
    worker
        .launch_certificates
        .remove(&(job.session, job.launch_id));
    worker.pending_launches -= 1;
    // Correlation-only lifecycle control: completion may occur long after the
    // already connected owner's original startup clock has expired.
    job.deadline = Instant::now() - Duration::from_secs(1);
    worker.local_zellij.active.insert(1, job);
    (
        crate::zellij_coordinator::PendingCompletion {
            correlation,
            receipt,
            deadline,
            viewport: None,
        },
        authorization,
    )
}
fn observe_completion(
    worker: &mut Worker,
    pending: crate::zellij_coordinator::PendingCompletion,
) -> crate::zellij_coordinator::PendingCompletion {
    let id = pending.correlation.operation_id;
    worker
        .observe_zellij_completion(pending.correlation, pending.receipt, pending.deadline)
        .unwrap();
    worker.local_zellij.pending_completions.remove(&id).unwrap()
}
#[test]
fn completed_history_authorizes_only_its_actual_saved_and_outlives_startup_clock() {
    // V1, V2 immediate completion, and V2 exec failure keep distinct evidence
    // through the same real writer; these are not live authentication controls.
    for case in 0..3 {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        let (mut pending, authorization) = active_completion(&mut worker);
        let (contract_version, observation_wire, expected_state) = match case {
            0 => (1, serde_json::Value::Null, "workload_completed"),
            1 => {
                pending.receipt = admitted_completion_receipt(pending.receipt.owner_uuid(), zmux_remote_runtime::local_zellij::completion::ExecutionObservation::NotObserved);
                (
                    2,
                    serde_json::json!({"kind":"not_observed"}),
                    "workload_completed",
                )
            }
            _ => {
                pending.receipt = admitted_completion_receipt(pending.receipt.owner_uuid(), zmux_remote_runtime::local_zellij::completion::ExecutionObservation::ExecFailed {
                    stage: zmux_remote_runtime::local_zellij::admission::ExecFailureStage::ConfiguredShellExec,
                    errno: 2,
                });
                (
                    2,
                    serde_json::json!({"kind":"exec_failed","stage":"configured_shell_exec","errno":2}),
                    "execution_failed",
                )
            }
        };
        let expected = pending.receipt.clone();
        let pending = observe_completion(&mut worker, pending);
        worker.zellij_workload_finished(pending).unwrap();
        assert!(
            authorization.try_recv().is_err(),
            "queued completion is not actual Saved"
        );
        saved(&mut worker);
        let local_zellij::Authorization::Completion(ack) = authorization.try_recv().unwrap() else {
            panic!("expected completion Saved")
        };
        assert_eq!(ack.owner_generation, expected.owner_generation());
        assert_eq!(ack.receipt_digest, expected.digest().unwrap());
        assert!(ack.io_id > 0);
        assert_eq!(
            worker.local_zellij.receipts[0].completion_saved_revision,
            Some(ack.document_revision)
        );
        let operation = &worker.local_zellij.receipts[0];
        assert_eq!(operation.state, expected_state);
        assert_eq!(operation.execution_contract_version, Some(contract_version));
        assert_eq!(
            serde_json::to_value(&operation.execution_observation).unwrap(),
            observation_wire
        );
        let document: SessionDocument =
            serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
        let wire = serde_json::to_value(document).unwrap();
        let content = &wire["workspaces"][0]["root"]["pane"]["surfaces"][1]["content"];
        assert_eq!(content["kind"], "completed_local_zellij_terminal");
        assert_eq!(
            content["history"]["receipt"],
            serde_json::to_value(expected).unwrap()
        );
        close_writer(&mut worker);
    }
}
#[test]
fn completed_history_queue_refusal_installs_nothing_and_write_failure_never_authorizes() {
    for refused in [true, false] {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        let (pending, authorization) = active_completion(&mut worker);
        let pending = observe_completion(&mut worker, pending);
        let before = worker.model.snapshot();
        if refused {
            close_writer(&mut worker);
            assert!(worker.zellij_workload_finished(pending).is_err());
            assert_eq!(worker.model.snapshot(), before);
        } else {
            // The real writer accepts this Save but cannot replace a directory.
            fs::remove_file(directory.0.join("session.json")).unwrap();
            fs::create_dir(directory.0.join("session.json")).unwrap();
            worker.zellij_workload_finished(pending).unwrap();
            saved(&mut worker);
            assert!(
                !worker
                    .persistence
                    .as_ref()
                    .unwrap()
                    .snapshot
                    .autosave_enabled
            );
            let surface = worker.local_zellij.active[&1].surface;
            assert!(
                worker
                    .model
                    .snapshot()
                    .workspaces
                    .iter()
                    .flat_map(|w| w.root.panes())
                    .flat_map(|pane| &pane.surfaces)
                    .find(|s| s.id == surface)
                    .unwrap()
                    .local_zellij_completion
                    .is_some()
            );
            assert!(
                worker.local_zellij.active[&1]
                    .cancel
                    .load(Ordering::Acquire)
            );
            flush_shutdown(&mut worker);
            assert!(directory.0.join("session.json").is_dir());
        }
        assert!(authorization.try_recv().is_err());
        assert!(
            worker.local_zellij.receipts[0]
                .completion_saved_revision
                .is_none()
        );
    }
}
#[test]
fn completed_history_shutdown_or_changed_slot_denies_even_actual_late_saved() {
    for shutdown in [false, true] {
        let directory = Directory::new();
        let mut worker = worker(&directory);
        let (pending, authorization) = active_completion(&mut worker);
        let pending = observe_completion(&mut worker, pending);
        worker.zellij_workload_finished(pending).unwrap();
        if shutdown {
            flush_shutdown(&mut worker);
        } else {
            let session = worker.local_zellij.active[&1].session;
            worker.slots.get_mut(&session).unwrap().launch_id += 1;
            saved(&mut worker);
            close_writer(&mut worker);
        }
        assert!(authorization.try_recv().is_err());
        assert!(
            worker.local_zellij.receipts[0]
                .completion_saved_revision
                .is_none()
        );
    }
}

#[test]
fn completed_fact_blocks_all_new_input_before_saved_while_the_real_pty_is_live() {
    use attention::{NotificationContent, NotificationReplyOutcome};
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let (pending, authorization) = active_completion(&mut worker);
    let job = &worker.local_zellij.active[&1];
    let session_id = job.session;
    let surface = job.surface;
    // A real PTY independently records delivered bytes. The completion fact
    // remains synthetic: the supervisor/CLI tests own live authentication.
    let session = Session::spawn(zmux_session_runtime::Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), r#"stty -echo; printf 'FENCE_READY\n'; while IFS= read -r line; do printf 'DELIVERED:%s\n' "$line"; done"#.into()],
        cwd: None,
        size: Size { rows: 24, cols: 80 },
    }).unwrap();
    worker.slots.get_mut(&session_id).unwrap().session = Some(session);
    let wait_for = |worker: &Worker, marker: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(snapshot) = worker.live_session(session_id).unwrap().snapshot()
                && snapshot.viewport.contents.contains(marker)
            {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "PTY did not deliver {marker}");
            thread::sleep(Duration::from_millis(2));
        }
    };
    wait_for(&worker, "FENCE_READY");
    worker
        .execute_command(Command::Input {
            session_id,
            text: "BEFORE\n".into(),
        })
        .unwrap();
    wait_for(&worker, "DELIVERED:BEFORE");
    // Claiming an ordinary valid reply reaches this same live PTY before fencing.
    let notify = |worker: &mut Worker| {
        let id = worker
            .attention
            .notify_owned(
                &worker.model.snapshot(),
                surface,
                "test",
                &NotificationContent {
                    title: "completion fence".into(),
                    reply: true,
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        worker
            .attention
            .items
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .target
            .clone()
            .unwrap()
    };
    let target = notify(&mut worker);
    let (sender, receiver) = mpsc::sync_channel(1);
    worker
        .admit_reply(
            target,
            "REPLY_BEFORE".into(),
            Some(Instant::now() + Duration::from_secs(1)),
            &mut Some(sender),
        )
        .unwrap();
    let reply_deadline = Instant::now() + Duration::from_secs(5);
    let execution = loop {
        worker.poll_replies();
        if let Ok(execution) = receiver.try_recv() {
            break execution;
        }
        assert!(Instant::now() < reply_deadline);
        thread::sleep(Duration::from_millis(2));
    };
    execution.result.unwrap();
    assert_eq!(
        execution.notification_reply.unwrap().outcome,
        NotificationReplyOutcome::Complete
    );
    let visible = wait_for(&worker, "DELIVERED:REPLY_BEFORE");
    worker.slots.get_mut(&session_id).unwrap().cached = Some(visible);
    worker
        .observe_zellij_completion(pending.correlation, pending.receipt, pending.deadline)
        .unwrap();
    assert!(
        authorization.try_recv().is_err(),
        "no completion Saved has occurred"
    );
    assert!(
        worker.local_zellij.receipts[0]
            .completion_saved_revision
            .is_none()
    );
    let mut commands = vec![
        Command::Input {
            session_id,
            text: "BLOCKED_INPUT\n".into(),
        },
        Command::Paste {
            session_id,
            text: "BLOCKED_PASTE\n".into(),
        },
        Command::Key {
            session_id,
            event: serde_json::from_value(serde_json::json!({"key":"enter"})).unwrap(),
        },
        Command::Pointer {
            session_id,
            event: zmux_terminal_adapter::Pointer {
                action: zmux_terminal_adapter::PointerAction::Press,
                button: zmux_terminal_adapter::PointerButton::Left,
                row: 0,
                col: 0,
                modifiers: 0,
            },
        },
        Command::Wheel {
            session_id,
            event: zmux_terminal_adapter::Wheel {
                lines: 1,
                row: 0,
                col: 0,
                modifiers: 0,
                history: false,
            },
        },
    ];
    for operation in [
        targets::TargetOperation::SendText {
            text: "BLOCKED_SEND\n".into(),
        },
        targets::TargetOperation::PasteText {
            text: "BLOCKED_TARGET_PASTE\n".into(),
        },
    ] {
        commands.push(Command::Targeted {
            request: targets::TargetRequest {
                scope: zmux_workspace_core::targets::TargetScope {
                    surface: Some(surface.0.to_string().parse().unwrap()),
                    ..Default::default()
                },
                operation,
                ..Default::default()
            },
        });
    }
    commands.push(Command::Transfer {
        action: transfers::TransferCommand::PasteFiles {
            surface_id: surface,
            expected_session_id: Some(session_id),
            paths: vec![PathBuf::from("/tmp/BLOCKED_PATH")],
        },
    });
    commands.push(Command::Targeted {
        request: targets::TargetRequest {
            scope: zmux_workspace_core::targets::TargetScope {
                surface: Some(surface.0.to_string().parse().unwrap()),
                ..Default::default()
            },
            operation: targets::TargetOperation::PasteFiles {
                paths: vec![PathBuf::from("/tmp/BLOCKED_TARGET_PATH")],
            },
            ..Default::default()
        },
    });
    for command in commands {
        let error = worker.execute_command(command).unwrap_err().to_string();
        assert!(
            error.contains("workload has completed"),
            "wrong denial: {error}"
        );
    }
    worker.notification_reply = None;
    let target = notify(&mut worker);
    let (sender, _receiver) = mpsc::sync_channel(1);
    worker
        .admit_reply(
            target,
            "BLOCKED_REPLY".into(),
            Some(Instant::now() + Duration::from_secs(1)),
            &mut Some(sender),
        )
        .unwrap();
    let reply = worker.notification_reply.as_ref().unwrap();
    assert_eq!(reply.outcome, NotificationReplyOutcome::NotWritten);
    assert_eq!(reply.bytes_written, 0);
    assert!(
        reply
            .reason
            .as_ref()
            .unwrap()
            .contains("workload has completed")
    );
    // Read-only history is still usable. A direct fixture barrier then proves no
    // denied command's bytes arrived before it in the actual Session FIFO.
    worker
        .execute_command(Command::Scroll {
            session_id,
            lines: 0,
        })
        .unwrap();
    worker
        .live_session(session_id)
        .unwrap()
        .send_input(b"BARRIER\n")
        .unwrap();
    let snapshot = wait_for(&worker, "DELIVERED:BARRIER");
    assert!(!snapshot.viewport.contents.contains("BLOCKED"));
    // A later cleanup screen must not replace the viewport frozen with the
    // completion event, even when another checkpoint delays its Save admission.
    worker.slots.get_mut(&session_id).unwrap().cached = Some(snapshot);
    let pending = worker.local_zellij.pending_completions.remove(&1).unwrap();
    worker.zellij_workload_finished(pending).unwrap();
    saved(&mut worker);
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.0.join("session.json")).unwrap()).unwrap();
    let contents =
        document["workspaces"][0]["root"]["pane"]["surfaces"][1]["content"]["history"]["viewport"]
            ["contents"]
            .as_str()
            .unwrap();
    assert!(contents.contains("DELIVERED:REPLY_BEFORE"));
    assert!(
        !contents.contains("BARRIER"),
        "later screen replaced captured workload history"
    );
    worker
        .slots
        .get_mut(&session_id)
        .unwrap()
        .session
        .as_mut()
        .unwrap()
        .shutdown(Duration::from_secs(5))
        .unwrap();
    close_writer(&mut worker);
}

#[test]
fn ssh_reconnect_appears_after_process_exit_and_replaces_through_worker_action() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let (namespace, surface_id, session_id) = install_ssh_surface(&mut worker);
    let session = Session::spawn(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "sleep 0.1; exit 7".into()],
        cwd: None,
        size: Size { rows: 24, cols: 80 },
    })
    .unwrap();
    let running = session.snapshot().unwrap();
    assert_eq!(running.state, zmux_session_runtime::State::Running);
    {
        let slot = worker.slots.get_mut(&session_id).unwrap();
        slot.starting = false;
        slot.session = Some(session);
        slot.cached = Some(running);
    }
    assert!(!worker.refresh_ssh_reconnectability());
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: namespace,
                surface_id,
                expected_session_id: session_id,
            })
            .unwrap_err()
            .to_string()
            .contains("still running")
    );

    let session = worker.slots[&session_id].session.as_ref().unwrap();
    wait_for_session_exit(session);
    assert!(
        !session.is_finished(),
        "process exit can be proved before the history worker is joined"
    );
    let exited = session.snapshot().unwrap();
    assert_eq!(exited.state, zmux_session_runtime::State::Exited);
    worker.slots.get_mut(&session_id).unwrap().cached = Some(exited);
    assert!(worker.refresh_ssh_reconnectability());
    let published = worker.publish(false);
    assert!(
        published
            .frames
            .iter()
            .any(|frame| frame.session_id == session_id && frame.reconnectable)
    );

    worker
        .action(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id,
            expected_session_id: session_id,
        })
        .unwrap();
    let after = worker.model.snapshot();
    let replacement = after.workspaces[0].root.panes()[0].selected();
    assert_eq!(replacement.id, surface_id);
    assert_ne!(replacement.session_id, Some(session_id));
    assert_eq!(
        replacement.remote.as_ref().unwrap().destination,
        "127.0.0.1"
    );
    assert_eq!(replacement.remote.as_ref().unwrap().command, None);
    assert_eq!(
        worker.retired.len(),
        1,
        "old-session cleanup remains asynchronous"
    );
    let replacement_id = replacement.session_id.unwrap();
    assert!(worker.slots[&replacement_id].starting);
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: namespace,
                surface_id,
                expected_session_id: session_id,
            })
            .is_err()
    );
    close_writer(&mut worker);
}

#[test]
fn ssh_reconnect_rejects_local_and_pending_but_accepts_settled_launch_failure() {
    let directory = Directory::new();
    let mut worker = worker(&directory);
    let local = worker.model.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: worker.model.snapshot().identity_namespace,
                surface_id: local.id,
                expected_session_id: local.session_id.unwrap(),
            })
            .is_err()
    );

    let (namespace, surface_id, session_id) = install_ssh_surface(&mut worker);
    assert!(!worker.refresh_ssh_reconnectability());
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: namespace,
                surface_id,
                expected_session_id: session_id,
            })
            .unwrap_err()
            .to_string()
            .contains("still starting")
    );
    worker.slots.get_mut(&session_id).unwrap().starting = false;
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: namespace,
                surface_id,
                expected_session_id: session_id,
            })
            .unwrap_err()
            .to_string()
            .contains("no completed launch failure")
    );
    worker.slots.get_mut(&session_id).unwrap().failure =
        Some("fixture authentication failure".into());
    assert!(worker.refresh_ssh_reconnectability());
    let published = worker.publish(false);
    assert!(
        published
            .frames
            .iter()
            .any(|frame| frame.session_id == session_id && frame.reconnectable)
    );
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: Uuid::new_v4(),
                surface_id,
                expected_session_id: session_id,
            })
            .unwrap_err()
            .to_string()
            .contains("identity changed")
    );
    worker
        .action(Action::ReconnectSsh {
            identity_namespace: namespace,
            surface_id,
            expected_session_id: session_id,
        })
        .unwrap();
    let replacement_id = worker.model.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .session_id
        .unwrap();
    assert_ne!(replacement_id, session_id);
    assert!(worker.slots[&replacement_id].starting);
    assert!(
        worker
            .action(Action::ReconnectSsh {
                identity_namespace: namespace,
                surface_id,
                expected_session_id: session_id,
            })
            .is_err(),
        "duplicate requests for the old generation are stale"
    );
    close_writer(&mut worker);
}
