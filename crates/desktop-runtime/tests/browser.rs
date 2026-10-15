//! Rust/native protocol ownership tests. These do not execute or claim WebKit behavior.
use serde_json::json;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zmux_desktop_runtime::browser::*;
use zmux_workspace_core::{
    Action, SurfaceId, WorkspaceStore,
    targets::{CallerContext, TargetKind, TargetScope},
};

fn fixture() -> (WorkspaceStore, SurfaceId) {
    let mut model = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    model
        .apply(Action::CreateWorkspace {
            name: "owner".into(),
        })
        .unwrap();
    let pane = model.snapshot().workspaces[0].focused_pane;
    model
        .apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.org".into(),
        })
        .unwrap();
    let surface = model.snapshot().workspaces[0].root.panes()[0].selected_surface;
    (model, surface)
}
fn request(model: &WorkspaceStore, surface: SurfaceId, action: BrowserAction) -> BrowserRequest {
    let snapshot = model.snapshot();
    BrowserRequest {
        scope: TargetScope {
            surface: Some(
                snapshot
                    .object_uuid(TargetKind::Surface, surface.0)
                    .to_string()
                    .parse()
                    .unwrap(),
            ),
            ..Default::default()
        },
        caller: CallerContext::default(),
        expected_model_revision: Some(snapshot.revision),
        timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
        action,
    }
}
fn read() -> BrowserAction {
    BrowserAction::Get {
        kind: GetKind::Url,
        selector: None,
        attribute: None,
        property: None,
    }
}
fn attach(
    runtime: &mut BrowserRuntime,
    model: &WorkspaceStore,
    surface_id: SurfaceId,
    now: Instant,
) -> BrowserOwner {
    let surface = model.snapshot().workspaces[0].root.panes()[0]
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .clone();
    let owner = BrowserOwner {
        epoch: runtime.epoch(),
        identity_namespace: model.snapshot().identity_namespace,
        surface_id,
        host_id: Uuid::new_v4(),
        document_generation: 1,
        model_navigation: surface.navigation,
    };
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::Attach {
                epoch: owner.epoch,
                surface_id,
                host_id: owner.host_id,
                document_generation: owner.document_generation,
                model_navigation: owner.model_navigation,
            },
            now,
        )
        .unwrap();
    owner
}

#[test]
fn executor_resolves_uuid_once_and_native_dispatch_is_once_only() {
    let (mut model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    assert!(matches!(
        runtime.admit(&model.snapshot(), request(&model, surface, read()), now),
        Err(BrowserError::Unavailable)
    ));
    attach(&mut runtime, &model, surface, now);
    let stale = request(&model, surface, read());
    model
        .apply(Action::RenameSurface {
            surface_id: surface,
            name: "new title".into(),
        })
        .unwrap();
    assert!(matches!(
        runtime.admit(&model.snapshot(), stale, now),
        Err(BrowserError::Target(_))
    ));
    let id = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    // A subsequent selection change must not resolve the request against a new focus.
    model
        .apply(Action::SelectSurface {
            surface_id: model.snapshot().workspaces[0].root.panes()[0].surfaces[0].id,
        })
        .unwrap();
    let batch = runtime.drain_requests(&model.snapshot(), now);
    assert_eq!(batch.requests.len(), 1);
    assert_eq!(batch.requests[0].owner.surface_id, surface);
    assert_eq!(batch.requests[0].operation_id, id);
    assert!(
        runtime
            .drain_requests(&model.snapshot(), now)
            .requests
            .is_empty()
    );
    let mut foreign = batch.requests[0].owner.clone();
    foreign.host_id = Uuid::new_v4();
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: id,
                owner: foreign,
                result: json!("foreign host")
            },
            now
        ),
        Err(BrowserError::StaleOwner)
    ));
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: id,
                owner: batch.requests[0].owner.clone(),
                result: json!({"value":"https://example.org/"}),
            },
            now,
        )
        .unwrap();
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Completed
    );
    assert_eq!(
        runtime.receipt(id).unwrap().result.as_ref().unwrap()["value"],
        "https://example.org/"
    );
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: id,
                owner: batch.requests[0].owner.clone(),
                result: json!("duplicate")
            },
            now
        ),
        Err(BrowserError::NotDispatched)
    ));
}

#[test]
fn causal_native_navigation_keeps_only_its_own_receipt_in_the_new_document() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let read_id = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    let navigation = runtime
        .admit(
            &model.snapshot(),
            request(
                &model,
                surface,
                BrowserAction::Reload {
                    snapshot_after: true,
                },
            ),
            now,
        )
        .unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::DocumentChanged {
                owner: owner.clone(),
                document_generation: 2,
                model_navigation: 0,
                caused_by: Some(navigation),
            },
            now,
        )
        .unwrap();
    assert_eq!(
        runtime.receipt(read_id).unwrap().state,
        BrowserOperationState::Failed
    );
    assert!(
        !runtime
            .receipt(read_id)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .outcome_unknown
    );
    assert_eq!(
        runtime.receipt(navigation).unwrap().state,
        BrowserOperationState::Pending
    );
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: navigation,
                owner: owner.clone(),
                result: json!("old document")
            },
            now
        ),
        Err(BrowserError::StaleOwner)
    ));
    let mut next_owner = owner;
    next_owner.document_generation = 2;
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: navigation,
                owner: next_owner,
                result: json!({"value":{"nodes":[]}}),
            },
            now,
        )
        .unwrap();
    assert_eq!(
        runtime.receipt(navigation).unwrap().state,
        BrowserOperationState::Completed
    );
}

#[test]
fn core_navigation_close_and_restore_epochs_reject_stale_native_owners() {
    let (mut model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let id = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    model
        .apply(Action::NavigateBrowser {
            surface_id: surface,
            url: "https://example.net".into(),
        })
        .unwrap();
    assert!(runtime.reconcile(&model.snapshot(), now));
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
    assert!(matches!(
        runtime.admit(&model.snapshot(), request(&model, surface, read()), now),
        Err(BrowserError::Unavailable)
    ));
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::DocumentChanged {
                owner: owner.clone(),
                document_generation: 2,
                model_navigation: 1,
                caused_by: None,
            },
            now,
        )
        .unwrap();
    let id = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    runtime.reset("session restore");
    assert_ne!(runtime.epoch(), owner.epoch);
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Attach {
                epoch: owner.epoch,
                surface_id: surface,
                host_id: owner.host_id,
                document_generation: 3,
                model_navigation: 1
            },
            now
        ),
        Err(BrowserError::StaleOwner)
    ));
    attach(&mut runtime, &model, surface, now);
    let id = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    model
        .apply(Action::CloseSurface {
            surface_id: surface,
        })
        .unwrap();
    runtime.reconcile(&model.snapshot(), now);
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
}

#[test]
fn deadlines_bound_queue_and_distinguish_unexecuted_from_uncertain_mutations() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    attach(&mut runtime, &model, surface, now);
    let mut queued = request(
        &model,
        surface,
        BrowserAction::Eval {
            script: "window.value = 1".into(),
        },
    );
    queued.timeout_ms = 10;
    let id = runtime
        .admit(&model.snapshot(), queued.clone(), now)
        .unwrap();
    runtime.reconcile(&model.snapshot(), now + Duration::from_millis(10));
    assert!(
        !runtime
            .receipt(id)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .outcome_unknown
    );
    let id = runtime.admit(&model.snapshot(), queued, now).unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    runtime.reconcile(&model.snapshot(), now + Duration::from_millis(10));
    assert!(
        runtime
            .receipt(id)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .outcome_unknown
    );
    for _ in 0..MAX_PENDING_BROWSER_OPERATIONS {
        runtime
            .admit(&model.snapshot(), request(&model, surface, read()), now)
            .unwrap();
    }
    assert!(matches!(
        runtime.admit(&model.snapshot(), request(&model, surface, read()), now),
        Err(BrowserError::QueueFull)
    ));
    assert_eq!(
        runtime.pending_request_count(),
        MAX_PENDING_BROWSER_OPERATIONS
    );
    runtime.reconcile(&model.snapshot(), now + Duration::from_secs(3));
    assert_eq!(runtime.pending_request_count(), 0);
    assert!(
        runtime
            .admit(
                &model.snapshot(),
                request(&model, surface, read()),
                now + Duration::from_secs(3)
            )
            .is_ok()
    );
}

#[test]
fn native_result_and_retained_history_budgets_fail_or_evict_explicitly() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let mut ids = Vec::new();
    for _ in 0..8 {
        let id = runtime
            .admit(&model.snapshot(), request(&model, surface, read()), now)
            .unwrap();
        runtime.drain_requests(&model.snapshot(), now);
        runtime
            .host_event(
                &model.snapshot(),
                BrowserHostEvent::Complete {
                    operation_id: id,
                    owner: owner.clone(),
                    result: json!("a".repeat(MAX_BROWSER_RESULT_BYTES - 128)),
                },
                now,
            )
            .unwrap();
        ids.push(id);
    }
    assert!(matches!(
        runtime.receipt(ids[0]),
        Err(BrowserError::NotFound)
    ));
    assert_eq!(
        runtime.receipt(*ids.last().unwrap()).unwrap().state,
        BrowserOperationState::Completed
    );
    let id = runtime
        .admit(
            &model.snapshot(),
            request(
                &model,
                surface,
                BrowserAction::Eval {
                    script: "'a'.repeat(100000)".into(),
                },
            ),
            now,
        )
        .unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: id,
                owner,
                result: json!("a".repeat(MAX_BROWSER_RESULT_BYTES + 1))
            },
            now
        ),
        Err(BrowserError::ResultTooLarge)
    ));
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
    assert!(
        runtime
            .receipt(id)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .outcome_unknown
    );
}

#[test]
fn public_socket_and_generic_command_json_cannot_forge_native_callbacks() {
    use zmux_desktop_runtime::{Command, control::ControlCommand};
    let event = json!({"event":"attach","epoch":Uuid::new_v4(),"surface_id":1,"host_id":Uuid::new_v4(),"document_generation":1,"model_navigation":0});
    assert!(serde_json::from_value::<BrowserHostEvent>(event.clone()).is_ok());
    let forged = json!({"method":"browser_host","event":event.clone()});
    assert!(serde_json::from_value::<ControlCommand>(forged).is_err());
    assert!(
        serde_json::from_value::<Command>(json!({"command":"browser_host","event":event})).is_err()
    );
    let mut invalid = BrowserAction::Eval {
        script: "x".repeat(MAX_SCRIPT_BYTES + 1),
    };
    assert!(invalid.validate().is_err());
    invalid = BrowserAction::Snapshot {
        options: SnapshotOptions {
            max_depth: Some(0),
            ..Default::default()
        },
    };
    assert!(invalid.validate().is_ok());
}

#[test]
fn late_navigation_after_deadline_updates_host_without_reviving_the_expired_operation() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let mut reload = request(
        &model,
        surface,
        BrowserAction::Reload {
            snapshot_after: false,
        },
    );
    reload.timeout_ms = 10;
    let id = runtime.admit(&model.snapshot(), reload, now).unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    let late = now + Duration::from_millis(20);
    runtime.reconcile(&model.snapshot(), late);
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::DocumentChanged {
                owner,
                document_generation: 2,
                model_navigation: 0,
                caused_by: Some(id),
            },
            late,
        )
        .unwrap();
    assert_eq!(
        runtime.receipt(id).unwrap().state,
        BrowserOperationState::Failed
    );
    runtime
        .admit(&model.snapshot(), request(&model, surface, read()), late)
        .unwrap();
    let dispatched = runtime.drain_requests(&model.snapshot(), late);
    assert_eq!(dispatched.requests.len(), 1);
    assert_eq!(dispatched.requests[0].owner.document_generation, 2);
}

#[test]
fn desktop_shutdown_fails_queued_reads_and_marks_dispatched_evaluation_uncertain() {
    use std::{sync::Arc, thread};
    use zmux_desktop_runtime::{Command, Desktop, Request, Shell, Snapshot};
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let wait = |predicate: &dyn Fn(&Snapshot) -> bool| -> Arc<Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(snapshot) = desktop.snapshot()
                && predicate(&snapshot)
            {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "desktop state did not settle");
            thread::sleep(Duration::from_millis(2));
        }
    };
    let initial = wait(&|snapshot| !snapshot.model.workspaces.is_empty());
    desktop
        .submit(Request {
            version: 1,
            request_id: 1,
            payload: Command::Action {
                action: Action::CreateBrowser {
                    pane_id: initial.model.workspaces[0].focused_pane,
                    url: "https://example.org/".into(),
                },
            },
        })
        .unwrap();
    let browser = wait(&|snapshot| snapshot.last_request_id == 1);
    assert!(browser.last_error.is_none());
    let surface = browser.model.workspaces[0].root.panes()[0].selected();
    desktop
        .browser_host_event(BrowserHostEvent::Attach {
            epoch: browser.browser_epoch,
            surface_id: surface.id,
            host_id: Uuid::new_v4(),
            document_generation: 1,
            model_navigation: surface.navigation,
        })
        .unwrap();
    let scope = TargetScope {
        surface: Some(format!("surface:{}", surface.id.0).parse().unwrap()),
        ..Default::default()
    };
    let make_request = |action| BrowserRequest {
        scope: scope.clone(),
        caller: Default::default(),
        expected_model_revision: None,
        timeout_ms: MAX_OPERATION_TIMEOUT_MS,
        action,
    };
    desktop
        .submit(Request {
            version: 1,
            request_id: 2,
            payload: Command::Browser {
                request: make_request(BrowserAction::Eval {
                    script: "window.someEffect = true".into(),
                }),
            },
        })
        .unwrap();
    wait(&|snapshot| snapshot.browser_operations.len() == 1);
    let dispatched = desktop
        .browser_drain_requests(Duration::from_secs(2))
        .unwrap();
    assert_eq!(dispatched.requests.len(), 1);
    let eval_id = dispatched.requests[0].operation_id;
    desktop
        .submit(Request {
            version: 1,
            request_id: 3,
            payload: Command::Browser {
                request: make_request(read()),
            },
        })
        .unwrap();
    let pending = wait(&|snapshot| snapshot.browser_operations.len() == 2);
    assert_eq!(
        pending.browser_pending_requests, 1,
        "read remains queued for native dispatch"
    );
    assert!(
        pending
            .browser_operations
            .iter()
            .all(|receipt| receipt.state == BrowserOperationState::Pending)
    );
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    let closed = desktop.snapshot().unwrap();
    assert!(closed.closed);
    assert_eq!(closed.browser_pending_requests, 0);
    assert_eq!(closed.browser_operations.len(), 2);
    for receipt in &closed.browser_operations {
        assert_eq!(receipt.state, BrowserOperationState::Failed);
        let error = receipt.error.as_ref().unwrap();
        assert_eq!(error.code, "browser_cancelled");
        assert_eq!(error.outcome_unknown, receipt.operation_id == eval_id);
    }
}

#[test]
fn full_owner_reattach_recovers_lost_document_events_without_reviving_or_hijacking_work() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let original = attach(&mut runtime, &model, surface, now);
    let stale = runtime
        .admit(
            &model.snapshot(),
            request(
                &model,
                surface,
                BrowserAction::Eval {
                    script: "window.effect = true".into(),
                },
            ),
            now,
        )
        .unwrap();
    runtime.drain_requests(&model.snapshot(), now);

    // Native has advanced twice, but its incremental document events were lost.
    // A full owner snapshot must recover the current host without replaying work.
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::Attach {
                epoch: original.epoch,
                surface_id: surface,
                host_id: original.host_id,
                document_generation: 3,
                model_navigation: original.model_navigation,
            },
            now,
        )
        .unwrap();
    let stale_receipt = runtime.receipt(stale).unwrap();
    assert_eq!(stale_receipt.state, BrowserOperationState::Failed);
    assert!(stale_receipt.error.as_ref().unwrap().outcome_unknown);
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: stale,
                owner: original.clone(),
                result: json!("late old document"),
            },
            now
        ),
        Err(BrowserError::StaleOwner)
    ));
    // An incremental event captured before resync cannot roll the host backward.
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::DocumentChanged {
                owner: original.clone(),
                document_generation: 2,
                model_navigation: original.model_navigation,
                caused_by: None,
            },
            now
        ),
        Err(BrowserError::StaleOwner)
    ));

    let fresh = runtime
        .admit(&model.snapshot(), request(&model, surface, read()), now)
        .unwrap();
    let dispatched = runtime.drain_requests(&model.snapshot(), now);
    assert_eq!(dispatched.requests.len(), 1);
    assert_eq!(dispatched.requests[0].operation_id, fresh);
    let current = dispatched.requests[0].owner.clone();
    assert_eq!(current.host_id, original.host_id);
    assert_eq!(current.document_generation, 3);
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Attach {
                epoch: original.epoch,
                surface_id: surface,
                host_id: Uuid::new_v4(),
                document_generation: 4,
                model_navigation: original.model_navigation,
            },
            now
        ),
        Err(BrowserError::HostConflict)
    ));
    runtime
        .host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: fresh,
                owner: current,
                result: json!({"value":"current document"}),
            },
            now,
        )
        .unwrap();
    assert_eq!(
        runtime.receipt(fresh).unwrap().state,
        BrowserOperationState::Completed
    );
    assert_eq!(
        runtime.receipt(stale).unwrap().state,
        BrowserOperationState::Failed
    );
}
