//! Download-owner behavior; actual WebKit networking belongs to native acceptance.
use std::{
    fs,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{browser::BrowserOwner, downloads::*};
use zmux_workspace_core::{Action, SurfaceId, WorkspaceStore, targets::TargetKind};

fn fixture() -> (WorkspaceStore, SurfaceId, BrowserOwner, DownloadRuntime) {
    let mut model = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    model
        .apply(Action::CreateWorkspace {
            name: "downloads".into(),
        })
        .unwrap();
    let pane = model.snapshot().workspaces[0].focused_pane;
    model
        .apply(Action::CreateBrowser {
            pane_id: pane,
            profile_id: None,
            focus: true,
            url: "https://example.org".into(),
        })
        .unwrap();
    let surface = model.snapshot().workspaces[0].root.panes()[0].selected_surface;
    let owner = BrowserOwner {
        epoch: Uuid::new_v4(),
        identity_namespace: model.snapshot().identity_namespace,
        surface_id: surface,
        host_id: Uuid::new_v4(),
        document_generation: 1,
        model_navigation: 0,
    };
    (model, surface, owner, DownloadRuntime::default())
}
fn begin(
    runtime: &mut DownloadRuntime,
    model: &WorkspaceStore,
    owner: &BrowserOwner,
) -> DownloadToken {
    let token = DownloadToken {
        epoch: runtime.epoch,
        download_id: Uuid::new_v4(),
        host_id: owner.host_id,
    };
    runtime
        .begin(
            &model.snapshot(),
            owner.clone(),
            token,
            "example.bin".into(),
            Instant::now(),
        )
        .unwrap()
}
fn saved(runtime: &mut DownloadRuntime, token: &DownloadToken) {
    runtime
        .update(NativeDownload::Destination {
            token: token.clone(),
            filename: "example.bin".into(),
        })
        .unwrap();
    runtime
        .update(NativeDownload::Finalizing {
            token: token.clone(),
        })
        .unwrap();
    runtime
        .update(NativeDownload::Saved {
            token: token.clone(),
            path: "/example.bin".into(),
            bytes: 7,
            cleanup_complete: true,
            message: None,
        })
        .unwrap();
}
#[test]
fn navigation_preserves_origin_close_cancels_exact_host_and_late_callbacks_are_fenced() {
    let (mut model, surface, owner, mut runtime) = fixture();
    let token = begin(&mut runtime, &model, &owner);
    model
        .apply(Action::NavigateBrowser {
            surface_id: surface,
            url: "https://example.net".into(),
        })
        .unwrap();
    runtime.poll(&model.snapshot(), Instant::now());
    assert_eq!(
        runtime.records()[0].status,
        DownloadState::AwaitingDestination
    );
    assert_eq!(runtime.records()[0].origin.browser, owner);
    runtime.detach(Uuid::new_v4());
    assert_eq!(
        runtime.records()[0].status,
        DownloadState::AwaitingDestination
    );
    model
        .apply(Action::CloseSurface {
            surface_id: surface,
        })
        .unwrap();
    runtime.poll(&model.snapshot(), Instant::now());
    assert_eq!(runtime.records()[0].status, DownloadState::Cancelling);
    runtime
        .update(NativeDownload::Cancelled {
            token: token.clone(),
            cleanup_complete: true,
        })
        .unwrap();
    assert!(
        runtime
            .update(NativeDownload::Destination {
                token: token.clone(),
                filename: "late".into()
            })
            .is_err()
    );
    assert!(
        runtime
            .update(NativeDownload::Cancelled {
                token,
                cleanup_complete: true
            })
            .is_err()
    );
}
#[test]
fn finish_reservation_wins_once_and_unowned_callbacks_cannot_mutate_it() {
    let (model, surface, owner, mut runtime) = fixture();
    let token = begin(&mut runtime, &model, &owner);
    runtime
        .update(NativeDownload::Destination {
            token: token.clone(),
            filename: "example.bin".into(),
        })
        .unwrap();
    let mut forged = token.clone();
    forged.host_id = Uuid::new_v4();
    assert!(
        runtime
            .update(NativeDownload::Progress {
                token: forged,
                bytes: 99,
                total_bytes: None
            })
            .is_err()
    );
    runtime
        .update(NativeDownload::Finalizing {
            token: token.clone(),
        })
        .unwrap();
    assert_eq!(
        runtime
            .cancel(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                token.download_id
            )
            .unwrap_err()
            .code,
        "download_too_late"
    );
    assert!(
        runtime
            .update(NativeDownload::Cancelled {
                token: token.clone(),
                cleanup_complete: true
            })
            .is_err()
    );
    runtime
        .update(NativeDownload::Saved {
            token,
            path: "/example.bin".into(),
            bytes: 7,
            cleanup_complete: true,
            message: None,
        })
        .unwrap();
    assert_eq!(runtime.records()[0].bytes, 7);
}
#[test]
fn terminal_events_assign_once_tickets_are_repeatable_and_list_does_not_consume() {
    let (model, surface, owner, mut runtime) = fixture();
    let token = begin(&mut runtime, &model, &owner);
    saved(&mut runtime, &token);
    assert_eq!(
        runtime
            .list(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                None
            )
            .unwrap(),
        runtime
            .list(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                None
            )
            .unwrap()
    );
    let first = runtime
        .wait(&model.snapshot(), surface, None, None, Instant::now())
        .unwrap();
    assert_eq!(first.state, "completed");
    let next = runtime
        .wait(&model.snapshot(), surface, None, None, Instant::now())
        .unwrap();
    assert_eq!(next.state, "pending");
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(runtime.status(first.epoch, first.ticket_id).unwrap()).unwrap()
    );
    assert!(runtime.status(Uuid::new_v4(), first.ticket_id).is_err());
    assert_eq!(
        runtime
            .list(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                None
            )
            .unwrap()["count"],
        1
    );
}
#[test]
fn cancellation_keeps_physical_capacity_until_cleanup_acknowledgement() {
    let (model, surface, owner, mut runtime) = fixture();
    let first = begin(&mut runtime, &model, &owner);
    let second = begin(&mut runtime, &model, &owner);
    runtime
        .cancel(
            model.snapshot().object_uuid(TargetKind::Surface, surface.0),
            first.download_id,
        )
        .unwrap();
    let candidate = DownloadToken {
        epoch: runtime.epoch,
        download_id: Uuid::new_v4(),
        host_id: owner.host_id,
    };
    assert_eq!(
        runtime
            .begin(
                &model.snapshot(),
                owner.clone(),
                candidate.clone(),
                "third".into(),
                Instant::now()
            )
            .unwrap_err()
            .code,
        "download_capacity"
    );
    runtime
        .update(NativeDownload::Cancelled {
            token: first,
            cleanup_complete: false,
        })
        .unwrap();
    assert!(
        runtime
            .begin(
                &model.snapshot(),
                owner.clone(),
                candidate.clone(),
                "third".into(),
                Instant::now()
            )
            .is_err()
    );
    runtime
        .update(NativeDownload::Cancelled {
            token: second,
            cleanup_complete: true,
        })
        .unwrap();
    runtime
        .begin(
            &model.snapshot(),
            owner,
            candidate,
            "third".into(),
            Instant::now(),
        )
        .unwrap();
}
#[test]
fn history_eviction_cannot_rewrite_a_promised_ticket_result() {
    let (model, surface, owner, mut runtime) = fixture();
    let first = begin(&mut runtime, &model, &owner);
    saved(&mut runtime, &first);
    let receipt = runtime
        .wait(&model.snapshot(), surface, None, None, Instant::now())
        .unwrap();
    for _ in 0..30 {
        let token = begin(&mut runtime, &model, &owner);
        saved(&mut runtime, &token);
    }
    assert_eq!(
        runtime
            .list(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                None
            )
            .unwrap()["count"],
        25
    );
    assert!(!runtime.records().iter().any(|r| r.token == first));
    assert_eq!(
        runtime
            .status(receipt.epoch, receipt.ticket_id)
            .unwrap()
            .result
            .unwrap()["download"]["download_id"],
        first.download_id.to_string()
    );
}
struct Files(std::path::PathBuf);
impl Files {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zmux-downloads-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("download fixture cleanup: {error}");
        }
    }
}
fn await_ticket(
    runtime: &mut DownloadRuntime,
    model: &WorkspaceStore,
    receipt: &WaitReceipt,
) -> WaitReceipt {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        runtime.poll(&model.snapshot(), Instant::now());
        let result = runtime.status(receipt.epoch, receipt.ticket_id).unwrap();
        if result.state != "pending" {
            return result;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn path_observation_is_nonconsuming_and_reports_empty_missing_parent_and_existing_paths() {
    let files = Files::new();
    let (model, surface, owner, mut runtime) = fixture();
    let token = begin(&mut runtime, &model, &owner);
    saved(&mut runtime, &token);
    let path = files.0.join("observed");
    fs::write(&path, b"separate file").unwrap();
    let receipt = runtime
        .wait(
            &model.snapshot(),
            surface,
            Some(path.to_str().unwrap().into()),
            None,
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        await_ticket(&mut runtime, &model, &receipt).result.unwrap()["downloaded"],
        true
    );
    let event = runtime
        .wait(&model.snapshot(), surface, None, None, Instant::now())
        .unwrap();
    assert_eq!(
        event.result.unwrap()["download"]["download_id"],
        token.download_id.to_string()
    );
    fs::write(&path, b"").unwrap();
    let empty = runtime
        .wait(
            &model.snapshot(),
            surface,
            Some(path.to_str().unwrap().into()),
            Some(5),
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        await_ticket(&mut runtime, &model, &empty).error.as_deref(),
        Some("timeout")
    );
    let missing = runtime
        .wait(
            &model.snapshot(),
            surface,
            Some(files.0.join("absent/leaf").to_str().unwrap().into()),
            None,
            Instant::now(),
        )
        .unwrap();
    assert_eq!(
        await_ticket(&mut runtime, &model, &missing)
            .error
            .as_deref(),
        Some("download_path_watch_failed")
    );
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .unwrap();
}
#[test]
fn completed_metadata_cannot_succeed_after_deadline_even_with_stale_poll_instant() {
    let files = Files::new();
    let path = files.0.join("observed");
    fs::write(&path, b"x").unwrap();
    let (model, surface, _, mut runtime) = fixture();
    let now = Instant::now();
    let receipt = runtime
        .wait(
            &model.snapshot(),
            surface,
            Some(path.to_str().unwrap().into()),
            Some(20),
            now,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    runtime.poll(&model.snapshot(), now);
    assert_eq!(
        runtime
            .status(receipt.epoch, receipt.ticket_id)
            .unwrap()
            .error
            .as_deref(),
        Some("timeout")
    );
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .unwrap();
}
#[test]
fn observer_shutdown_surfaces_worker_fault_instead_of_stranding_waiters() {
    let files = Files::new();
    let (model, surface, _, mut runtime) = fixture();
    let receipt = runtime
        .wait(
            &model.snapshot(),
            surface,
            Some(files.0.join("absent").to_str().unwrap().into()),
            None,
            Instant::now(),
        )
        .unwrap();
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .unwrap();
    runtime.poll(&model.snapshot(), Instant::now());
    let result = runtime.status(receipt.epoch, receipt.ticket_id).unwrap();
    assert_eq!(result.state, "failed");
    assert_eq!(
        result.error.as_deref(),
        Some("download path observer stopped")
    );
}

#[test]
fn reused_numeric_surface_after_restore_cannot_inherit_history_cancel_or_wait_events() {
    let (model, surface, owner, mut runtime) = fixture();
    let old = begin(&mut runtime, &model, &owner);
    let old_wait = runtime
        .wait(&model.snapshot(), surface, None, None, Instant::now())
        .unwrap();
    let mut replacement = model.snapshot();
    replacement.identity_namespace = Uuid::new_v4();
    let new_scope = replacement.object_uuid(TargetKind::Surface, surface.0);
    assert!(runtime.cancel(new_scope, old.download_id).is_err());
    assert_eq!(runtime.list(new_scope, None).unwrap()["count"], 0);
    let new_wait = runtime
        .wait(&replacement, surface, None, None, Instant::now())
        .unwrap();
    let mut new_owner = owner.clone();
    new_owner.identity_namespace = replacement.identity_namespace;
    new_owner.host_id = Uuid::new_v4();
    let new_token = DownloadToken {
        epoch: runtime.epoch,
        host_id: new_owner.host_id,
        download_id: Uuid::new_v4(),
    };
    runtime
        .begin(
            &replacement,
            new_owner,
            new_token.clone(),
            "new.bin".into(),
            Instant::now(),
        )
        .unwrap();
    saved(&mut runtime, &new_token);
    assert_eq!(
        runtime
            .status(old_wait.epoch, old_wait.ticket_id)
            .unwrap()
            .state,
        "pending"
    );
    assert_eq!(
        runtime
            .status(new_wait.epoch, new_wait.ticket_id)
            .unwrap()
            .result
            .unwrap()["download"]["download_id"],
        new_token.download_id.to_string()
    );
    runtime.poll(&replacement, Instant::now());
    runtime
        .update(NativeDownload::Cancelled {
            token: old.clone(),
            cleanup_complete: true,
        })
        .unwrap();
    assert_eq!(
        runtime
            .status(old_wait.epoch, old_wait.ticket_id)
            .unwrap()
            .result
            .unwrap()["download"]["download_id"],
        old.download_id.to_string()
    );
}

#[test]
fn elapsed_path_tickets_hold_worker_credits_until_real_completion_is_drained() {
    let files = Files::new();
    let (model, surface, _, mut runtime) = fixture();
    let path = Some(files.0.join("absent").to_str().unwrap().to_owned());
    let now = Instant::now();
    for _ in 0..MAX_WAITERS {
        runtime
            .wait(&model.snapshot(), surface, path.clone(), Some(1), now)
            .unwrap();
    }
    // Elapsed deadlines alone never authorize reusing physical worker credits.
    // Even already-enqueued completions remain charged until actually drained.
    let far_future = now + Duration::from_secs(1);
    assert_eq!(
        runtime
            .wait(
                &model.snapshot(),
                surface,
                path.clone(),
                Some(1),
                far_future
            )
            .unwrap_err()
            .code,
        "download_wait_capacity"
    );
    std::thread::sleep(Duration::from_millis(100));
    runtime.poll(&model.snapshot(), far_future);
    runtime
        .wait(&model.snapshot(), surface, None, Some(1), Instant::now())
        .unwrap();
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .unwrap();
}

#[test]
fn generic_json_cannot_forge_native_lifecycle_or_bypass_browser_validation() {
    let native = serde_json::json!({"command":"native_download","request":{"operation":"drain"}});
    assert!(serde_json::from_value::<zmux_desktop_runtime::Command>(native.clone()).is_err());
    assert!(
        serde_json::from_value::<zmux_desktop_runtime::control::ControlCommand>(native).is_err()
    );
    let (model, surface, mut owner, mut runtime) = fixture();
    owner.identity_namespace = Uuid::new_v4();
    let token = DownloadToken {
        epoch: runtime.epoch,
        host_id: owner.host_id,
        download_id: Uuid::new_v4(),
    };
    assert!(
        runtime
            .begin(
                &model.snapshot(),
                owner,
                token,
                "native.bin".into(),
                Instant::now()
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .list(
                model.snapshot().object_uuid(TargetKind::Surface, surface.0),
                None
            )
            .unwrap()["count"],
        0
    );
}
