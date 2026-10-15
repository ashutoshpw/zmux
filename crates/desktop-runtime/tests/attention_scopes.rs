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
