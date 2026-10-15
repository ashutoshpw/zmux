//! Protocol-owner checks for native notification desires; no OS delivery is simulated.
use uuid::Uuid;
use zmux_desktop_runtime::attention::*;
use zmux_workspace_core::{Action, RestoreLimits, SessionId, SurfaceId, WorkspaceStore};

fn fixture() -> (WorkspaceStore, SurfaceId) {
    let mut store = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    store
        .apply(Action::CreateWorkspace {
            name: "notification owner".into(),
        })
        .unwrap();
    let surface = store.snapshot().workspaces[0].root.panes()[0].selected_surface;
    (store, surface)
}
fn content(title: &str) -> NotificationContent {
    NotificationContent {
        title: title.into(),
        subtitle: "Agent".into(),
        body: "Ready for review".into(),
        desktop: None,
    }
}
fn authorize(
    attention: &mut Attention,
    store: &WorkspaceStore,
    generation: u64,
    state: NotificationAuthorization,
) {
    attention
        .native_event(
            &store.snapshot(),
            NativeNotificationEvent::Authorization {
                epoch: attention.epoch(),
                generation,
                state,
            },
        )
        .unwrap();
}
fn create(
    attention: &mut Attention,
    store: &WorkspaceStore,
    surface: SurfaceId,
    title: &str,
) -> NotificationTarget {
    let id = attention
        .notify_owned(&store.snapshot(), surface, "cli", &content(title), false)
        .unwrap();
    attention.find(id).unwrap().target.clone().unwrap()
}

#[test]
fn native_scheduling_requires_authorization_and_read_removes_the_scheduled_desire() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    let target = create(&mut attention, &store, surface, "Review");
    let before = attention.native_snapshot(&store.snapshot());
    assert_eq!(before.desired.len(), 1);
    assert_eq!(before.desired[0].authorization_generation, 0);
    assert_eq!(before.desired[0].subtitle, "Agent");
    assert_eq!(
        before.desired[0].identifier,
        format!(
            "zmux.notification.{}.{}",
            target.epoch, target.notification_id
        )
    );
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target: target.clone(),
                authorization_generation: 0
            }
        ),
        Err(AttentionError::StaleAuthorization)
    ));
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &target),
        Err(AttentionError::StaleAuthorization)
    ));
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    attention
        .native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target: target.clone(),
                authorization_generation: 1,
            },
        )
        .unwrap();
    assert_eq!(
        attention
            .find(target.notification_id)
            .unwrap()
            .delivery
            .state,
        DesktopDeliveryState::Scheduled
    );
    assert_eq!(
        attention.native_snapshot(&store.snapshot()).desired.len(),
        1,
        "scheduled unread banners remain desired"
    );
    attention.read(target.notification_id).unwrap();
    assert!(
        attention
            .native_snapshot(&store.snapshot())
            .desired
            .is_empty()
    );
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &target),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target,
                authorization_generation: 1
            }
        ),
        Err(AttentionError::StaleTarget)
    ));
}

#[test]
fn revocation_cancels_old_attempts_without_replaying_them_after_permission_returns() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    let old = create(&mut attention, &store, surface, "Before revocation");
    authorize(&mut attention, &store, 2, NotificationAuthorization::Denied);
    assert!(
        attention
            .native_snapshot(&store.snapshot())
            .desired
            .is_empty()
    );
    let retained = attention.find(old.notification_id).unwrap();
    assert!(!retained.read);
    assert_eq!(retained.delivery.state, DesktopDeliveryState::Suppressed);
    assert_eq!(
        attention
            .native_inbox_target(&store.snapshot(), &old)
            .unwrap(),
        surface
    );
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &old),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target: old.clone(),
                authorization_generation: 1
            }
        ),
        Err(AttentionError::StaleTarget)
    ));
    authorize(
        &mut attention,
        &store,
        3,
        NotificationAuthorization::Authorized,
    );
    assert!(
        attention
            .native_snapshot(&store.snapshot())
            .desired
            .is_empty(),
        "permission return must not replay old banners"
    );
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Failed {
                target: old.clone(),
                authorization_generation: 1,
                message: "late failure".into()
            }
        ),
        Err(AttentionError::StaleTarget)
    ));
    attention.read(old.notification_id).unwrap();
    assert_eq!(
        attention
            .native_inbox_target(&store.snapshot(), &old)
            .unwrap(),
        surface
    );
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &old),
        Err(AttentionError::StaleTarget)
    ));
    let fresh = create(&mut attention, &store, surface, "After permission return");
    // A routine authorized refresh does not revoke an already-authorized attempt.
    authorize(
        &mut attention,
        &store,
        4,
        NotificationAuthorization::Authorized,
    );
    assert_eq!(
        attention.native_snapshot(&store.snapshot()).desired[0].authorization_generation,
        3
    );
    attention
        .native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target: fresh,
                authorization_generation: 3,
            },
        )
        .unwrap();
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Authorization {
                epoch: attention.epoch(),
                generation: 2,
                state: NotificationAuthorization::Denied
            }
        ),
        Err(AttentionError::StaleAuthorization)
    ));
    assert_eq!(
        attention.native_snapshot(&store.snapshot()).authorization,
        NotificationAuthorization::Authorized
    );
}

#[test]
fn restored_surface_with_a_fresh_session_cannot_receive_an_old_notification_action() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    let target = create(&mut attention, &store, surface, "Original process");
    assert_eq!(
        attention
            .open_target(&store.snapshot(), target.notification_id)
            .unwrap(),
        surface
    );
    let document = store
        .session_document()
        .unwrap()
        .validate(RestoreLimits {
            terminals: 8,
            browsers: 4,
        })
        .unwrap();
    let restored = WorkspaceStore::prepare_session_restore(document, &[SessionId(10000)])
        .unwrap()
        .store;
    assert_eq!(
        restored.snapshot().workspaces[0].root.panes()[0].selected_surface,
        surface
    );
    assert!(attention.reconcile(&restored.snapshot()));
    assert!(
        attention
            .native_snapshot(&restored.snapshot())
            .desired
            .is_empty()
    );
    assert!(
        !attention.find(target.notification_id).unwrap().read,
        "unavailable rows remain visible in the inbox"
    );
    assert!(matches!(
        attention.open_target(&restored.snapshot(), target.notification_id),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_open_target(&restored.snapshot(), &target),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_event(
            &restored.snapshot(),
            NativeNotificationEvent::Scheduled {
                target,
                authorization_generation: 1
            }
        ),
        Err(AttentionError::StaleTarget)
    ));
}

#[test]
fn restore_epoch_fences_same_namespace_browser_targets_and_old_permission_callbacks() {
    let (mut store, _terminal) = fixture();
    let pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::CreateBrowser {
            pane_id: pane,
            url: "https://example.org/".into(),
            focus: true,
        })
        .unwrap();
    let surface = store.snapshot().workspaces[0].root.panes()[0].selected_surface;
    let mut attention = Attention::default();
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    let old = create(&mut attention, &store, surface, "Old browser");
    assert!(old.session_id.is_none());
    attention.reset_native("session restore");
    assert_ne!(attention.epoch(), old.epoch);
    assert!(
        attention
            .native_snapshot(&store.snapshot())
            .desired
            .is_empty()
    );
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &old),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_dismiss(&old),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Authorization {
                epoch: old.epoch,
                generation: 100,
                state: NotificationAuthorization::Authorized
            }
        ),
        Err(AttentionError::StaleAuthorization)
    ));
    let fresh = create(&mut attention, &store, surface, "Current browser");
    assert_eq!(fresh.epoch, attention.epoch());
    assert!(!attention.find(old.notification_id).unwrap().read);
}

#[test]
fn coalescing_and_cursor_clear_remove_obsolete_native_jobs_without_consuming_new_arrivals() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    let first = create(&mut attention, &store, surface, "Duplicate");
    let replacement = create(&mut attention, &store, surface, "Duplicate");
    assert_ne!(first.notification_id, replacement.notification_id);
    assert_eq!(attention.items.len(), 1);
    assert!(attention.read(first.notification_id).is_err());
    assert!(!attention.find(replacement.notification_id).unwrap().read);
    assert!(matches!(
        attention.native_open_target(&store.snapshot(), &first),
        Err(AttentionError::StaleTarget)
    ));
    assert!(matches!(
        attention.native_event(
            &store.snapshot(),
            NativeNotificationEvent::Scheduled {
                target: first,
                authorization_generation: 1
            }
        ),
        Err(AttentionError::StaleTarget)
    ));
    let later = create(&mut attention, &store, surface, "Later arrival");
    attention.clear(replacement.notification_id);
    let desired = attention.native_snapshot(&store.snapshot()).desired;
    assert_eq!(desired.len(), 1);
    assert_eq!(desired[0].target, later);
    assert!(matches!(
        attention.native_dismiss(&replacement),
        Err(AttentionError::StaleTarget)
    ));
    assert!(!attention.find(later.notification_id).unwrap().read);
}

#[test]
fn native_failures_focus_and_desktop_override_never_drop_the_in_app_notification() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    authorize(
        &mut attention,
        &store,
        1,
        NotificationAuthorization::Authorized,
    );
    let mut disabled = content("Desktop disabled");
    disabled.desktop = Some(false);
    let disabled = attention
        .notify_owned(&store.snapshot(), surface, "cli", &disabled, false)
        .unwrap();
    let focused = attention
        .notify_owned(&store.snapshot(), surface, "cli", &content("Focused"), true)
        .unwrap();
    let failed = create(&mut attention, &store, surface, "Schedule failure");
    attention
        .native_event(
            &store.snapshot(),
            NativeNotificationEvent::Failed {
                target: failed.clone(),
                authorization_generation: 1,
                message: "daemon unavailable".into(),
            },
        )
        .unwrap();
    assert!(
        attention
            .native_snapshot(&store.snapshot())
            .desired
            .is_empty()
    );
    assert!(!attention.find(disabled).unwrap().read);
    assert!(attention.find(focused).unwrap().read);
    assert_eq!(
        attention
            .find(failed.notification_id)
            .unwrap()
            .delivery
            .state,
        DesktopDeliveryState::Failed
    );
    assert_eq!(
        attention
            .find(failed.notification_id)
            .unwrap()
            .delivery
            .error
            .as_deref(),
        Some("daemon unavailable")
    );
    assert_eq!(
        attention.open_target(&store.snapshot(), disabled).unwrap(),
        surface,
        "desktop opt-out must not disable in-app open"
    );
    attention.native_dismiss(&failed).unwrap();
    assert!(attention.find(failed.notification_id).unwrap().read);
    assert!(!attention.find(disabled).unwrap().read);
}

#[test]
fn desired_native_snapshot_and_payloads_are_bounded_by_the_inbox_owner() {
    let (store, surface) = fixture();
    let mut attention = Attention::default();
    let oldest = create(&mut attention, &store, surface, "Unique 0");
    let mut newest = oldest.clone();
    for index in 1..131 {
        newest = create(&mut attention, &store, surface, &format!("Unique {index}"));
    }
    assert!(
        attention.read(oldest.notification_id).is_err(),
        "oldest row was evicted"
    );
    assert!(!attention.find(newest.notification_id).unwrap().read);
    assert_eq!(attention.items.len(), 128);
    assert_eq!(attention.dropped, 3);
    assert_eq!(
        attention.native_snapshot(&store.snapshot()).desired.len(),
        128
    );
    let payload = NotificationContent {
        title: "t".repeat(200),
        subtitle: "s".repeat(200),
        body: "b".repeat(3000),
        desktop: Some(true),
    };
    let id = attention
        .notify_owned(&store.snapshot(), surface, "cli", &payload, false)
        .unwrap();
    let notification = attention.find(id).unwrap();
    assert_eq!(notification.title.chars().count(), 128);
    assert_eq!(notification.subtitle.chars().count(), 128);
    assert_eq!(notification.body.chars().count(), 2048);
}

#[test]
fn initial_denial_and_daemon_unavailability_preserve_unread_in_app_records() {
    let (store, surface) = fixture();
    for (state, expected) in [
        (
            NotificationAuthorization::Denied,
            DesktopDeliveryState::Suppressed,
        ),
        (
            NotificationAuthorization::Unavailable,
            DesktopDeliveryState::Failed,
        ),
    ] {
        let mut attention = Attention::default();
        let target = create(&mut attention, &store, surface, "Before authorization");
        authorize(
            &mut attention,
            &store,
            1,
            NotificationAuthorization::NotDetermined,
        );
        assert_eq!(
            attention.native_snapshot(&store.snapshot()).desired[0].authorization_generation,
            0
        );
        authorize(&mut attention, &store, 2, state);
        assert!(
            attention
                .native_snapshot(&store.snapshot())
                .desired
                .is_empty()
        );
        let record = attention.find(target.notification_id).unwrap();
        assert_eq!(record.delivery.state, expected);
        assert!(!record.read);
        assert_eq!(
            attention
                .native_inbox_target(&store.snapshot(), &target)
                .unwrap(),
            surface
        );
    }
}
