//! Canonical checklist validation and undo ownership; persistence transactions live in runtime tests.
use zmux_workspace_core::{todo::*, *};

fn item(id: u128, text: &str) -> TodoEntry {
    TodoEntry {
        id: uuid::Uuid::from_u128(id),
        text: text.into(),
        state: TodoState::InProgress,
        origin: TodoOrigin::Agent,
    }
}
fn create(store: &mut WorkspaceStore) -> Workspace {
    store
        .apply(Action::CreateWorkspace {
            name: "checklist".into(),
        })
        .unwrap();
    store.snapshot().workspaces.last().unwrap().clone()
}
fn finish(store: &mut WorkspaceStore, effects: &[Effect], success: bool) {
    for effect in effects {
        if let Effect::StartSession { session_id, .. } = effect {
            store.finish_restore(*session_id, success);
        }
    }
}
#[test]
fn direct_core_todo_changes_validate_ids_counts_text_and_cross_workspace_ownership_atomically() {
    let mut store = WorkspaceStore::default();
    let first = create(&mut store);
    let second = create(&mut store);
    store
        .replace_workspace_todos(first.id, vec![item(1, "retained")])
        .unwrap();
    for (owner, items) in [
        (first.id, vec![item(0, "nil")]),
        (first.id, vec![item(2, "duplicate"), item(2, "duplicate")]),
        (second.id, vec![item(1, "foreign UUID")]),
        (first.id, vec![item(3, "  ")]),
        (first.id, vec![item(3, "bad\u{0007}")]),
        (
            first.id,
            vec![item(3, &"x".repeat(MAX_TODO_TEXT_BYTES + 1))],
        ),
        (
            first.id,
            (1..=MAX_TODO_ENTRIES + 1)
                .map(|id| item(id as u128, "too many"))
                .collect(),
        ),
    ] {
        let before = store.snapshot();
        assert!(store.replace_workspace_todos(owner, items).is_err());
        assert_eq!(store.snapshot(), before);
    }
    assert_eq!(
        store.snapshot().workspaces[0].todos,
        vec![item(1, "retained")]
    );
}
#[test]
fn removed_workspace_checklist_follows_exact_workspace_last_tab_and_last_pane_recovery() {
    for operation in ["workspace", "last-tab", "last-pane"] {
        let mut store = WorkspaceStore::default();
        let owner = create(&mut store);
        let todos = vec![item(1, "first"), item(2, "second")];
        store
            .replace_workspace_todos(owner.id, todos.clone())
            .unwrap();
        let pane = owner.root.panes()[0];
        let action = match operation {
            "workspace" => Action::CloseWorkspace {
                workspace_id: owner.id,
            },
            "last-tab" => Action::CloseSurface {
                surface_id: pane.selected_surface,
            },
            _ => Action::ClosePane { pane_id: pane.id },
        };
        store.apply(action).unwrap();
        assert!(store.snapshot().workspaces.is_empty());
        assert_eq!(store.snapshot().closed_items.len(), 1);
        let first = store
            .apply(Action::ReopenClosed { closed_id: None })
            .unwrap();
        assert_eq!(store.snapshot().workspaces[0].todos, todos, "{operation}");
        let pending = store.snapshot();
        let new_owner = pending.workspaces[0].id;
        assert_ne!(new_owner, owner.id);
        assert!(
            store.replace_workspace_todos(new_owner, vec![]).is_err(),
            "provisional owner cannot edit recovery data"
        );
        assert_eq!(store.snapshot(), pending);
        finish(&mut store, &first.effects, false);
        assert!(store.snapshot().workspaces.is_empty());
        assert_eq!(store.snapshot().closed_items.len(), 1);
        assert!(!store.snapshot().closed_items[0].restoring);
        let second = store
            .apply(Action::ReopenClosed { closed_id: None })
            .unwrap();
        finish(&mut store, &second.effects, true);
        let recovered = store.snapshot();
        assert!(recovered.closed_items.is_empty());
        assert_eq!(recovered.workspaces[0].todos, todos, "{operation}");
    }
}
#[test]
fn tab_recovery_preserves_a_surviving_owners_newer_checklist_and_duplicate_recovery_fails_atomically()
 {
    let mut store = WorkspaceStore::default();
    let owner = create(&mut store);
    let pane = owner.focused_pane;
    store
        .replace_workspace_todos(owner.id, vec![item(1, "old")])
        .unwrap();
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    let tab = store.snapshot().workspaces[0].root.panes()[0].selected_surface;
    store
        .apply(Action::CloseSurface { surface_id: tab })
        .unwrap();
    let newer = vec![item(1, "newer owner text"), item(2, "new item")];
    store
        .replace_workspace_todos(owner.id, newer.clone())
        .unwrap();
    let reopened = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert_eq!(store.snapshot().workspaces[0].todos, newer);
    finish(&mut store, &reopened.effects, true);
    assert_eq!(store.snapshot().workspaces[0].todos, newer);
    store
        .apply(Action::CloseWorkspace {
            workspace_id: owner.id,
        })
        .unwrap();
    let foreign = create(&mut store);
    store
        .replace_workspace_todos(foreign.id, vec![item(1, "current live owner")])
        .unwrap();
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::ReopenClosed { closed_id: None })
            .is_err()
    );
    assert_eq!(
        store.snapshot(),
        before,
        "duplicate recovery must preserve both current owner and old recovery item"
    );
}

#[test]
fn moving_last_surface_cannot_discard_checklist_or_rebind_it_to_unrelated_history() {
    for stale_history in [true, false] {
        let mut store = WorkspaceStore::default();
        let destination = create(&mut store);
        let owner = create(&mut store);
        store
            .replace_workspace_todos(owner.id, vec![item(1, "belongs to source workspace")])
            .unwrap();
        if stale_history {
            store
                .apply(Action::CreateSurface {
                    pane_id: owner.focused_pane,
                })
                .unwrap();
            let extra = store.snapshot().workspaces[1].root.panes()[0].selected_surface;
            store
                .apply(Action::CloseSurface { surface_id: extra })
                .unwrap();
        }
        let before = store.snapshot();
        let result = store.apply(Action::MoveSurface {
            surface_id: owner.root.panes()[0].selected_surface,
            pane_id: destination.focused_pane,
            index: 1,
        });
        assert_eq!(
            store.snapshot().closed_items,
            before.closed_items,
            "moving a surface must never graft its workspace checklist onto an older close descriptor"
        );
        assert!(
            result.is_err(),
            "last-surface move must preserve checklist ownership, including with stale history={stale_history}"
        );
        assert_eq!(store.snapshot(), before, "refusal must be atomic");

        // Clearing the checklist intentionally permits the existing destructive-owner move.
        store.replace_workspace_todos(owner.id, vec![]).unwrap();
        let history = store.snapshot().closed_items;
        store
            .apply(Action::MoveSurface {
                surface_id: owner.root.panes()[0].selected_surface,
                pane_id: destination.focused_pane,
                index: 1,
            })
            .unwrap();
        assert_eq!(store.snapshot().workspaces.len(), 1);
        assert_eq!(store.snapshot().closed_items, history);
    }
}

#[test]
fn moving_one_of_multiple_surfaces_keeps_the_source_workspace_checklist() {
    let mut store = WorkspaceStore::default();
    let destination = create(&mut store);
    let owner = create(&mut store);
    let todos = vec![item(1, "workspace scoped")];
    store
        .replace_workspace_todos(owner.id, todos.clone())
        .unwrap();
    store
        .apply(Action::CreateSurface {
            pane_id: owner.focused_pane,
        })
        .unwrap();
    store
        .apply(Action::MoveSurface {
            surface_id: owner.root.panes()[0].selected_surface,
            pane_id: destination.focused_pane,
            index: 1,
        })
        .unwrap();
    let after = store.snapshot();
    assert_eq!(after.workspaces[1].todos, todos);
    assert!(after.workspaces[0].todos.is_empty());
    assert!(after.closed_items.is_empty());
    assert_eq!(after.workspaces[0].root.panes()[0].surfaces.len(), 2);
    assert_eq!(after.workspaces[1].root.panes()[0].surfaces.len(), 1);
}
#[test]
fn checklist_recovery_history_evicts_oldest_settled_items_within_its_separate_budget() {
    let mut store = WorkspaceStore::default();
    let mut history = Vec::new();
    for batch in 0..3 {
        let owner = create(&mut store);
        let todos: Vec<_> = (1..=MAX_TODO_ENTRIES)
            .map(|id| {
                item(
                    (batch * MAX_TODO_ENTRIES + id) as u128,
                    &"\\".repeat(MAX_TODO_TEXT_BYTES),
                )
            })
            .collect();
        store.replace_workspace_todos(owner.id, todos).unwrap();
        store
            .apply(Action::CloseWorkspace {
                workspace_id: owner.id,
            })
            .unwrap();
        history.push(store.snapshot().closed_items.last().unwrap().id);
    }
    let retained: Vec<_> = store
        .snapshot()
        .closed_items
        .iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(
        retained,
        history[1..],
        "512 KiB recovery budget must evict the oldest settled descriptor"
    );
    let newest = store
        .apply(Action::ReopenClosed {
            closed_id: history.last().copied(),
        })
        .unwrap();
    finish(&mut store, &newest.effects, true);
    assert_eq!(store.snapshot().workspaces[0].todos.len(), MAX_TODO_ENTRIES);
}

#[test]
fn undo_budget_keeps_inflight_and_newest_recovery_and_evicts_only_an_older_settled_owner() {
    let mut store = WorkspaceStore::default();
    let mut history = Vec::new();
    for batch in 0..2 {
        let owner = create(&mut store);
        store
            .replace_workspace_todos(
                owner.id,
                (1..=MAX_TODO_ENTRIES)
                    .map(|id| {
                        item(
                            (batch * MAX_TODO_ENTRIES + id) as u128,
                            &"\\".repeat(MAX_TODO_TEXT_BYTES),
                        )
                    })
                    .collect(),
            )
            .unwrap();
        store
            .apply(Action::CloseWorkspace {
                workspace_id: owner.id,
            })
            .unwrap();
        history.push(store.snapshot().closed_items.last().unwrap().id);
    }
    let in_flight = store
        .apply(Action::ReopenClosed {
            closed_id: Some(history[0]),
        })
        .unwrap();
    let newest = create(&mut store);
    store
        .replace_workspace_todos(
            newest.id,
            (101..101 + MAX_TODO_ENTRIES)
                .map(|id| item(id as u128, &"\\".repeat(MAX_TODO_TEXT_BYTES)))
                .collect(),
        )
        .unwrap();
    store
        .apply(Action::CloseWorkspace {
            workspace_id: newest.id,
        })
        .unwrap();
    let records = store.snapshot().closed_items;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, history[0]);
    assert!(records[0].restoring);
    assert_ne!(records[1].id, history[1]);
    assert_eq!(records[1].workspace_id, newest.id);
    finish(&mut store, &in_flight.effects, true);
    assert_eq!(store.snapshot().closed_items.len(), 1);
    assert_eq!(store.snapshot().closed_items[0].workspace_id, newest.id);
}
