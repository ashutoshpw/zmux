//! Rust/native protocol ownership tests. These do not execute or claim WebKit behavior.
#[path = "support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::json;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zmux_desktop_runtime as desktop_runtime;
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
            profile_id: None,
            focus: true,
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

fn wait_desktop(
    desktop: &zmux_desktop_runtime::Desktop,
    predicate: &dyn Fn(&zmux_desktop_runtime::Snapshot) -> bool,
) -> std::sync::Arc<zmux_desktop_runtime::Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = desktop_snapshot::read_until(desktop, deadline);
        if predicate(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "desktop browser state did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn observed_browser_metadata_is_bounded_and_fenced_after_a_newer_cli_navigation() {
    use zmux_desktop_runtime::{
        Command, Desktop, Request, Shell,
        targets::{TargetOperation, TargetRequest},
    };
    use zmux_workspace_core::targets::TargetAction;
    let desktop = Desktop::new(Shell {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: None,
    })
    .unwrap();
    let initial = wait_desktop(&desktop, &|snapshot| !snapshot.model.workspaces.is_empty());
    let send = |id, payload| {
        desktop
            .submit(Request {
                version: 1,
                request_id: id,
                payload,
            })
            .unwrap();
        wait_desktop(&desktop, &|snapshot| snapshot.last_request_id == id)
    };
    let created = send(
        1,
        Command::Action {
            action: Action::CreateBrowser {
                profile_id: None,
                focus: true,
                pane_id: initial.model.workspaces[0].focused_pane,
                url: "https://example.org/old".into(),
            },
        },
    );
    let browser = created.model.workspaces[0].root.panes()[0]
        .selected()
        .clone();
    let original = BrowserOwner {
        epoch: created.browser_epoch,
        identity_namespace: created.model.identity_namespace,
        surface_id: browser.id,
        host_id: Uuid::new_v4(),
        document_generation: 1,
        model_navigation: browser.navigation,
    };
    desktop
        .browser_host_event(BrowserHostEvent::Attach {
            epoch: original.epoch,
            surface_id: browser.id,
            host_id: original.host_id,
            document_generation: 1,
            model_navigation: original.model_navigation,
        })
        .unwrap();
    let background = send(
        2,
        Command::Action {
            action: Action::CreateWorkspace {
                name: "Keep focus".into(),
            },
        },
    );
    let foreground = background.model.focused_workspace;
    let navigated = send(
        3,
        Command::Targeted {
            request: TargetRequest {
                scope: TargetScope {
                    surface: Some(format!("surface:{}", browser.id.0).parse().unwrap()),
                    ..Default::default()
                },
                caller: CallerContext::default(),
                expected_model_revision: None,
                operation: TargetOperation::Action {
                    action: TargetAction::NavigateBrowser {
                        url: "https://example.org/new".into(),
                    },
                },
            },
        },
    );
    assert!(navigated.last_error.is_none());
    let unowned = send(
        4,
        Command::Action {
            action: Action::ObserveBrowser {
                surface_id: browser.id,
                url: "https://example.org/old".into(),
                title: "Stale title".into(),
            },
        },
    );
    assert!(
        unowned.last_error.is_some(),
        "generic observation bypassed native document ownership after a CLI navigation"
    );
    assert_eq!(unowned.model, navigated.model);
    let observed = |owner: &BrowserOwner, url: &str, title: &str| {
        serde_json::from_value(json!({
            "event":"observed", "owner":owner, "url":url, "title":title,
        }))
        .unwrap()
    };
    assert!(
        desktop
            .browser_host_event(observed(
                &original,
                "https://example.org/old",
                "Late old document"
            ))
            .is_err()
    );
    assert_eq!(desktop_snapshot::read(&desktop).model, navigated.model);
    let mut current = original.clone();
    current.document_generation += 1;
    current.model_navigation += 1;
    desktop
        .browser_host_event(BrowserHostEvent::DocumentChanged {
            owner: original.clone(),
            document_generation: current.document_generation,
            model_navigation: current.model_navigation,
            caused_by: None,
        })
        .unwrap();
    let title = "\u{0007}界".repeat(130);
    let valid_url = format!("https://example.org/{}", "p".repeat(4500));
    desktop
        .browser_host_event(observed(&current, &valid_url, &title))
        .unwrap();
    let accepted = desktop_snapshot::read(&desktop);
    let actual = accepted.model.workspaces[0].root.panes()[0]
        .surfaces
        .iter()
        .find(|surface| surface.id == browser.id)
        .unwrap();
    assert_eq!(actual.title, "界".repeat(128));
    assert_eq!(actual.url.as_deref(), Some(valid_url.as_str()));
    assert_eq!(
        actual.navigation, current.model_navigation,
        "observations must not cause another load"
    );
    assert_eq!(accepted.model.focused_workspace, foreground);
    for owner in [
        original,
        BrowserOwner {
            host_id: Uuid::new_v4(),
            ..current.clone()
        },
        BrowserOwner {
            identity_namespace: Uuid::new_v4(),
            ..current.clone()
        },
        BrowserOwner {
            epoch: Uuid::new_v4(),
            ..current.clone()
        },
    ] {
        assert!(
            desktop
                .browser_host_event(observed(&owner, "https://example.org/stale", "Stale title"))
                .is_err()
        );
    }
    assert!(
        desktop
            .browser_host_event(observed(
                &current,
                "https://example.org/",
                &"x".repeat(4097)
            ))
            .is_err()
    );
    assert!(
        desktop
            .browser_host_event(observed(
                &current,
                &format!("https://example.org/{}", "x".repeat(8192)),
                "oversized URL"
            ))
            .is_err()
    );
    assert_eq!(desktop_snapshot::read(&desktop).model, accepted.model);
    let closed = send(
        5,
        Command::Action {
            action: Action::CloseSurface {
                surface_id: browser.id,
            },
        },
    );
    assert!(
        desktop
            .browser_host_event(observed(&current, "https://example.org/closed", "Closed"))
            .is_err()
    );
    assert_eq!(desktop_snapshot::read(&desktop).model, closed.model);
    desktop.shutdown(Duration::from_secs(5)).unwrap();
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
    for action in [
        BrowserAction::Keyboard {
            operation: BrowserKeyboardOperation::Press,
            key: "Enter".into(),
            snapshot_after: true,
        },
        BrowserAction::Keyboard {
            operation: BrowserKeyboardOperation::Down,
            key: "Shift".into(),
            snapshot_after: false,
        },
        BrowserAction::Keyboard {
            operation: BrowserKeyboardOperation::Up,
            key: "Shift".into(),
            snapshot_after: false,
        },
        BrowserAction::Eval {
            script: "window.value = 1".into(),
        },
        BrowserAction::Click {
            selector: "#send".into(),
            snapshot_after: false,
        },
        BrowserAction::Type {
            selector: "#entry".into(),
            text: "chunk".into(),
            snapshot_after: true,
        },
        BrowserAction::Select {
            selector: "#choice".into(),
            value: "a".into(),
            snapshot_after: true,
        },
        BrowserAction::Scroll {
            selector: None,
            dx: 1,
            dy: 2,
            snapshot_after: true,
        },
    ]
    .into_iter()
    .chain(
        [
            BrowserInteraction::Dblclick,
            BrowserInteraction::Hover,
            BrowserInteraction::Focus,
            BrowserInteraction::Check,
            BrowserInteraction::Uncheck,
            BrowserInteraction::ScrollIntoView,
        ]
        .map(|operation| BrowserAction::Interact {
            operation,
            selector: "#control".into(),
            snapshot_after: true,
        }),
    ) {
        let mut queued = request(&model, surface, action);
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
        assert!(
            runtime
                .drain_requests(&model.snapshot(), now + Duration::from_millis(10))
                .requests
                .is_empty()
        );
    }
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
fn input_selectors_and_fill_text_are_bounded_before_mutation_dispatch() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let before = model.snapshot();
    let mut runtime = BrowserRuntime::default();
    attach(&mut runtime, &model, surface, now);
    let action = |selector: &str, text: String| BrowserAction::Fill {
        selector: selector.into(),
        text,
        snapshot_after: false,
    };
    for rejected in [
        BrowserAction::Click {
            selector: String::new(),
            snapshot_after: false,
        },
        BrowserAction::Click {
            selector: "x".repeat(MAX_SELECTOR_BYTES + 1),
            snapshot_after: false,
        },
        BrowserAction::Click {
            selector: "bad\0selector".into(),
            snapshot_after: false,
        },
        action("", String::new()),
        action("#entry", "x".repeat(MAX_INPUT_TEXT_BYTES + 1)),
        action("#entry", "bad\0text".into()),
    ] {
        assert!(matches!(
            runtime.admit(&before, request(&model, surface, rejected), now),
            Err(BrowserError::Invalid(_))
        ));
    }
    for operation in [
        BrowserInteraction::Dblclick,
        BrowserInteraction::Hover,
        BrowserInteraction::Focus,
        BrowserInteraction::Check,
        BrowserInteraction::Uncheck,
        BrowserInteraction::ScrollIntoView,
    ] {
        for selector in [
            String::new(),
            "x".repeat(MAX_SELECTOR_BYTES + 1),
            "bad\0selector".into(),
        ] {
            let action = BrowserAction::Interact {
                operation,
                selector,
                snapshot_after: false,
            };
            assert!(matches!(
                runtime.admit(&before, request(&model, surface, action), now),
                Err(BrowserError::Invalid(_))
            ));
        }
    }
    assert!(runtime.drain_requests(&before, now).requests.is_empty());
    for text in [
        String::new(),
        " quoted \"\n雪 ".into(),
        "x".repeat(MAX_INPUT_TEXT_BYTES),
    ] {
        let mut fill = request(&model, surface, action("#entry", text.clone()));
        assert!(!fill.action.is_read());
        fill.timeout_ms = 10;
        let id = runtime.admit(&before, fill, now).unwrap();
        let batch = runtime.drain_requests(&before, now);
        assert_eq!(batch.requests.len(), 1);
        assert!(
            matches!(&batch.requests[0].action, BrowserAction::Fill { text: actual, .. } if actual == &text)
        );
        assert!(runtime.drain_requests(&before, now).requests.is_empty());
        runtime.reconcile(&before, now + Duration::from_millis(10));
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
    assert_eq!(
        model.snapshot(),
        before,
        "admission must not change app selection or the document"
    );
}

#[test]
fn zoom_finite_validation_precedes_dispatch_and_expired_native_mutation_is_not_replayed() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            runtime.admit(
                &model.snapshot(),
                request(
                    &model,
                    surface,
                    BrowserAction::Zoom {
                        operation: BrowserZoom::Set { factor }
                    }
                ),
                now
            ),
            Err(BrowserError::Invalid(_))
        ));
    }
    assert!(
        runtime
            .drain_requests(&model.snapshot(), now)
            .requests
            .is_empty()
    );
    let mut zoom = request(
        &model,
        surface,
        BrowserAction::Zoom {
            operation: BrowserZoom::Set { factor: 1.25 },
        },
    );
    zoom.timeout_ms = 10;
    let id = runtime.admit(&model.snapshot(), zoom, now).unwrap();
    let batch = runtime.drain_requests(&model.snapshot(), now);
    assert_eq!(batch.requests.len(), 1);
    assert_eq!(batch.requests[0].owner, owner);
    assert!(matches!(
        batch.requests[0].action,
        BrowserAction::Zoom {
            operation: BrowserZoom::Set { factor: 1.25 }
        }
    ));
    let expired = now + Duration::from_millis(10);
    runtime.reconcile(&model.snapshot(), expired);
    let receipt = runtime.receipt(id).unwrap();
    assert_eq!(receipt.state, BrowserOperationState::Failed);
    assert!(receipt.error.as_ref().unwrap().outcome_unknown);
    assert!(
        runtime
            .drain_requests(&model.snapshot(), expired)
            .requests
            .is_empty()
    );
    assert!(matches!(
        runtime.host_event(
            &model.snapshot(),
            BrowserHostEvent::Complete {
                operation_id: id,
                owner,
                result: json!({"kind":"page_zoom","value":1.25})
            },
            expired
        ),
        Err(BrowserError::NotDispatched)
    ));
    assert!(runtime.receipt(id).unwrap().result.is_none());
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
            let snapshot = desktop_snapshot::read_until(&desktop, deadline);
            if predicate(&snapshot) {
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
                    profile_id: None,
                    focus: true,
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
    let closed = desktop_snapshot::read(&desktop);
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

#[test]
fn locator_admission_rejects_unbounded_and_conflicting_queries_before_dispatch() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    attach(&mut runtime, &model, surface, now);
    for locator in [
        json!({"kind":"role","query":"two tokens"}),
        json!({"kind":"text","query":"x".repeat(MAX_SELECTOR_BYTES + 1)}),
        json!({"kind":"label","query":"ok","name":"irrelevant"}),
        json!({"kind":"first","selector":"button","exact":true}),
        json!({"kind":"nth","selector":"button","index":4096}),
        json!({"kind":"nth","selector":"button"}),
    ] {
        let action = serde_json::from_value(json!({"action":"find","locator":locator})).unwrap();
        assert!(matches!(
            runtime.admit(&model.snapshot(), request(&model, surface, action), now),
            Err(BrowserError::Invalid(_))
        ));
        assert_eq!(runtime.pending_request_count(), 0);
    }
}

#[test]
fn admitted_wait_expires_or_cancels_on_navigation_without_uncertain_mutation() {
    let (mut model, surface) = fixture();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let mut wait = request(
        &model,
        surface,
        BrowserAction::Wait {
            selector: "#ready".into(),
            state: WaitState::Visible,
        },
    );
    wait.timeout_ms = 100;
    let expired = runtime.admit(&model.snapshot(), wait.clone(), now).unwrap();
    assert_eq!(
        runtime
            .drain_requests(&model.snapshot(), now)
            .requests
            .len(),
        1
    );
    runtime.reconcile(&model.snapshot(), now + Duration::from_millis(100));
    let error = runtime.receipt(expired).unwrap().error.as_ref().unwrap();
    assert_eq!(error.code, "browser_timeout");
    assert!(!error.outcome_unknown);
    let cancelled = runtime.admit(&model.snapshot(), wait, now).unwrap();
    runtime.drain_requests(&model.snapshot(), now);
    model
        .apply(Action::NavigateBrowser {
            surface_id: surface,
            url: "https://example.net/ready".into(),
        })
        .unwrap();
    runtime.reconcile(&model.snapshot(), now);
    assert_eq!(
        runtime.receipt(cancelled).unwrap().state,
        BrowserOperationState::Failed
    );
    assert!(
        !runtime
            .receipt(cancelled)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .outcome_unknown
    );
    assert!(
        runtime
            .host_event(
                &model.snapshot(),
                BrowserHostEvent::Complete {
                    operation_id: cancelled,
                    owner,
                    result: json!({"value":{"matched":true}})
                },
                now
            )
            .is_err()
    );
    assert_eq!(
        runtime.receipt(cancelled).unwrap().state,
        BrowserOperationState::Failed
    );
}

#[test]
fn dom_mutation_navigation_fails_original_owner_without_promoting_or_replaying_it() {
    let (model, surface) = fixture();
    let now = Instant::now();
    let before = model.snapshot();
    for operation in [
        "dblclick",
        "hover",
        "focus",
        "check",
        "uncheck",
        "scroll_into_view",
    ]
    .map(|operation| {
        json!({"action":"interact", "operation":operation,
        "selector":"#navigation-handler", "snapshot_after":true})
    })
    .into_iter()
    .chain([
        json!({"action":"type","selector":"#entry","text":"chunk"}),
        json!({"action":"keyboard","operation":"press","key":"Enter"}),
        json!({"action":"select","selector":"#choice","value":"a"}),
        json!({"action":"scroll","dx":1,"dy":2}),
    ]) {
        let mut runtime = BrowserRuntime::default();
        let owner = attach(&mut runtime, &model, surface, now);
        let action = serde_json::from_value(operation).unwrap();
        let id = runtime
            .admit(&before, request(&model, surface, action), now)
            .unwrap();
        assert_eq!(runtime.drain_requests(&before, now).requests.len(), 1);
        runtime
            .host_event(
                &before,
                BrowserHostEvent::DocumentChanged {
                    owner: owner.clone(),
                    document_generation: owner.document_generation + 1,
                    model_navigation: owner.model_navigation,
                    caused_by: Some(id),
                },
                now,
            )
            .unwrap();
        let receipt = runtime.receipt(id).unwrap();
        assert_eq!(receipt.state, BrowserOperationState::Failed);
        assert_eq!(
            receipt.owner, owner,
            "a DOM event is not an explicit navigation request"
        );
        let failure = receipt.error.as_ref().unwrap();
        assert!(failure.outcome_unknown);
        assert_eq!(failure.code, "stale_browser_owner");
        assert!(matches!(
            runtime.host_event(
                &before,
                BrowserHostEvent::Complete {
                    operation_id: id,
                    owner,
                    result: json!({"value":{"dispatched":true}}),
                },
                now
            ),
            Err(BrowserError::StaleOwner)
        ));
        assert!(runtime.drain_requests(&before, now).requests.is_empty());
    }
    assert_eq!(model.snapshot(), before);
    assert!(
        serde_json::from_value::<BrowserAction>(
            json!({"action":"interact", "operation":"press", "selector":"#button"})
        )
        .is_err()
    );
}

#[test]
fn edit_normalization_and_scroll_bounds_are_owned_before_native_dispatch() {
    let (model, surface) = fixture();
    let before = model.snapshot();
    let now = Instant::now();
    let mut runtime = BrowserRuntime::default();
    let owner = attach(&mut runtime, &model, surface, now);
    let make = |value| serde_json::from_value::<BrowserAction>(value).unwrap();
    for action in [
        make(json!({"action":"keyboard","operation":"press","key":""})),
        make(json!({"action":"keyboard","operation":"down","key":"x".repeat(129)})),
        make(json!({"action":"keyboard","operation":"up","key":format!("bad{}key",char::from(0))})),
        make(json!({"action":"type","selector":"#entry","text":"  "})),
        make(json!({"action":"select","selector":"#choice","value":""})),
        make(
            json!({"action":"type","selector":"#entry","text":"x".repeat(MAX_INPUT_TEXT_BYTES+1)}),
        ),
        make(
            json!({"action":"select","selector":"#choice","value":"x".repeat(MAX_INPUT_TEXT_BYTES+1)}),
        ),
        make(json!({"action":"scroll","dx":MAX_SCROLL_DELTA+1})),
        make(json!({"action":"scroll","dy":i64::MIN})),
        make(json!({"action":"scroll","selector":" "})),
        make(
            json!({"action":"type","selector":"#entry","text":format!("bad{}text",char::from(0))}),
        ),
        make(
            json!({"action":"select","selector":"#choice","value":format!("bad{}value",char::from(0))}),
        ),
    ] {
        assert!(matches!(
            runtime.admit(&before, request(&model, surface, action), now),
            Err(BrowserError::Invalid(_))
        ));
    }
    for action in [
        json!({"action":"scroll","dx":1.5}),
        json!({"action":"scroll","dy":"Infinity"}),
    ] {
        assert!(serde_json::from_value::<BrowserAction>(action).is_err());
    }
    assert!(runtime.drain_requests(&before, now).requests.is_empty());
    for (raw, expected) in [
        (
            json!({"action":"keyboard","operation":"press","key":" ","snapshot_after":true}),
            json!({"action":"keyboard","operation":"press","key":" ","snapshot_after":true}),
        ),
        (
            json!({"action":"type","selector":"#entry","text":"  雪  ","snapshot_after":true}),
            json!({"action":"type","selector":"#entry","text":"雪","snapshot_after":true}),
        ),
        (
            json!({"action":"select","selector":"#choice","value":"  value  "}),
            json!({"action":"select","selector":"#choice","value":"value","snapshot_after":false}),
        ),
        (
            json!({"action":"scroll","dx":MAX_SCROLL_DELTA,"dy":-MAX_SCROLL_DELTA}),
            json!({"action":"scroll","selector":null,"dx":MAX_SCROLL_DELTA,"dy":-MAX_SCROLL_DELTA,"snapshot_after":false}),
        ),
    ] {
        let id = runtime
            .admit(&before, request(&model, surface, make(raw)), now)
            .unwrap();
        let batch = runtime.drain_requests(&before, now);
        assert_eq!(batch.requests.len(), 1);
        assert_eq!(
            serde_json::to_value(&batch.requests[0].action).unwrap(),
            expected
        );
        assert!(runtime.drain_requests(&before, now).requests.is_empty());
        runtime
            .host_event(
                &before,
                BrowserHostEvent::Complete {
                    operation_id: id,
                    owner: owner.clone(),
                    result: json!({"value":{"dispatched":true}}),
                },
                now,
            )
            .unwrap();
    }
    assert_eq!(model.snapshot(), before);
}
