use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    Command, Desktop, Request, Shell, Snapshot,
    attention::*,
    targets::{TargetOperation, TargetRequest},
};
use zmux_workspace_core::{
    Action, SurfaceId, WorkspaceStore,
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
        app.wait(|snapshot| !snapshot.model.workspaces.is_empty());
        app
    }
    fn wait(&self, predicate: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(snapshot) = self.desktop.snapshot()
                && predicate(&snapshot)
            {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "desktop notification operation did not settle"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn send(&mut self, command: Command) -> Arc<Snapshot> {
        self.next += 1;
        self.desktop
            .submit(Request {
                version: 1,
                request_id: self.next,
                payload: command,
            })
            .unwrap();
        self.wait(|snapshot| snapshot.last_request_id == self.next)
    }
    fn action(&mut self, action: Action) -> Arc<Snapshot> {
        let snapshot = self.send(Command::Action { action });
        assert!(snapshot.last_error.is_none(), "{:?}", snapshot.last_error);
        snapshot
    }
    fn notify(&mut self, surface_id: SurfaceId, title: &str) -> Arc<Snapshot> {
        let snapshot = self.send(Command::Attention {
            action: AttentionCommand::Notify {
                surface_id,
                title: title.into(),
                subtitle: String::new(),
                body: String::new(),
                desktop: None,
            },
        });
        assert!(snapshot.last_error.is_none());
        snapshot
    }
    fn scoped(
        &mut self,
        scope: TargetScope,
        caller: CallerContext,
        action: NotificationAction,
    ) -> Arc<Snapshot> {
        self.send(Command::Targeted {
            request: TargetRequest {
                scope,
                caller,
                expected_model_revision: None,
                operation: TargetOperation::Notifications { action },
            },
        })
    }
    fn close(self) {
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}
fn workspace(id: u64) -> TargetScope {
    TargetScope {
        workspace: Some(format!("workspace:{id}").parse().unwrap()),
        ..Default::default()
    }
}

#[test]
fn notification_uuid_handles_open_read_and_dismiss_the_exact_admitted_generation() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let first = initial.model.workspaces[0].clone();
    let surface = first.root.panes()[0].selected_surface;
    app.action(Action::CreateWorkspace {
        name: "Keep foreground".into(),
    });
    let created = app.notify(surface, "Handle owner");
    let row = created.notifications.last().unwrap();
    let id = row.id;
    let wire = serde_json::to_value(row).unwrap();
    let uuid = wire["uuid"]
        .as_str()
        .expect("admission must expose a UUID handle");
    assert!(Uuid::parse_str(uuid).is_ok());
    assert_eq!(wire["reference"], format!("notification:{uuid}"));
    let action =
        serde_json::from_value(serde_json::json!({"operation":"open", "id":uuid})).unwrap();
    let opened = app.send(Command::Attention { action });
    assert!(opened.last_error.is_none());
    assert_eq!(opened.model.focused_workspace, Some(first.id));
    assert!(
        opened
            .notifications
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .read
    );
    let action = serde_json::from_value(
        serde_json::json!({"operation":"read", "id":format!("notification:{uuid}")}),
    )
    .unwrap();
    assert!(app.send(Command::Attention { action }).last_error.is_none());
    let action =
        serde_json::from_value(serde_json::json!({"operation":"dismiss", "id":uuid})).unwrap();
    let dismissed = app.send(Command::Attention { action });
    assert!(dismissed.last_error.is_none());
    assert!(dismissed.notifications.iter().all(|row| row.id != id));
    let action =
        serde_json::from_value(serde_json::json!({"operation":"open", "id":uuid})).unwrap();
    let missing = app.send(Command::Attention { action });
    assert!(missing.last_error.is_some());
    assert_eq!(missing.model.focused_workspace, Some(first.id));
    app.close();
}

#[test]
fn jump_to_unread_skips_stale_newer_rows_and_acknowledges_only_the_selected_live_owner() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let first = initial.model.workspaces[0].clone();
    let oldest = app.notify(first.root.panes()[0].selected_surface, "Oldest reachable");
    let oldest_id = oldest.notifications.last().unwrap().id;
    let workspace_owner = app.action(Action::CreateWorkspace {
        name: "Newest live workspace".into(),
    });
    let live_workspace = workspace_owner.model.workspaces.last().unwrap().id;
    let live = app.send(Command::Targeted {
        request: TargetRequest {
            scope: workspace(live_workspace.0),
            caller: CallerContext::default(),
            expected_model_revision: None,
            operation: TargetOperation::Notify {
                title: "Newest reachable".into(),
                subtitle: String::new(),
                body: String::new(),
                desktop: Some(false),
            },
        },
    });
    let live_id = live.notifications.last().unwrap().id;
    let other = app.action(Action::CreateWorkspace {
        name: "Will close".into(),
    });
    let second = other.model.workspaces.last().unwrap().clone();
    let stale = app.notify(
        second.root.panes()[0].selected_surface,
        "Newest closed owner",
    );
    let stale_id = stale.notifications.last().unwrap().id;
    app.action(Action::CloseWorkspace {
        workspace_id: second.id,
    });
    let foreground = app.action(Action::CreateWorkspace {
        name: "Foreground".into(),
    });
    let foreground = foreground.model.focused_workspace;
    let action = serde_json::from_value(serde_json::json!({"action":"jump_to_unread"})).unwrap();
    let result = app
        .desktop
        .native_notification_action(action)
        .expect("shared unread navigation must be supported");
    let receipt = serde_json::to_value(result).unwrap();
    assert_eq!(receipt["navigation"]["opened"], true);
    assert_eq!(receipt["navigation"]["notification_id"], live_id);
    assert_eq!(receipt["focused_workspace_id"], live_workspace.0);
    let opened = app.desktop.snapshot().unwrap();
    assert_ne!(opened.model.focused_workspace, foreground);
    assert!(
        opened
            .notifications
            .iter()
            .find(|row| row.id == live_id)
            .unwrap()
            .read
    );
    assert!(
        !opened
            .notifications
            .iter()
            .find(|row| row.id == oldest_id)
            .unwrap()
            .read
    );
    assert!(
        !opened
            .notifications
            .iter()
            .find(|row| row.id == stale_id)
            .unwrap()
            .read
    );
    let action = serde_json::from_value(serde_json::json!({"action":"jump_to_unread"})).unwrap();
    let next = app.desktop.native_notification_action(action).unwrap();
    assert_eq!(
        serde_json::to_value(next).unwrap()["navigation"]["notification_id"],
        oldest_id
    );
    let last_opened = app.desktop.snapshot().unwrap();
    assert_eq!(last_opened.model.focused_workspace, Some(first.id));
    let action = serde_json::from_value(serde_json::json!({"action":"jump_to_unread"})).unwrap();
    let no_candidate = app.desktop.native_notification_action(action).unwrap();
    assert_eq!(
        serde_json::to_value(no_candidate).unwrap()["navigation"]["opened"],
        false
    );
    let unchanged = app.desktop.snapshot().unwrap();
    assert_eq!(
        unchanged.model.focused_workspace,
        last_opened.model.focused_workspace
    );
    assert!(
        !unchanged
            .notifications
            .iter()
            .find(|row| row.id == stale_id)
            .unwrap()
            .read
    );
    app.close();
}

#[test]
fn notification_creation_preserves_workspace_intent_and_uses_only_proven_caller_surfaces() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let first = initial.model.workspaces[0].clone();
    let caller_surface = first.root.panes()[0].selected_surface;
    app.action(Action::CreateSurface {
        pane_id: first.focused_pane,
    });
    let added = app.action(Action::CreateWorkspace {
        name: "Other destination".into(),
    });
    let other = added.model.workspaces.last().unwrap().clone();
    let other_surface = other.root.panes()[0].selected_surface;
    let caller = |workspace_id: u64, surface_id: Option<u64>| CallerContext {
        workspace: Some(format!("workspace:{workspace_id}").parse().unwrap()),
        surface: surface_id.map(|id| format!("surface:{id}").parse().unwrap()),
    };
    for (name, scope, context, expected_workspace, expected_surface) in [
        (
            "Explicit workspace",
            workspace(first.id.0),
            CallerContext::default(),
            first.id,
            None,
        ),
        (
            "Same workspace caller",
            workspace(first.id.0),
            caller(first.id.0, Some(caller_surface.0)),
            first.id,
            Some(caller_surface.0),
        ),
        (
            "Foreign caller cannot override workspace",
            workspace(first.id.0),
            caller(other.id.0, Some(other_surface.0)),
            first.id,
            None,
        ),
        (
            "External caller",
            TargetScope::default(),
            CallerContext::default(),
            other.id,
            None,
        ),
        (
            "Workspace caller",
            TargetScope::default(),
            caller(first.id.0, None),
            first.id,
            None,
        ),
        (
            "Stable surface wins stale workspace",
            TargetScope::default(),
            caller(999_999, Some(caller_surface.0)),
            first.id,
            Some(caller_surface.0),
        ),
        (
            "Stale surface retains proven workspace",
            TargetScope::default(),
            caller(first.id.0, Some(999_999)),
            first.id,
            None,
        ),
    ] {
        let snapshot = app.send(Command::Targeted {
            request: TargetRequest {
                scope,
                caller: context,
                expected_model_revision: None,
                operation: TargetOperation::Notify {
                    title: name.into(),
                    subtitle: String::new(),
                    body: String::new(),
                    desktop: Some(false),
                },
            },
        });
        assert!(
            snapshot.last_error.is_none(),
            "{name}: {:?}",
            snapshot.last_error
        );
        let row = snapshot.notifications.last().unwrap();
        assert_eq!(row.workspace_id, expected_workspace, "{name}");
        let wire = serde_json::to_value(row).unwrap();
        assert_eq!(
            wire["surface_id"].as_u64(),
            expected_surface,
            "{name} must not borrow the selected surface"
        );
        assert_eq!(
            snapshot.model.focused_workspace,
            Some(other.id),
            "creation changed focus"
        );
    }
    let count = app.desktop.snapshot().unwrap().notifications.len();
    for (name, scope, context) in [
        (
            "Invalid explicit workspace",
            workspace(999_999),
            caller(first.id.0, Some(caller_surface.0)),
        ),
        (
            "Stale caller cannot guess focus",
            TargetScope::default(),
            caller(999_999, Some(999_999)),
        ),
        (
            "Explicit surface cannot cross workspace",
            TargetScope {
                surface: Some(format!("surface:{}", other_surface.0).parse().unwrap()),
                ..workspace(first.id.0)
            },
            CallerContext::default(),
        ),
    ] {
        let snapshot = app.send(Command::Targeted {
            request: TargetRequest {
                scope,
                caller: context,
                expected_model_revision: None,
                operation: TargetOperation::Notify {
                    title: name.into(),
                    subtitle: String::new(),
                    body: String::new(),
                    desktop: None,
                },
            },
        });
        assert!(snapshot.last_error.is_some(), "{name}");
        assert_eq!(snapshot.notifications.len(), count, "{name}");
        assert_eq!(snapshot.model.focused_workspace, Some(other.id), "{name}");
    }
    app.close();
}

#[test]
fn active_workspace_acknowledges_workspace_rows_without_reading_hidden_surface_rows() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let first = initial.model.workspaces[0].clone();
    let hidden = first.root.panes()[0].selected_surface;
    app.action(Action::CreateSurface {
        pane_id: first.focused_pane,
    });
    app.notify(hidden, "Hidden surface");
    let created = app.send(Command::Targeted {
        request: TargetRequest {
            scope: workspace(first.id.0),
            caller: CallerContext::default(),
            expected_model_revision: None,
            operation: TargetOperation::Notify {
                title: "Workspace unread".into(),
                subtitle: String::new(),
                body: String::new(),
                desktop: None,
            },
        },
    });
    let workspace_row = created.notifications.last().unwrap().id;
    assert!(created.notifications.iter().all(|row| !row.read));
    let active = app.send(Command::Attention {
        action: AttentionCommand::AppActive { active: true },
    });
    assert!(
        active
            .notifications
            .iter()
            .find(|row| row.id == workspace_row)
            .unwrap()
            .read
    );
    assert!(
        !active
            .notifications
            .iter()
            .find(|row| row.surface_id == Some(hidden))
            .unwrap()
            .read
    );
    let current = app.send(Command::Targeted {
        request: TargetRequest {
            scope: workspace(first.id.0),
            caller: CallerContext::default(),
            expected_model_revision: None,
            operation: TargetOperation::Notify {
                title: "Workspace already visible".into(),
                subtitle: String::new(),
                body: String::new(),
                desktop: None,
            },
        },
    });
    assert!(current.notifications.last().unwrap().read);
    let cleared = app.scoped(
        TargetScope {
            surface: Some(format!("surface:{}", hidden.0).parse().unwrap()),
            ..workspace(first.id.0)
        },
        CallerContext::default(),
        NotificationAction::Clear {
            scope: NotificationScope::Surface,
            through_id: None,
            expected_epoch: None,
        },
    );
    assert!(cleared.last_error.is_none());
    assert_eq!(
        cleared.notifications.len(),
        2,
        "surface clear must preserve workspace rows"
    );
    assert!(
        cleared
            .notifications
            .iter()
            .all(|row| row.surface_id.is_none())
    );
    app.close();
}

#[test]
fn workspace_notification_open_survives_selected_surface_close_and_preserves_workspace_tab() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let first = initial.model.workspaces[0].clone();
    let original = first.root.panes()[0].selected_surface;
    let extra = app.action(Action::CreateSurface {
        pane_id: first.focused_pane,
    });
    let selected_at_creation = extra.model.workspaces[0].root.panes()[0].selected_surface;
    let added = app.action(Action::CreateWorkspace {
        name: "Keep focus".into(),
    });
    let other = added.model.workspaces.last().unwrap().id;
    let created = app.send(Command::Targeted {
        request: TargetRequest {
            scope: workspace(first.id.0),
            caller: CallerContext::default(),
            expected_model_revision: None,
            operation: TargetOperation::Notify {
                title: "Workspace task".into(),
                subtitle: String::new(),
                body: String::new(),
                desktop: Some(false),
            },
        },
    });
    let row = created.notifications.last().unwrap();
    assert!(
        serde_json::to_value(row).unwrap()["surface_id"].is_null(),
        "workspace intent requires a null surface, not a mutable selected tab"
    );
    let target = row.target.clone().unwrap();
    let id = row.id;
    app.action(Action::CloseSurface {
        surface_id: selected_at_creation,
    });
    let opened = app
        .desktop
        .native_notification_action(NativeAttentionAction::OpenInbox { target })
        .unwrap();
    let receipt = serde_json::to_value(opened).unwrap();
    assert_eq!(receipt["focused_workspace_id"], first.id.0);
    let state = app.desktop.snapshot().unwrap();
    assert_ne!(state.model.focused_workspace, Some(other));
    assert_eq!(state.model.focused_workspace, Some(first.id));
    assert_eq!(
        state.model.workspaces[0].root.panes()[0].selected_surface,
        original
    );
    assert!(
        state
            .notifications
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .read
    );
    app.close();
}

#[test]
fn moved_then_closed_surface_notifications_remain_scoped_to_the_last_live_workspace() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let source = &initial.model.workspaces[0];
    let source_workspace = source.id;
    let source_surface = source.root.panes()[0].selected_surface;
    app.action(Action::CreateSurface {
        pane_id: source.focused_pane,
    });
    let destination = app.action(Action::CreateWorkspace {
        name: "destination".into(),
    });
    let destination = destination.model.workspaces.last().unwrap().clone();
    let destination_surface = destination.root.panes()[0].selected_surface;
    let notified = app.notify(source_surface, "Moved terminal");
    let original = notified.notifications.last().unwrap().clone();
    app.notify(destination_surface, "Destination terminal");
    let moved = app.action(Action::MoveSurface {
        surface_id: source_surface,
        pane_id: destination.focused_pane,
        index: 1,
    });
    let moved_row = moved
        .notifications
        .iter()
        .find(|row| row.id == original.id)
        .unwrap();
    assert_eq!(moved_row.workspace_id, destination.id);
    assert_eq!(
        moved_row.target, original.target,
        "moving a terminal must retain its native callback token"
    );
    app.action(Action::CloseSurface {
        surface_id: source_surface,
    });
    let source_read = app.scoped(
        workspace(source_workspace.0),
        CallerContext::default(),
        NotificationAction::MarkRead {
            selection: NotificationSelection::Workspace,
        },
    );
    assert!(source_read.last_error.is_none());
    assert!(
        !source_read
            .notifications
            .iter()
            .find(|row| row.id == original.id)
            .unwrap()
            .read
    );
    let before_focus = source_read.model.focused_workspace;
    let destination_read = app.scoped(
        workspace(destination.id.0),
        CallerContext::default(),
        NotificationAction::MarkRead {
            selection: NotificationSelection::Workspace,
        },
    );
    assert!(destination_read.last_error.is_none());
    assert!(destination_read.notifications.iter().all(|row| row.read));
    assert_eq!(
        destination_read.model.focused_workspace, before_focus,
        "bulk acknowledgement must not change focus"
    );
    assert_eq!(
        destination_read
            .notifications
            .iter()
            .find(|row| row.id == original.id)
            .unwrap()
            .workspace_id,
        destination.id
    );
    app.close();
}

#[test]
fn global_history_operations_work_without_a_live_workspace_or_valid_caller() {
    let mut app = App::new();
    let initial = app.desktop.snapshot().unwrap();
    let workspace_id = initial.model.workspaces[0].id;
    let surface = initial.model.workspaces[0].root.panes()[0].selected_surface;
    let notified = app.notify(surface, "Before closing desktop");
    let cutoff = notified.notification_cursor;
    app.action(Action::CloseWorkspace { workspace_id });
    let stale = CallerContext {
        workspace: Some("workspace:999999".parse().unwrap()),
        surface: Some("surface:999999".parse().unwrap()),
    };
    let acknowledged = app.scoped(
        TargetScope::default(),
        stale.clone(),
        NotificationAction::MarkRead {
            selection: NotificationSelection::All,
        },
    );
    assert!(acknowledged.last_error.is_none());
    assert!(acknowledged.model.workspaces.is_empty());
    assert!(acknowledged.notifications[0].read);
    let dismissed = app.scoped(
        TargetScope::default(),
        stale.clone(),
        NotificationAction::Dismiss {
            selection: DismissSelection::AllRead,
        },
    );
    assert!(dismissed.last_error.is_none());
    assert!(dismissed.notifications.is_empty());
    assert_eq!(
        dismissed.notification_cursor, cutoff,
        "clearing history must not rewind its cursor"
    );
    let cleared = app.scoped(
        TargetScope::default(),
        stale,
        NotificationAction::Clear {
            scope: NotificationScope::All,
            through_id: Some(0),
            expected_epoch: None,
        },
    );
    assert!(cleared.last_error.is_none());
    assert_eq!(cleared.notification_cursor, cutoff);
    app.close();
}

fn model() -> (WorkspaceStore, SurfaceId) {
    let mut model = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
    model
        .apply(Action::CreateWorkspace {
            name: "same numeric identity".into(),
        })
        .unwrap();
    let surface = model.snapshot().workspaces[0].root.panes()[0].selected_surface;
    (model, surface)
}
fn owned(
    attention: &mut Attention,
    model: &WorkspaceStore,
    surface: SurfaceId,
    title: &str,
) -> u64 {
    attention
        .notify_owned(
            &model.snapshot(),
            surface,
            "cli",
            &NotificationContent {
                title: title.into(),
                ..Default::default()
            },
            false,
        )
        .unwrap()
}

#[test]
fn workspace_bulk_matching_includes_namespace_and_all_read_dismiss_preserves_unread_rows() {
    let (first, first_surface) = model();
    let (second, second_surface) = model();
    assert_eq!(
        first_surface, second_surface,
        "fixture deliberately reuses numeric IDs across namespaces"
    );
    let mut attention = Attention::default();
    let old = owned(&mut attention, &first, first_surface, "Old namespace");
    let current = owned(&mut attention, &second, second_surface, "Current namespace");
    let snapshot = second.snapshot();
    let filter = NotificationFilter::Workspace {
        identity_namespace: snapshot.identity_namespace,
        workspace_id: snapshot.workspaces[0].id,
    };
    let receipt = attention
        .apply_mutation(&filter, NotificationMutationKind::MarkRead, None, None)
        .unwrap();
    assert_eq!(receipt.affected_ids, vec![current]);
    assert_eq!(receipt.affected_count, 1);
    assert!(!attention.find(old).unwrap().read);
    let dismissed = attention
        .apply_mutation(
            &NotificationFilter::All,
            NotificationMutationKind::DismissRead,
            None,
            None,
        )
        .unwrap();
    assert_eq!(dismissed.affected_ids, vec![current]);
    assert_eq!(attention.items.len(), 1);
    assert_eq!(attention.items[0].id, old);
}

#[test]
fn cutoff_receipts_and_epoch_checks_preserve_newer_generations_and_monotonic_cursor() {
    let (model, surface) = model();
    let mut attention = Attention::default();
    let old = owned(&mut attention, &model, surface, "Observed");
    let cutoff = attention.cursor();
    let newer = owned(&mut attention, &model, surface, "Arrived after observation");
    let receipt = attention
        .apply_mutation(
            &NotificationFilter::All,
            NotificationMutationKind::Clear,
            Some(cutoff),
            Some(attention.epoch()),
        )
        .unwrap();
    assert_eq!(receipt.cursor, cutoff);
    assert_eq!(receipt.affected_ids, vec![old]);
    assert_eq!(attention.items[0].id, newer);
    let previous_epoch = attention.epoch();
    attention.reset_native("restored another document");
    assert!(matches!(
        attention.apply_mutation(
            &NotificationFilter::All,
            NotificationMutationKind::Clear,
            None,
            Some(previous_epoch)
        ),
        Err(AttentionError::StaleEpoch)
    ));
    assert_eq!(attention.items[0].id, newer);
    let cleared = attention
        .apply_mutation(
            &NotificationFilter::All,
            NotificationMutationKind::Clear,
            None,
            None,
        )
        .unwrap();
    assert_eq!(cleared.affected_ids, vec![newer]);
    assert_eq!(attention.cursor(), newer);
    let next = owned(
        &mut attention,
        &model,
        surface,
        "New generation after empty history",
    );
    assert!(next > newer);
}
