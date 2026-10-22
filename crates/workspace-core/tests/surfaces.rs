use zmux_workspace_core::*;

fn workspace(store: &mut WorkspaceStore) -> (WorkspaceId, PaneId, SurfaceId, SessionId) {
    store
        .apply(Action::CreateWorkspace {
            name: "work".into(),
        })
        .unwrap();
    let state = store.snapshot();
    let w = state.workspaces.last().unwrap();
    let p = w.root.panes()[0];
    (w.id, p.id, p.selected_surface, p.session_id().unwrap())
}
fn add_tab(store: &mut WorkspaceStore, pane: PaneId) -> Surface {
    store
        .apply(Action::CreateSurface { pane_id: pane })
        .unwrap();
    store
        .snapshot()
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .find(|p| p.id == pane)
        .unwrap()
        .selected()
        .clone()
}
fn sessions(effects: &[Effect]) -> Vec<SessionId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StartSession { session_id, .. } => Some(*session_id),
            _ => None,
        })
        .collect()
}

#[test]
fn tabs_move_and_reorder_without_restarting_sessions_and_close_only_their_owner() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, first_session) = workspace(&mut store);
    let second = add_tab(&mut store, pane);
    store
        .apply(Action::SelectSurface { surface_id: first })
        .unwrap();
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0]
            .session_id()
            .unwrap(),
        first_session
    );
    assert!(
        store
            .apply(Action::ReorderSurface {
                focus: true,
                surface_id: second.id,
                index: 0
            })
            .unwrap()
            .effects
            .is_empty()
    );
    let (other, destination, _, _) = workspace(&mut store);
    let result = store
        .apply(Action::MoveSurface {
            surface_id: second.id,
            pane_id: destination,
            index: 1,
        })
        .unwrap();
    assert!(result.effects.is_empty());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.focused_workspace, Some(other));
    assert_eq!(
        snapshot.workspaces[0].root.panes()[0].selected_surface,
        first
    );
    assert_eq!(snapshot.workspaces[1].root.panes()[0].selected(), &second);
    let closed = store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    assert_eq!(
        closed.effects,
        vec![Effect::StopSession {
            session_id: second.session_id.unwrap()
        }]
    );
    assert_eq!(store.snapshot().workspaces.len(), 2);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::SelectSurface {
                surface_id: second.id
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
}

#[test]
fn reopen_consumes_history_only_after_launch_and_failed_restore_is_retriable() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, first_session) = workspace(&mut store);
    let second = add_tab(&mut store, pane);
    store.set_session_cwd(second.session_id.unwrap(), "/tmp/original directory".into());
    store
        .apply(Action::RenameSurface {
            surface_id: second.id,
            name: "build".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    let history_id = store.snapshot().closed_items[0].id;
    let reopen = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(
        matches!(&reopen.effects[0], Effect::StartSession { cwd: Some(cwd), .. } if cwd == "/tmp/original directory")
    );
    let session = sessions(&reopen.effects)[0];
    let state = store.snapshot();
    let restored = state.workspaces[0].root.panes()[0].selected();
    assert_eq!(restored.title, "build");
    assert_ne!(restored.id, second.id);
    assert_ne!(session, second.session_id.unwrap());
    assert!(state.closed_items[0].restoring);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::ReopenClosed {
                closed_id: Some(history_id)
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(
        store.finish_restore(session, false),
        vec![Effect::StopSession {
            session_id: session
        }]
    );
    let state = store.snapshot();
    assert_eq!(state.workspaces[0].root.panes()[0].selected_surface, first);
    assert_eq!(
        state.workspaces[0].root.panes()[0].session_id().unwrap(),
        first_session
    );
    assert!(!state.closed_items[0].restoring);
    let retry = store
        .apply(Action::ReopenClosed {
            closed_id: Some(history_id),
        })
        .unwrap();
    let replacement = sessions(&retry.effects)[0];
    assert_ne!(session, replacement);
    assert!(store.finish_restore(replacement, true).is_empty());
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn selected_history_and_last_tab_fallback_preserve_order_without_stealing_live_ids() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    store
        .apply(Action::RenameSurface {
            surface_id: first,
            name: "A".into(),
        })
        .unwrap();
    let second = add_tab(&mut store, pane);
    store
        .apply(Action::RenameSurface {
            surface_id: second.id,
            name: "B".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface { surface_id: first })
        .unwrap();
    let a_history = store.snapshot().closed_items[0].id;
    store
        .apply(Action::CloseSurface {
            surface_id: second.id,
        })
        .unwrap();
    assert!(store.snapshot().workspaces.is_empty());
    let (_, live_pane, live_surface, _) = workspace(&mut store);
    for (closed_id, title) in [(Some(a_history), "A"), (None, "B")] {
        let result = store.apply(Action::ReopenClosed { closed_id }).unwrap();
        let session = sessions(&result.effects)[0];
        store.finish_restore(session, true);
        let state = store.snapshot();
        let restored = state.workspaces.last().unwrap().root.panes()[0].selected();
        assert_eq!(restored.title, title);
        assert_ne!(restored.id, live_surface);
        assert_eq!(state.workspaces[0].root.panes()[0].id, live_pane);
    }
    assert_eq!(store.snapshot().workspaces.len(), 2);
    assert!(store.snapshot().closed_items.is_empty());
}

#[test]
fn grouped_workspace_restore_preserves_layout_and_rolls_back_all_provisional_sessions() {
    let mut store = WorkspaceStore::default();
    let (wid, pane, _, _) = workspace(&mut store);
    add_tab(&mut store, pane);
    store
        .apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Vertical,
        })
        .unwrap();
    let old = store.snapshot().workspaces[0].clone();
    store
        .apply(Action::CloseWorkspace { workspace_id: wid })
        .unwrap();
    assert_eq!(store.snapshot().closed_items.len(), 1);
    let result = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    let started = sessions(&result.effects);
    assert_eq!(started.len(), 3);
    assert!(store.finish_restore(started[0], true).is_empty());
    let cleanup = store.finish_restore(started[1], false);
    assert_eq!(cleanup.len(), 3);
    assert!(store.snapshot().workspaces.is_empty());
    assert!(!store.snapshot().closed_items[0].restoring);
    // A late completion for the aborted group cannot consume history or attach anything.
    assert!(store.finish_restore(started[2], true).is_empty());
    let retry = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    for id in sessions(&retry.effects) {
        store.finish_restore(id, true);
    }
    let state = store.snapshot();
    let restored = &state.workspaces[0];
    assert!(matches!(
        restored.root,
        PaneTree::Split {
            axis: Axis::Vertical,
            ..
        }
    ));
    assert_eq!(
        restored
            .root
            .panes()
            .iter()
            .map(|p| p.surfaces.len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(restored.focused_pane, restored.root.panes()[1].id);
    assert_ne!(restored.id, old.id);
    assert!(state.closed_items.is_empty());
}

#[test]
fn batch_close_has_one_history_entry_and_invalid_moves_and_capacity_are_atomic() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    for _ in 1..MAX_SURFACES_PER_PANE {
        add_tab(&mut store, pane);
    }
    let before = store.snapshot();
    for action in [
        Action::CreateSurface { pane_id: pane },
        Action::MoveSurface {
            surface_id: first,
            pane_id: PaneId(999999),
            index: 0,
        },
        Action::ReorderSurface {
            focus: true,
            surface_id: first,
            index: MAX_SURFACES_PER_PANE,
        },
    ] {
        assert!(store.apply(action).is_err());
        assert_eq!(store.snapshot(), before);
    }
    let result = store
        .apply(Action::CloseOtherSurfaces { surface_id: first })
        .unwrap();
    assert_eq!(result.effects.len(), MAX_SURFACES_PER_PANE - 1);
    assert_eq!(store.snapshot().closed_items.len(), 1);
    let restored = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    for session in sessions(&restored.effects) {
        store.finish_restore(session, true);
    }
    let after = store.snapshot();
    assert_eq!(
        after.workspaces[0].root.panes()[0].surfaces.len(),
        MAX_SURFACES_PER_PANE
    );
    assert!(after.closed_items.is_empty());
}

#[test]
fn close_history_evicts_old_entries_and_single_tab_reorder_is_safe() {
    let mut store = WorkspaceStore::default();
    let (_, pane, first, _) = workspace(&mut store);
    assert!(
        store
            .apply(Action::ReorderSurface {
                focus: true,
                surface_id: first,
                index: 0
            })
            .unwrap()
            .effects
            .is_empty()
    );
    for _ in 0..MAX_CLOSED_ITEMS + 2 {
        let tab = add_tab(&mut store, pane);
        store
            .apply(Action::CloseSurface { surface_id: tab.id })
            .unwrap();
    }
    let state = store.snapshot();
    assert_eq!(state.closed_items.len(), MAX_CLOSED_ITEMS);
    assert!(state.closed_items.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(state.workspaces[0].root.panes()[0].selected_surface, first);
}

#[test]
fn browser_creation_can_preserve_every_workspace_and_pane_selection() {
    let mut store = WorkspaceStore::default();
    let (first, first_pane, _, _) = workspace(&mut store);
    store
        .apply(Action::SplitPane {
            pane_id: first_pane,
            axis: Axis::Horizontal,
        })
        .unwrap();
    let split_pane = store.snapshot().workspaces[0].focused_pane;
    store
        .apply(Action::FocusPane {
            pane_id: first_pane,
        })
        .unwrap();
    let (_, other_pane, _, _) = workspace(&mut store);
    store
        .apply(Action::SelectWorkspace {
            workspace_id: first,
        })
        .unwrap();
    for pane in [first_pane, split_pane, other_pane] {
        let before = store.snapshot();
        let action: Action = serde_json::from_value(serde_json::json!({
            "action":"create_browser", "pane_id":pane, "url":"about:blank", "focus":false
        }))
        .expect("create_browser accepts explicit focus policy");
        store.apply(action).unwrap();
        let after = store.snapshot();
        assert_eq!(after.focused_workspace, before.focused_workspace);
        for (after, before) in after.workspaces.iter().zip(&before.workspaces) {
            assert_eq!(after.focused_pane, before.focused_pane);
            for (after, before) in after.root.panes().iter().zip(before.root.panes()) {
                assert_eq!(after.selected_surface, before.selected_surface);
            }
        }
        let pane = after
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .find(|p| p.id == pane)
            .unwrap();
        assert_eq!(pane.surfaces.last().unwrap().kind, SurfaceKind::Browser);
        assert_ne!(pane.selected_surface, pane.surfaces.last().unwrap().id);
    }
    // Legacy clients omit focus; creation must still select their new browser.
    let legacy: Action = serde_json::from_value(serde_json::json!({
        "action":"create_browser", "pane_id":other_pane, "url":"about:blank"
    }))
    .unwrap();
    store.apply(legacy).unwrap();
    let after = store.snapshot();
    let target = after
        .workspaces
        .iter()
        .find(|w| w.root.panes().iter().any(|p| p.id == other_pane))
        .unwrap();
    let pane = target
        .root
        .panes()
        .into_iter()
        .find(|p| p.id == other_pane)
        .unwrap();
    assert_eq!(after.focused_workspace, Some(target.id));
    assert_eq!(target.focused_pane, other_pane);
    assert_eq!(pane.selected_surface, pane.surfaces.last().unwrap().id);
}

#[test]
fn browser_tabs_have_no_pty_and_restore_the_observed_url_without_replaying_navigation() {
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    let created = store
        .apply(Action::CreateBrowser {
            profile_id: None,
            focus: true,
            pane_id: pane,
            url: "https://example.com/initial".into(),
        })
        .unwrap();
    assert!(created.effects.is_empty());
    let browser = store.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .clone();
    assert_eq!(browser.kind, SurfaceKind::Browser);
    assert_eq!(browser.session_id, None);
    store
        .apply(Action::NavigateBrowser {
            surface_id: browser.id,
            url: "https://example.com/next".into(),
        })
        .unwrap();
    let generation = store.snapshot().workspaces[0].root.panes()[0]
        .selected()
        .navigation;
    store
        .apply(Action::ObserveBrowser {
            surface_id: browser.id,
            url: "https://example.com/redirected".into(),
            title: "Docs".into(),
        })
        .unwrap();
    let before = store.snapshot();
    assert_eq!(
        before.workspaces[0].root.panes()[0].selected().navigation,
        generation
    );
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,test",
        "https://user:secret@example.com/",
        "https://",
    ] {
        assert!(
            store
                .apply(Action::NavigateBrowser {
                    surface_id: browser.id,
                    url: url.into()
                })
                .is_err()
        );
        assert_eq!(store.snapshot(), before);
    }
    let closed = store
        .apply(Action::CloseSurface {
            surface_id: browser.id,
        })
        .unwrap();
    assert!(closed.effects.is_empty());
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0].selected_surface,
        terminal
    );
    let restored = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap();
    assert!(restored.effects.is_empty());
    let state = store.snapshot();
    let restored = state.workspaces[0].root.panes()[0].selected();
    assert_eq!(
        restored.url.as_deref(),
        Some("https://example.com/redirected")
    );
    assert_eq!(restored.title, "Docs");
    assert_ne!(restored.id, browser.id);
    assert!(state.closed_items.is_empty());
}

// ---- file surfaces (CU-14 S1): identity and a trusted dirty bit, never content ----

fn file_pane(store: &mut WorkspaceStore) -> PaneId {
    workspace(store).1
}
fn create_file(
    store: &mut WorkspaceStore,
    pane: PaneId,
    path: &str,
    mode: FileMode,
) -> Result<Transition, ActionError> {
    store.apply(Action::CreateFile {
        pane_id: pane,
        path: path.into(),
        mode,
        focus: true,
    })
}
fn pane_surfaces(store: &WorkspaceStore, pane: PaneId) -> Vec<Surface> {
    store
        .snapshot()
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .find(|p| p.id == pane)
        .unwrap()
        .surfaces
        .clone()
}
fn last_surface(store: &WorkspaceStore, pane: PaneId) -> Surface {
    pane_surfaces(store, pane).pop().unwrap()
}
fn mark_dirty(store: &mut WorkspaceStore, surface: SurfaceId, dirty: bool) {
    store
        .apply(Action::ObserveFile {
            surface_id: surface,
            dirty,
        })
        .unwrap();
}

#[test]
fn create_file_appends_a_tab_without_session_effects_and_honours_focus() {
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    let created = create_file(
        &mut store,
        pane,
        "/home/a/proj/README.md",
        FileMode::Markdown,
    )
    .expect("absolute canonical path is accepted");
    assert!(created.effects.is_empty(), "file tabs never start sessions");
    let file = last_surface(&store, pane);
    assert_eq!(file.kind, SurfaceKind::File);
    assert_eq!(file.session_id, None);
    assert_eq!(file.url, None);
    assert_eq!(file.navigation, 0);
    assert_eq!(file.browser_profile_id, None);
    assert_eq!(file.title, "README.md");
    assert_eq!(
        file.file,
        Some(FileSurface {
            path: "/home/a/proj/README.md".into(),
            mode: FileMode::Markdown,
            dirty: false
        })
    );
    let snapshot = store.snapshot();
    assert_eq!(
        snapshot.workspaces[0].root.panes()[0].selected_surface,
        file.id
    );
    // focus:false leaves selection alone; terminals keep `file == None`.
    let terminal_surface = pane_surfaces(&store, pane)
        .into_iter()
        .find(|s| s.id == terminal)
        .unwrap();
    assert_eq!(terminal_surface.file, None);
    store
        .apply(Action::SelectSurface {
            surface_id: terminal,
        })
        .unwrap();
    let action: Action = serde_json::from_value(serde_json::json!({
        "action":"create_file", "pane_id":pane, "path":"/home/a/main.rs", "mode":"text", "focus":false
    }))
    .unwrap();
    store.apply(action).unwrap();
    assert_eq!(
        store.snapshot().workspaces[0].root.panes()[0].selected_surface,
        terminal
    );
    // Legacy-style requests omit focus; the new tab is then selected like a browser.
    let action: Action = serde_json::from_value(serde_json::json!({
        "action":"create_file", "pane_id":pane, "path":"/home/a/pic.png", "mode":"image"
    }))
    .unwrap();
    store.apply(action).unwrap();
    let pane_now = store.snapshot().workspaces[0].root.panes()[0].clone();
    assert_eq!(
        pane_now.selected_surface,
        pane_now.surfaces.last().unwrap().id
    );
}

#[test]
fn create_file_rejects_bad_paths_and_modes_without_touching_the_store() {
    let mut store = WorkspaceStore::default();
    let pane = file_pane(&mut store);
    let before = store.snapshot();
    let next = store.next_identifier();
    let too_long = format!("/{}", "a".repeat(4096));
    for path in [
        "",
        "relative/file.txt",
        "file.txt",
        "~/file.txt",
        "/",
        "/tmp/dir/",
        "/tmp/../etc/passwd",
        "/tmp/./file",
        "/tmp//file",
        "/tmp/fi\0le",
        "/tmp/fi\nle",
        "/tmp/fi\u{7f}le",
        "/tmp/\u{85}file",
        too_long.as_str(),
    ] {
        let error = create_file(&mut store, pane, path, FileMode::Text)
            .expect_err(&format!("{path:?} must be rejected"));
        assert!(
            matches!(error, ActionError::InvalidOperation(_)),
            "{path:?}: {error:?}"
        );
        assert_eq!(store.snapshot(), before, "{path:?}");
        assert_eq!(store.next_identifier(), next, "{path:?}");
    }
    // The limit is on bytes: a path of exactly 4096 bytes is the largest accepted.
    let longest = format!("/{}", "b".repeat(4095));
    create_file(&mut store, pane, &longest, FileMode::Text).unwrap();
    assert_eq!(last_surface(&store, pane).title.chars().count(), 128);
    // Unknown modes and fields never deserialize.
    for wire in [
        serde_json::json!({"action":"create_file","pane_id":pane,"path":"/a","mode":"video"}),
        serde_json::json!({"action":"create_file","pane_id":pane,"path":"/a","mode":"text","dirty":true}),
        serde_json::json!({"action":"create_file","pane_id":pane,"path":"/a"}),
    ] {
        assert!(serde_json::from_value::<Action>(wire).is_err());
    }
    assert!(
        create_file(&mut store, PaneId(9999), "/tmp/x", FileMode::Text).is_err(),
        "unknown pane"
    );
}

#[test]
fn create_file_rejects_duplicate_paths_and_a_full_pane() {
    let mut store = WorkspaceStore::default();
    let (_, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/one.txt", FileMode::Text).unwrap();
    let before = store.snapshot();
    // Exact canonical duplicate, even in another mode, is refused in any pane.
    for mode in [FileMode::Text, FileMode::Markdown] {
        let error = create_file(&mut store, pane, "/tmp/one.txt", mode).unwrap_err();
        assert!(error.to_string().contains("already open"), "{error}");
    }
    let (_, other_pane, _, _) = workspace(&mut store);
    let before_other = store.snapshot();
    let error = create_file(&mut store, other_pane, "/tmp/one.txt", FileMode::Text).unwrap_err();
    assert!(error.to_string().contains("already open"), "{error}");
    assert_eq!(store.snapshot(), before_other);
    assert_ne!(before, before_other);
    // Case variants are different canonical strings (documented gap, D18).
    create_file(&mut store, pane, "/tmp/ONE.txt", FileMode::Text).unwrap();
    // Fill the pane to MAX_SURFACES_PER_PANE; the next file is a capacity error.
    while pane_surfaces(&store, pane).len() < MAX_SURFACES_PER_PANE {
        let path = format!("/tmp/fill-{}.txt", pane_surfaces(&store, pane).len());
        create_file(&mut store, pane, &path, FileMode::Text).unwrap();
    }
    let full = store.snapshot();
    assert_eq!(
        create_file(&mut store, pane, "/tmp/overflow.txt", FileMode::Text).unwrap_err(),
        ActionError::Capacity {
            resource: "tabs per pane",
            limit: MAX_SURFACES_PER_PANE
        }
    );
    assert_eq!(store.snapshot(), full);
}

#[test]
fn observe_file_sets_and_clears_dirty_only_on_editable_file_surfaces() {
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let text = last_surface(&store, pane).id;
    create_file(&mut store, pane, "/tmp/b.md", FileMode::Markdown).unwrap();
    let markdown = last_surface(&store, pane).id;
    create_file(&mut store, pane, "/tmp/c.png", FileMode::Image).unwrap();
    let image = last_surface(&store, pane).id;
    create_file(&mut store, pane, "/tmp/d.pdf", FileMode::Pdf).unwrap();
    let pdf = last_surface(&store, pane).id;
    store
        .apply(Action::CreateBrowser {
            profile_id: None,
            focus: false,
            pane_id: pane,
            url: "https://example.com/".into(),
        })
        .unwrap();
    let browser = last_surface(&store, pane).id;
    for id in [text, markdown] {
        mark_dirty(&mut store, id, true);
        let dirty = |store: &WorkspaceStore| {
            pane_surfaces(store, pane)
                .into_iter()
                .find(|s| s.id == id)
                .unwrap()
                .file
                .unwrap()
                .dirty
        };
        assert!(dirty(&store));
        mark_dirty(&mut store, id, false);
        assert!(!dirty(&store));
    }
    let before = store.snapshot();
    let observe = |id, dirty| Action::ObserveFile {
        surface_id: id,
        dirty,
    };
    for id in [terminal, browser] {
        for dirty in [true, false] {
            assert!(store.apply(observe(id, dirty)).is_err(), "{id:?}");
        }
    }
    for id in [image, pdf] {
        assert!(
            store.apply(observe(id, true)).is_err(),
            "{id:?} is never dirty"
        );
    }
    assert!(store.apply(observe(SurfaceId(9999), true)).is_err());
    assert_eq!(
        store.snapshot(),
        before,
        "rejections leave the store untouched"
    );
    // Clearing an already clean preview surface is an idempotent no-op.
    store.apply(observe(image, false)).unwrap();
    assert_eq!(store.snapshot(), before);
}

#[test]
fn observe_file_is_idempotent_and_does_not_advance_the_revision_when_unchanged() {
    let mut store = WorkspaceStore::default();
    let pane = file_pane(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let id = last_surface(&store, pane).id;
    let clean = store.snapshot();
    let same = store
        .apply(Action::ObserveFile {
            surface_id: id,
            dirty: false,
        })
        .unwrap();
    assert!(same.effects.is_empty());
    assert_eq!(same.revision, clean.revision);
    assert_eq!(store.snapshot(), clean);
    let changed = store
        .apply(Action::ObserveFile {
            surface_id: id,
            dirty: true,
        })
        .unwrap();
    assert_eq!(changed.revision, clean.revision + 1);
    let repeated = store
        .apply(Action::ObserveFile {
            surface_id: id,
            dirty: true,
        })
        .unwrap();
    assert_eq!(repeated.revision, changed.revision);
    assert_eq!(store.snapshot().revision, changed.revision);
}

#[test]
fn close_paths_refuse_dirty_file_atomically_and_succeed_once_clean() {
    // CloseSurface
    let mut store = WorkspaceStore::default();
    let (_, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let file = last_surface(&store, pane).id;
    mark_dirty(&mut store, file, true);
    let before = store.snapshot();
    let next = store.next_identifier();
    let error = store
        .apply(Action::CloseSurface { surface_id: file })
        .unwrap_err();
    assert!(error.to_string().contains("unsaved changes"), "{error}");
    assert_eq!(store.snapshot(), before);
    assert_eq!(store.next_identifier(), next);
    mark_dirty(&mut store, file, false);
    store
        .apply(Action::CloseSurface { surface_id: file })
        .unwrap();

    // CloseOtherSurfaces: only the tabs that would be closed matter.
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let file = last_surface(&store, pane).id;
    mark_dirty(&mut store, file, true);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::CloseOtherSurfaces {
                surface_id: terminal
            })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    store
        .apply(Action::CloseOtherSurfaces { surface_id: file })
        .expect("a dirty file that stays open does not block closing its siblings");
    assert_eq!(pane_surfaces(&store, pane).len(), 1);
    mark_dirty(&mut store, file, false);

    // ClosePane
    let mut store = WorkspaceStore::default();
    let (_, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Markdown).unwrap();
    let file = last_surface(&store, pane).id;
    store
        .apply(Action::SplitPane {
            pane_id: pane,
            axis: Axis::Horizontal,
        })
        .unwrap();
    mark_dirty(&mut store, file, true);
    let before = store.snapshot();
    assert!(store.apply(Action::ClosePane { pane_id: pane }).is_err());
    assert_eq!(store.snapshot(), before);
    mark_dirty(&mut store, file, false);
    store.apply(Action::ClosePane { pane_id: pane }).unwrap();

    // CloseWorkspace
    let mut store = WorkspaceStore::default();
    let (workspace_id, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let file = last_surface(&store, pane).id;
    mark_dirty(&mut store, file, true);
    let before = store.snapshot();
    assert!(
        store
            .apply(Action::CloseWorkspace { workspace_id })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    mark_dirty(&mut store, file, false);
    store
        .apply(Action::CloseWorkspace { workspace_id })
        .unwrap();

    // CloseWindow reaches the same guard through its workspaces.
    let mut store = WorkspaceStore::default();
    workspace(&mut store);
    store.apply(Action::CreateWindow { focus: false }).unwrap();
    let state = store.snapshot();
    let window = state.windows[1].id;
    let window_workspace = state.windows[1].selected_workspace.unwrap();
    let window_pane = state
        .workspaces
        .iter()
        .find(|w| w.id == window_workspace)
        .unwrap()
        .focused_pane;
    create_file(&mut store, window_pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let file = last_surface(&store, window_pane).id;
    mark_dirty(&mut store, file, true);
    let before = store.snapshot();
    let next = store.next_identifier();
    assert!(
        store
            .apply(Action::CloseWindow { window_id: window })
            .is_err()
    );
    assert_eq!(store.snapshot(), before);
    assert_eq!(store.next_identifier(), next);
    mark_dirty(&mut store, file, false);
    store
        .apply(Action::CloseWindow { window_id: window })
        .unwrap();
}

#[test]
fn reopen_file_resets_dirty_and_rejects_open_path() {
    let mut store = WorkspaceStore::default();
    let (_, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.md", FileMode::Markdown).unwrap();
    let original = last_surface(&store, pane);
    store
        .apply(Action::RenameSurface {
            surface_id: original.id,
            name: "notes".into(),
        })
        .unwrap();
    store
        .apply(Action::CloseSurface {
            surface_id: original.id,
        })
        .unwrap();
    let effects = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap()
        .effects;
    assert!(effects.is_empty(), "reopening a file starts no session");
    let reopened = last_surface(&store, pane);
    assert_ne!(reopened.id, original.id, "a fresh surface identity");
    assert_eq!(reopened.title, "notes");
    assert_eq!(
        reopened.file,
        Some(FileSurface {
            path: "/tmp/a.md".into(),
            mode: FileMode::Markdown,
            dirty: false
        })
    );
    assert!(store.snapshot().closed_items.is_empty());

    // The path was opened elsewhere while the tab was closed: explicit failure, history kept.
    store
        .apply(Action::CloseSurface {
            surface_id: reopened.id,
        })
        .unwrap();
    let (_, other_pane, _, _) = workspace(&mut store);
    create_file(&mut store, other_pane, "/tmp/a.md", FileMode::Text).unwrap();
    let before = store.snapshot();
    let error = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap_err();
    assert!(error.to_string().contains("already open"), "{error}");
    assert_eq!(store.snapshot(), before);
    assert_eq!(before.closed_items.len(), 1);
}

#[test]
fn reopening_a_closed_pane_or_workspace_keeps_file_paths_clean() {
    let mut store = WorkspaceStore::default();
    let (workspace_id, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    create_file(&mut store, pane, "/tmp/b.pdf", FileMode::Pdf).unwrap();
    store
        .apply(Action::CloseWorkspace { workspace_id })
        .unwrap();
    let effects = store
        .apply(Action::ReopenClosed { closed_id: None })
        .unwrap()
        .effects;
    assert_eq!(sessions(&effects).len(), 1, "only the terminal restarts");
    let state = store.snapshot();
    let files: Vec<_> = state
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .flat_map(|p| p.surfaces.iter())
        .filter_map(|s| s.file.clone())
        .collect();
    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|f| !f.dirty));
}

#[test]
fn move_surface_keeps_file_state() {
    let mut store = WorkspaceStore::default();
    let (_, pane, _, _) = workspace(&mut store);
    create_file(&mut store, pane, "/tmp/a.txt", FileMode::Text).unwrap();
    let file = last_surface(&store, pane);
    mark_dirty(&mut store, file.id, true);
    let (_, destination, _, _) = workspace(&mut store);
    let effects = store
        .apply(Action::MoveSurface {
            surface_id: file.id,
            pane_id: destination,
            index: 1,
        })
        .unwrap()
        .effects;
    assert!(effects.is_empty());
    let moved = last_surface(&store, destination);
    assert_eq!(moved.id, file.id);
    assert_eq!(
        moved.file,
        Some(FileSurface {
            path: "/tmp/a.txt".into(),
            mode: FileMode::Text,
            dirty: true
        })
    );
}

#[test]
fn snapshot_serializes_file_surface_shape() {
    let mut store = WorkspaceStore::default();
    let (_, pane, terminal, _) = workspace(&mut store);
    create_file(
        &mut store,
        pane,
        "/Users/a/proj/README.md",
        FileMode::Markdown,
    )
    .unwrap();
    let wire = serde_json::to_value(store.snapshot()).unwrap();
    let surfaces = &wire["workspaces"][0]["root"]["pane"]["surfaces"];
    assert_eq!(surfaces[0]["id"], terminal.0);
    assert_eq!(surfaces[0]["kind"], "terminal");
    assert!(surfaces[0]["file"].is_null());
    let file = &surfaces[1];
    assert_eq!(file["kind"], "file");
    assert!(file["session_id"].is_null());
    assert!(file["url"].is_null());
    assert_eq!(file["navigation"], 0);
    assert!(file["browser_profile_id"].is_null());
    assert_eq!(file["title"], "README.md");
    assert!(file["cwd"].is_null());
    assert!(file["remote"].is_null());
    assert!(file["local_tmux"].is_null());
    assert!(file["local_zellij"].is_null());
    assert!(file["local_zellij_completion"].is_null());
    assert_eq!(
        file["file"],
        serde_json::json!({"path":"/Users/a/proj/README.md","mode":"markdown","dirty":false})
    );
    assert_eq!(wire["model_version"], 8);
    assert_eq!(
        serde_json::to_value(FileMode::Pdf).unwrap(),
        serde_json::json!("pdf")
    );
}

#[test]
fn model_version_is_eight() {
    assert_eq!(MODEL_VERSION, 8);
    assert_eq!(WorkspaceStore::default().snapshot().model_version, 8);
}
