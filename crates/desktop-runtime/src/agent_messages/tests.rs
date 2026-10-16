//! Real journal-worker tests. WorkspaceStore creates authoritative target/session identities;
//! the public CLI regression separately proves socket dispatch and actual PTY non-input.
use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    time::{Duration, Instant},
};
use zmux_workspace_core::{
    Action, WorkspaceStore,
    targets::{TargetKind, TargetRef},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-inbox-{}", Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Inbox {
    runtime: Runtime,
    model: WorkspaceStore,
}
impl Inbox {
    fn new(path: &std::path::Path) -> Self {
        let mut model = WorkspaceStore::with_identity_namespace(Uuid::new_v4());
        model
            .apply(Action::CreateWorkspace {
                name: "recipient".into(),
            })
            .unwrap();
        let mut runtime = Runtime::start(Some(path.into())).unwrap();
        runtime.observe(model.snapshot(), vec![]);
        Self { runtime, model }
    }
    fn surface(&self) -> String {
        let m = self.model.snapshot();
        m.object_uuid(
            TargetKind::Surface,
            m.workspaces[0].root.panes()[0].selected_surface.0,
        )
        .to_string()
    }
    fn wait(&mut self, id: Uuid) -> OperationReceipt {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.runtime.poll();
            let receipt = self.runtime.status(id, None).unwrap();
            if receipt.state != "pending" {
                return receipt;
            }
            assert!(Instant::now() < deadline, "message worker did not complete");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn execute(&mut self, request: MessageRequest) -> OperationReceipt {
        let id = request.request_id;
        assert_eq!(self.runtime.admit(request, None).unwrap().state, "pending");
        self.wait(id)
    }
    fn call(&mut self, action: MessageAction) -> Value {
        let receipt = self.execute(request(action));
        assert_eq!(receipt.state, "completed", "{:?}", receipt);
        receipt.result.unwrap()
    }
    fn send(&mut self, text: &str) -> Value {
        let target = self.surface();
        self.call(send(&target, text))
    }
    fn list(&mut self) -> Value {
        self.call(MessageAction::List {
            surface: None,
            states: vec![],
            limit: 32,
            cursor: None,
        })
    }
    fn register(&mut self) -> Value {
        self.call(MessageAction::Register {
            surface: self.surface(),
            provider: "test-neutral".into(),
            interactive: true,
        })
    }
    fn consumer(&self, registration: &Value) -> (String, Uuid, String) {
        (
            self.surface(),
            registration["consumer"].as_str().unwrap().parse().unwrap(),
            registration["token"].as_str().unwrap().into(),
        )
    }
    fn stop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.runtime.poll_shutdown().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Inbox {
    fn drop(&mut self) {
        self.stop();
    }
}
fn request(action: MessageAction) -> MessageRequest {
    MessageRequest {
        request_id: Uuid::new_v4(),
        caller: CallerContext::default(),
        action,
    }
}
fn send(target: &str, body: &str) -> MessageAction {
    MessageAction::Send {
        target: Some(target.into()),
        reply_to: None,
        body: body.into(),
        sender_name: None,
        thread_id: None,
    }
}
fn id(value: &Value, key: &str) -> Uuid {
    value[key].as_str().unwrap().parse().unwrap()
}
#[test]
fn retained_request_ids_deduplicate_conflict_and_survive_worker_restart() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let request = request(send(&inbox.surface(), "once"));
    let first = inbox.execute(request.clone());
    let same = inbox.execute(request.clone());
    assert_eq!(first.result, same.result);
    let mut conflicting = request.clone();
    if let MessageAction::Send { body, .. } = &mut conflicting.action {
        *body = "other".into();
    }
    assert_eq!(inbox.execute(conflicting).state, "failed");
    assert_eq!(inbox.list()["messages"].as_array().unwrap().len(), 1);
    inbox.stop();
    inbox.runtime = Runtime::start(Some(dir.0.clone())).unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    assert_eq!(inbox.wait(request.request_id).result, first.result);
    let before = inbox.model.snapshot();
    inbox
        .model
        .apply(Action::CloseWorkspace {
            workspace_id: before.workspaces[0].id,
        })
        .unwrap();
    inbox
        .runtime
        .commit_model(&before, &inbox.model.snapshot())
        .unwrap();
    let after_close = inbox.execute(request);
    assert_eq!(
        after_close.state, "completed",
        "retained replay is receipt retrieval after target closure: {:?}",
        after_close.error
    );
    assert_eq!(after_close.result, first.result);
    assert_eq!(inbox.wait(Uuid::new_v4()).state, "unknown_or_expired");
}
#[test]
fn consumer_handoff_ack_and_exact_read_turn_leave_new_arrivals_queued() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let first = inbox.send("first");
    let registration = inbox.register();
    let (surface, consumer, token) = inbox.consumer(&registration);
    let claimed = inbox.call(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    let ids = vec![id(&first["message"], "id")];
    assert_eq!(claimed["messages"][0]["id"], first["message"]["id"]);
    let second = inbox.send("later");
    let ack = MessageAction::Ack {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
        lease_id: id(&claimed, "lease_id"),
        message_ids: ids.clone(),
    };
    assert_eq!(inbox.call(ack.clone())["state"], "delivered");
    assert_eq!(inbox.call(ack)["state"], "delivered");
    inbox.call(MessageAction::ReadTurn {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
        ids,
    });
    let rows = inbox.list();
    assert_eq!(rows["messages"][0]["state"], "read");
    assert_eq!(rows["messages"][1]["state"], "queued");
    assert_eq!(rows["messages"][1]["id"], second["message"]["id"]);
    let wrong = inbox.execute(request(MessageAction::ReadTurn {
        surface,
        consumer,
        token,
        ids: vec![id(&second["message"], "id")],
    }));
    assert_eq!(wrong.state, "failed");
    let bytes = fs::read_to_string(dir.0.join(JOURNAL_NAME)).unwrap();
    assert!(
        !bytes.contains(registration["token"].as_str().unwrap()),
        "plaintext capability persisted"
    );
}
#[test]
fn off_before_handoff_fails_reservation_and_on_never_resurrects() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    inbox.send("pending");
    let registration = inbox.register();
    let (surface, consumer, token) = inbox.consumer(&registration);
    let claim = request(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token,
    });
    let claim_id = claim.request_id;
    inbox.runtime.admit(claim, None).unwrap();
    // Wait only for reservation completion; do not invoke status/handoff.
    let deadline = Instant::now() + Duration::from_secs(5);
    while inbox.runtime.pending.contains(&claim_id) {
        inbox.runtime.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(inbox.runtime.receipts[&claim_id].state, "awaiting_handoff");
    inbox.call(MessageAction::Settings {
        scope: SettingScope::Surface,
        target: Some(surface.clone()),
        enabled: Some(false),
    });
    assert_eq!(inbox.wait(claim_id).state, "failed");
    assert_eq!(inbox.list()["messages"][0]["state"], "failed");
    assert_eq!(
        inbox.execute(request(send(&surface, "refused"))).state,
        "failed"
    );
    inbox.call(MessageAction::Settings {
        scope: SettingScope::Surface,
        target: Some(surface),
        enabled: Some(true),
    });
    assert_eq!(inbox.list()["messages"][0]["state"], "failed");
}
#[test]
fn handed_lease_can_ack_after_off_and_superseded_consumer_cannot_claim() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let first = inbox.send("handed");
    let registration = inbox.register();
    let (surface, consumer, token) = inbox.consumer(&registration);
    let claim_request = request(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    let claim_id = claim_request.request_id;
    let claim = inbox.execute(claim_request).result.unwrap();
    inbox.call(MessageAction::Settings {
        scope: SettingScope::Global,
        target: None,
        enabled: Some(false),
    });
    inbox.call(MessageAction::Ack {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
        lease_id: id(&claim, "lease_id"),
        message_ids: vec![id(&first["message"], "id")],
    });
    assert_eq!(inbox.list()["messages"][0]["state"], "delivered");
    assert_eq!(
        inbox.wait(claim_id).state,
        "failed",
        "off must prohibit renewed context exposure while allowing the prior exact ack"
    );
    inbox.register();
    assert_eq!(
        inbox
            .execute(request(MessageAction::Claim {
                surface,
                consumer,
                token
            }))
            .state,
        "failed"
    );
}
#[test]
fn concurrent_writers_preserve_both_messages_and_cross_runtime_supersession() {
    let dir = Directory::new();
    let mut a = Inbox::new(&dir.0);
    let mut b = Inbox::new(&dir.0);
    b.model = a.model.clone();
    b.runtime.observe(b.model.snapshot(), vec![]);
    let one = request(send(&a.surface(), "one"));
    let two = request(send(&a.surface(), "two"));
    a.runtime.admit(one.clone(), None).unwrap();
    b.runtime.admit(two.clone(), None).unwrap();
    assert_eq!(a.wait(one.request_id).state, "completed");
    assert_eq!(b.wait(two.request_id).state, "completed");
    assert_eq!(a.list()["messages"].as_array().unwrap().len(), 2);
    let registration = a.register();
    let (surface, consumer, token) = a.consumer(&registration);
    let claim = a.call(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    // Another live runtime reading the same journal must preserve this active foreign lease.
    assert_eq!(b.list()["messages"].as_array().unwrap().len(), 2);
    let duplicate = a.call(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    assert_eq!(duplicate["messages"], json!([]));
    b.register();
    assert_eq!(
        a.execute(request(MessageAction::Ack {
            surface,
            consumer,
            token,
            lease_id: id(&claim, "lease_id"),
            message_ids: claim["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| id(m, "id"))
                .collect()
        }))
        .state,
        "failed"
    );
}
#[test]
fn corrupt_and_symlinked_journals_fail_without_replacement() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let path = dir.0.join(JOURNAL_NAME);
    fs::write(&path, b"{broken").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        inbox.execute(request(send(&inbox.surface(), "no"))).state,
        "failed"
    );
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    fs::remove_file(&path).unwrap();
    let elsewhere = dir.0.join("unrelated");
    fs::write(&elsewhere, b"untouched").unwrap();
    symlink(&elsewhere, &path).unwrap();
    assert_eq!(
        inbox.execute(request(send(&inbox.surface(), "no"))).state,
        "failed"
    );
    assert_eq!(fs::read(&elsewhere).unwrap(), b"untouched");
}
#[test]
fn sender_reply_routing_and_workspace_opt_out_survive_live_move() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let recipient = inbox.surface();
    let before = inbox.model.snapshot();
    let recipient_surface = before.workspaces[0].root.panes()[0].selected_surface;
    inbox
        .model
        .apply(Action::CreateSurface {
            pane_id: before.workspaces[0].root.panes()[0].id,
        })
        .unwrap();
    inbox
        .model
        .apply(Action::CreateWorkspace {
            name: "sender".into(),
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    let model = inbox.model.snapshot();
    let sender = &model.workspaces[1];
    let sender_surface = sender.root.panes()[0].selected_surface;
    let sender_uuid = model.object_uuid(TargetKind::Surface, sender_surface.0);
    let sender_workspace = model.object_uuid(TargetKind::Workspace, sender.id.0);
    let mut original = request(send(&recipient, "question"));
    original.caller = CallerContext {
        workspace: Some(sender_workspace.to_string().parse::<TargetRef>().unwrap()),
        surface: Some(sender_uuid.to_string().parse().unwrap()),
        ..Default::default()
    };
    let original = inbox.execute(original).result.unwrap();
    inbox
        .model
        .apply(Action::RenameWorkspace {
            workspace_id: sender.id,
            name: "renamed".into(),
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    let reply = inbox.call(MessageAction::Send {
        target: None,
        reply_to: Some(id(&original["message"], "id")),
        body: "answer".into(),
        sender_name: None,
        thread_id: None,
    });
    assert_eq!(
        reply["message"]["recipient"]["surface_id"],
        sender_uuid.to_string()
    );
    assert_eq!(
        reply["message"]["thread_id"],
        original["message"]["thread_id"]
    );
    let recipient_workspace = model.object_uuid(TargetKind::Workspace, model.workspaces[0].id.0);
    inbox
        .model
        .apply(Action::MoveSurface {
            surface_id: recipient_surface,
            pane_id: sender.root.panes()[0].id,
            index: 0,
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    inbox.call(MessageAction::Settings {
        scope: SettingScope::Workspace,
        target: Some(recipient_workspace.to_string()),
        enabled: Some(false),
    });
    let rows = inbox.list();
    assert_eq!(rows["messages"][0]["state"], "failed");
    assert_eq!(rows["messages"][1]["state"], "queued");
}

#[test]
fn unread_capacity_refusal_preserves_journal_bytes_and_settings_require_evidence() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    for n in 0..128 {
        inbox.send(&format!("retained {n}"));
    }
    let bytes = fs::read(dir.0.join(JOURNAL_NAME)).unwrap();
    assert_eq!(
        inbox
            .execute(request(send(&inbox.surface(), "overflow")))
            .state,
        "failed"
    );
    assert_eq!(fs::read(dir.0.join(JOURNAL_NAME)).unwrap(), bytes);
    assert_eq!(
        inbox
            .execute(request(MessageAction::Settings {
                scope: SettingScope::Surface,
                target: None,
                enabled: Some(false)
            }))
            .state,
        "failed"
    );
    let listed = inbox.list();
    let first = id(&listed["messages"][0], "id");
    inbox.call(MessageAction::MarkRead { ids: vec![first] });
    inbox.send("prunes only settled history");
    assert!(
        !inbox.list()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| id(m, "id") == first)
    );
}
#[test]
fn request_horizon_retains_unsettled_send_and_truthfully_expires_settled_evidence() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let unsettled = inbox.send("unsettled");
    let first_setting = request(MessageAction::Settings {
        scope: SettingScope::Global,
        target: None,
        enabled: Some(true),
    });
    let first_id = first_setting.request_id;
    assert_eq!(inbox.execute(first_setting).state, "completed");
    for _ in 0..257 {
        inbox.call(MessageAction::Settings {
            scope: SettingScope::Global,
            target: None,
            enabled: Some(true),
        });
    }
    assert_eq!(inbox.wait(first_id).state, "unknown_or_expired");
    assert_eq!(
        inbox
            .wait(id(&unsettled["message"], "request_id"))
            .result
            .unwrap()["message"]["id"],
        unsettled["message"]["id"]
    );
}
#[test]
fn expired_lease_cannot_ack_and_reclaim_uses_same_message_identity() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let sent = inbox.send("retry after lost ack");
    let registered = inbox.register();
    let (surface, consumer, token) = inbox.consumer(&registered);
    let claim = inbox.call(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    thread::sleep(Duration::from_millis(10100));
    assert_eq!(
        inbox
            .execute(request(MessageAction::Ack {
                surface: surface.clone(),
                consumer,
                token: token.clone(),
                lease_id: id(&claim, "lease_id"),
                message_ids: vec![id(&sent["message"], "id")]
            }))
            .state,
        "failed"
    );
    let again = inbox.call(MessageAction::Claim {
        surface,
        consumer,
        token,
    });
    assert_ne!(claim["lease_id"], again["lease_id"]);
    assert_eq!(again["messages"][0]["id"], sent["message"]["id"]);
}
#[test]
fn exact_close_and_session_change_invalidate_handoff_without_using_focus() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let sent = inbox.send("close target");
    let registered = inbox.register();
    let (surface, consumer, token) = inbox.consumer(&registered);
    let claim = request(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    let claim_id = claim.request_id;
    inbox.runtime.admit(claim, None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while inbox.runtime.pending.contains(&claim_id) {
        inbox.runtime.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let mut model = inbox.model.snapshot();
    if let zmux_workspace_core::PaneTree::Leaf { pane } = &mut model.workspaces[0].root {
        pane.surfaces[0].session_id = Some(zmux_workspace_core::SessionId(9000));
    }
    inbox.runtime.observe(model, vec![]);
    assert_eq!(inbox.wait(claim_id).state, "failed");
    assert_eq!(
        inbox
            .execute(request(MessageAction::Poll {
                surface: surface.clone(),
                consumer,
                token,
                held: None
            }))
            .state,
        "failed"
    );
    // Absence alone does not close history; only explicit exact removal does.
    inbox.runtime.observe(Snapshot::default(), vec![]);
    inbox
        .runtime
        .observe(inbox.model.snapshot(), vec![surface.parse().unwrap()]);
    let rows = inbox.list();
    assert_eq!(rows["messages"][0]["id"], sent["message"]["id"]);
    assert_eq!(rows["messages"][0]["state"], "failed");
}
#[test]
fn pending_worker_credits_and_shutdown_remain_bounded_during_locked_io() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let directory = crate::persistence::state_directory(&dir.0).unwrap();
    let lock = crate::persistence::lock_named(&directory, LOCK_NAME).unwrap();
    for n in 0..8 {
        inbox
            .runtime
            .admit(
                request(send(&inbox.surface(), &format!("accepted {n}"))),
                None,
            )
            .unwrap();
    }
    assert!(
        inbox
            .runtime
            .admit(request(send(&inbox.surface(), "over capacity")), None)
            .is_err()
    );
    // Ordinary core actions proceed while the message worker is blocked on the file lock.
    inbox
        .model
        .apply(Action::CreateWorkspace {
            name: "unrelated stays live".into(),
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    assert!(!inbox.runtime.poll_shutdown().unwrap());
    drop(lock);
    inbox.stop();
    inbox.runtime = Runtime::start(Some(dir.0.clone())).unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    assert_eq!(inbox.list()["messages"].as_array().unwrap().len(), 8);
}
#[test]
fn sender_surface_ownership_overrides_stale_ambient_window_hint() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let before = inbox.model.snapshot();
    let old_window = before.windows[0].id;
    let source = before.workspaces[0].id;
    let source_surface = inbox.surface();
    inbox
        .model
        .apply(Action::CreateWindow { focus: true })
        .unwrap();
    let current = inbox.model.snapshot();
    let new_window = current.focused_window.unwrap();
    inbox
        .model
        .apply(Action::MoveWorkspaceToWindow {
            workspace_id: source,
            destination_window_id: new_window,
            index: None,
            focus: false,
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    let mut send = request(send(&source_surface, "CRLF\r\n preserved\t"));
    send.caller = CallerContext {
        window: Some(
            before
                .object_uuid(TargetKind::Window, old_window.0)
                .to_string()
                .parse()
                .unwrap(),
        ),
        workspace: Some(
            before
                .object_uuid(TargetKind::Workspace, source.0)
                .to_string()
                .parse()
                .unwrap(),
        ),
        surface: Some(source_surface.parse().unwrap()),
        ..Default::default()
    };
    let receipt = inbox.execute(send);
    assert_eq!(receipt.state, "completed");
    let sent = receipt.result.unwrap();
    assert_eq!(sent["message"]["sender"]["surface_id"], source_surface);
    assert_eq!(sent["message"]["body"], "CRLF\r\n preserved\t");
}
#[test]
fn explicit_targets_reject_ambiguity_browser_and_history_but_keep_failed_inbox_filterable() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let first = inbox.send("retained failure");
    let old_surface = inbox.surface();
    inbox
        .model
        .apply(Action::CreateWorkspace {
            name: "recipient other".into(),
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    assert_eq!(
        inbox.execute(request(send("reci", "ambiguous"))).state,
        "failed"
    );
    assert_eq!(
        inbox
            .execute(request(send(&Uuid::new_v4().to_string(), "stale")))
            .state,
        "failed"
    );
    let model = inbox.model.snapshot();
    let pane = model.workspaces[1].root.panes()[0].id;
    inbox
        .model
        .apply(Action::CreateBrowser {
            profile_id: None,
            pane_id: pane,
            url: "about:blank".into(),
            focus: true,
        })
        .unwrap();
    let model = inbox.model.snapshot();
    let browser = model.object_uuid(
        TargetKind::Surface,
        model.workspaces[1].root.panes()[0].selected_surface.0,
    );
    inbox.runtime.observe(model, vec![]);
    assert_eq!(
        inbox
            .execute(request(send(&browser.to_string(), "not terminal")))
            .state,
        "failed"
    );
    let mut history = inbox.model.snapshot();
    if let zmux_workspace_core::PaneTree::Leaf { pane } = &mut history.workspaces[0].root {
        pane.surfaces[0].session_id = None;
    }
    inbox.runtime.observe(history, vec![]);
    assert_eq!(
        inbox.execute(request(send(&old_surface, "not live"))).state,
        "failed"
    );
    let before = inbox.model.snapshot();
    inbox
        .model
        .apply(Action::CloseWorkspace {
            workspace_id: before.workspaces[0].id,
        })
        .unwrap();
    inbox
        .runtime
        .commit_model(&before, &inbox.model.snapshot())
        .unwrap();
    let rows = inbox.call(MessageAction::List {
        surface: Some(old_surface),
        states: vec![MessageState::Failed],
        limit: 32,
        cursor: None,
    });
    assert_eq!(rows["messages"][0]["id"], first["message"]["id"]);
}
#[test]
fn frozen_admission_model_prevents_queued_workspace_focus_retargeting() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let original = inbox.surface();
    let before = inbox.model.snapshot();
    let directory = crate::persistence::state_directory(&dir.0).unwrap();
    let lock = crate::persistence::lock_named(&directory, LOCK_NAME).unwrap();
    let sending = request(send("recipient", "pinned before worker IO"));
    let operation = sending.request_id;
    inbox.runtime.admit(sending, None).unwrap();
    inbox
        .model
        .apply(Action::CreateSurface {
            pane_id: before.workspaces[0].root.panes()[0].id,
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    drop(lock);
    let sent = inbox.wait(operation);
    assert_eq!(sent.state, "completed");
    assert_eq!(
        sent.result.unwrap()["message"]["recipient"]["surface_id"],
        original
    );
}
#[test]
fn sender_surface_move_overrides_stale_ambient_workspace_hint() {
    let dir = Directory::new();
    let mut inbox = Inbox::new(&dir.0);
    let source = inbox.model.snapshot();
    let sender_surface = source.workspaces[0].root.panes()[0].selected_surface;
    let sender_uuid = inbox.surface();
    inbox
        .model
        .apply(Action::CreateWorkspace {
            name: "moved sender owner".into(),
        })
        .unwrap();
    let next = inbox.model.snapshot();
    let destination = &next.workspaces[1];
    inbox
        .model
        .apply(Action::MoveSurface {
            surface_id: sender_surface,
            pane_id: destination.root.panes()[0].id,
            index: 0,
        })
        .unwrap();
    inbox.runtime.observe(inbox.model.snapshot(), vec![]);
    let mut sending = request(send(&sender_uuid, "same exact sender"));
    sending.caller = CallerContext {
        workspace: Some(
            source
                .object_uuid(TargetKind::Workspace, source.workspaces[0].id.0)
                .to_string()
                .parse()
                .unwrap(),
        ),
        surface: Some(sender_uuid.parse().unwrap()),
        ..Default::default()
    };
    let sent = inbox.execute(sending);
    assert_eq!(sent.state, "completed");
    let sent = sent.result.unwrap();
    assert_eq!(sent["message"]["sender"]["surface_id"], sender_uuid);
    assert_eq!(
        sent["message"]["sender"]["workspace_id"],
        next.object_uuid(TargetKind::Workspace, destination.id.0)
            .to_string()
    );
}
#[test]
fn completed_cached_claim_rechecks_held_and_real_expiry_before_exposing_text() {
    let mut stale_cases = Vec::new();
    for case in ["held", "expired"] {
        let dir = Directory::new();
        let mut inbox = Inbox::new(&dir.0);
        inbox.send("must remain withheld after the final fence");
        let registration = inbox.register();
        let (surface, consumer, token) = inbox.consumer(&registration);
        let claim = request(MessageAction::Claim {
            surface: surface.clone(),
            consumer,
            token: token.clone(),
        });
        let operation = claim.request_id;
        inbox.runtime.admit(claim, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while inbox.runtime.pending.contains(&operation) {
            inbox.runtime.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(inbox.runtime.receipts[&operation].state, "awaiting_handoff");
        assert_eq!(
            inbox.runtime.status(operation, None).unwrap().state,
            "pending"
        );
        while inbox.runtime.pending.contains(&operation) {
            inbox.runtime.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let cached = &inbox.runtime.receipts[&operation];
        assert_eq!(cached.state, "completed");
        assert_eq!(
            cached.result.as_ref().unwrap()["messages"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        if case == "held" {
            inbox.call(MessageAction::Poll {
                surface,
                consumer,
                token,
                held: Some(true),
            });
        } else {
            thread::sleep(Duration::from_millis(10100));
        }
        let outcome = inbox.runtime.status(operation, None).unwrap();
        if outcome.state != "failed" || outcome.result.is_some() {
            stale_cases.push(case);
        }
    }
    assert!(
        stale_cases.is_empty(),
        "completed cached claim exposed stale context after {stale_cases:?}"
    );
}
#[test]
fn committed_handoff_allows_one_inflight_response_after_foreign_supersession_but_no_new_authority()
{
    let dir = Directory::new();
    let mut a = Inbox::new(&dir.0);
    let mut b = Inbox::new(&dir.0);
    b.model = a.model.clone();
    b.runtime.observe(b.model.snapshot(), vec![]);
    let sent = a.send("authorized before foreign registration");
    let registration = a.register();
    let (surface, consumer, token) = a.consumer(&registration);
    let claim = request(MessageAction::Claim {
        surface: surface.clone(),
        consumer,
        token: token.clone(),
    });
    let operation = claim.request_id;
    a.runtime.admit(claim, None).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.runtime.pending.contains(&operation) {
        a.runtime.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(a.runtime.receipts[&operation].state, "awaiting_handoff");
    assert_eq!(a.runtime.status(operation, None).unwrap().state, "pending");
    while a.runtime.pending.contains(&operation) {
        a.runtime.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(a.runtime.receipts[&operation].state, "completed");
    // The lock-protected, fsynced handoff authorization precedes foreign registration.
    // The first already-authorized response can still arrive inside its original TTL.
    let newer = b.register();
    assert_ne!(newer["consumer"], registration["consumer"]);
    let inflight = a.runtime.status(operation, None).unwrap();
    assert_eq!(inflight.state, "completed");
    let response = inflight.result.unwrap();
    assert_eq!(response["messages"][0]["id"], sent["message"]["id"]);
    assert_eq!(
        a.execute(request(MessageAction::Ack {
            surface,
            consumer,
            token,
            lease_id: id(&response, "lease_id"),
            message_ids: vec![id(&sent["message"], "id")]
        }))
        .state,
        "failed"
    );
    let repeated = a.wait(operation);
    assert_eq!(repeated.state, "failed");
    assert!(repeated.result.is_none());
    assert_eq!(a.list()["messages"][0]["state"], "queued");
}
