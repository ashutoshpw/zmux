//! Public notification grammar exercised through the actual CLI and desktop socket.
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct App {
    socket: PathBuf,
    server: ControlServer,
    desktop: Arc<Desktop>,
}
impl App {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let socket = std::env::temp_dir()
            .join(format!(
                "zn{:x}{nonce:x}{:x}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("c");
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd: None,
            })
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let app = Self {
            socket,
            server,
            desktop,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.run(&["list-workspaces"])["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(
                Instant::now() < deadline,
                "starter workspace did not appear"
            );
            thread::sleep(Duration::from_millis(10));
        }
        app
    }
    fn output(&self, args: &[&str], caller: Option<(&str, &str)>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command
            .arg("--socket")
            .arg(&self.socket)
            .args(["--timeout", "5000"])
            .args(args)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .env_remove("ZMUX_SOCKET_PASSWORD_FILE");
        if let Some((workspace, surface)) = caller {
            command
                .env("ZMUX_WORKSPACE_ID", workspace)
                .env("ZMUX_SURFACE_ID", surface);
        }
        command.output().unwrap()
    }
    fn call(&self, args: &[&str], caller: Option<(&str, &str)>) -> Value {
        let output = self.output(args, caller);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn run(&self, args: &[&str]) -> Value {
        self.call(args, None)
    }
    fn notify(&self, surface: &str, title: &str) -> u64 {
        self.run(&["notify", "--surface", surface, "--title", title]);
        self.run(&["list-notifications"])["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["title"] == title)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.server.shutdown(Duration::from_secs(2)).unwrap();
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
        fs::remove_dir(self.socket.parent().unwrap()).unwrap();
    }
}
fn id(value: &Value, key: &str) -> String {
    value[key].as_u64().unwrap().to_string()
}
fn ids(list: &Value) -> Vec<u64> {
    list["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_u64().unwrap())
        .collect()
}
fn create(app: &App, title: &str) -> (String, String, String) {
    let receipt = app.run(&["new-workspace", title]);
    (
        id(&receipt, "focused_workspace"),
        id(&receipt, "focused_pane"),
        id(&receipt, "focused_surface"),
    )
}

#[test]
fn notify_defaults_and_content_options_reach_real_attention_owner() {
    let app = App::new();
    let (_, _, surface) = create(&app, "content target");
    app.run(&["notify", "--surface", &surface]);
    let list = app.run(&["list-notifications"]);
    let item = &list["notifications"][0];
    assert_eq!(item["title"], "Notification");
    assert_eq!(item["subtitle"], "");
    assert_eq!(item["body"], "");
    assert!(item["desktop"].is_null());
    assert!(list["epoch"].is_string());
    assert!(list["cursor"].is_u64());
    app.run(&[
        "notify",
        "--body",
        "exact body",
        "--subtitle",
        "subtitle",
        "--desktop",
        "false",
        "--surface",
        &surface,
    ]);
    let list = app.run(&["list-notifications"]);
    let item = list["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["body"] == "exact body")
        .unwrap();
    assert_eq!(item["title"], "Notification");
    assert_eq!(item["subtitle"], "subtitle");
    assert_eq!(item["desktop"], false);
    assert_eq!(item["delivery"]["state"], "suppressed");
    app.run(&[
        "notify",
        "--surface",
        &surface,
        "--desktop",
        "true",
        "--title",
        "desktop requested",
    ]);
    let list = app.run(&["list-notifications"]);
    assert_eq!(
        list["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["title"] == "desktop requested")
            .unwrap()["desktop"],
        true
    );
    app.run(&["notify", "--clear"]);
    assert!(ids(&app.run(&["list-notifications"])).is_empty());
}

#[test]
fn explicit_workspace_keeps_same_workspace_terminal_caller_provenance() {
    let app = App::new();
    let (workspace, pane, original) = create(&app, "caller owner");
    let selected = id(&app.run(&["new-surface", &pane]), "focused_surface");
    assert_ne!(original, selected);
    app.call(
        &[
            "notify",
            "--workspace",
            &workspace,
            "--title",
            "hidden caller",
        ],
        Some((&workspace, &original)),
    );
    let list = app.run(&["list-notifications"]);
    let row = list["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"] == "hidden caller")
        .unwrap();
    assert_eq!(row["surface_id"].as_u64().unwrap().to_string(), original);

    let (foreign_workspace, _, foreign_surface) = create(&app, "other caller");
    app.call(
        &[
            "notify",
            "--workspace",
            &workspace,
            "--title",
            "explicit workspace",
        ],
        Some((&foreign_workspace, &foreign_surface)),
    );
    app.run(&["notify", "--title", "external caller"]);
    let list = app.run(&["list-notifications"]);
    for (title, expected_workspace) in [
        ("explicit workspace", &workspace),
        ("external caller", &foreign_workspace),
    ] {
        let row = list["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["title"] == title)
            .unwrap();
        assert!(
            row["surface_id"].is_null(),
            "{title} borrowed a selected terminal"
        );
        assert_eq!(
            row["workspace_id"].as_u64().unwrap().to_string(),
            *expected_workspace
        );
        assert_eq!(row["target"]["destination"]["kind"], "workspace");
    }
}

#[test]
fn scoped_read_dismiss_and_clear_keep_other_workspaces_intact() {
    let app = App::new();
    let (workspace, pane, a) = create(&app, "A");
    let b = id(&app.run(&["new-surface", &pane]), "focused_surface");
    let (_, _, foreign) = create(&app, "B");
    let a_id = app.notify(&a, "A one");
    let b_id = app.notify(&b, "A two");
    let foreign_id = app.notify(&foreign, "B one");
    let read = app.run(&[
        "mark-notification-read",
        "--workspace",
        &workspace,
        "--surface",
        &a,
    ]);
    assert_eq!(read["affected_ids"], serde_json::json!([a_id]));
    assert_eq!(read["affected_count"], 1);
    let list = app.run(&["list-notifications"]);
    assert_eq!(list["unread"], 2);
    assert!(
        list["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["read"] == (item["id"] == a_id))
    );
    app.run(&["dismiss-notification", "--all-read"]);
    assert_eq!(
        ids(&app.run(&["list-notifications"])),
        vec![b_id, foreign_id]
    );
    let newer_a = app.notify(&a, "A newer");
    let read = app.call(
        &["mark-notification-read", "--surface", &a],
        Some((&workspace, &foreign)),
    );
    assert_eq!(read["affected_ids"], serde_json::json!([newer_a]));
    app.run(&["mark-notification-read", "--workspace", &workspace]);
    assert_eq!(app.run(&["list-notifications"])["unread"], 1);
    app.run(&["dismiss-notification", "--id", &b_id.to_string()]);
    app.run(&[
        "notify",
        "--clear",
        "--workspace",
        &workspace,
        "--surface",
        &a,
    ]);
    assert!(!ids(&app.run(&["list-notifications"])).contains(&newer_a));
    assert_eq!(ids(&app.run(&["list-notifications"])), vec![foreign_id]);
    app.call(
        &["mark-notification-read", "--all"],
        Some(("invalid caller workspace", "invalid caller surface")),
    );
    assert_eq!(app.run(&["list-notifications"])["unread"], 0);
    app.call(
        &["clear-notifications"],
        Some(("", "invalid caller surface")),
    );
    assert!(ids(&app.run(&["list-notifications"])).is_empty());
}

#[test]
fn caller_clear_and_observed_cutoff_preserve_unrelated_and_newer_arrivals() {
    let app = App::new();
    let (workspace, pane, a) = create(&app, "caller A");
    let b = id(&app.run(&["new-surface", &pane]), "focused_surface");
    let (_, _, foreign) = create(&app, "caller B");
    let a_id = app.notify(&a, "older A");
    let foreign_id = app.notify(&foreign, "foreign B");
    app.run(&[
        "clear-notifications",
        "--workspace",
        &workspace,
        "--through",
        "0",
    ]);
    assert_eq!(
        ids(&app.run(&["list-notifications"])),
        vec![a_id, foreign_id]
    );
    let cutoff = app.run(&["list-notifications"])["cursor"].as_u64().unwrap();
    let b_id = app.notify(&b, "newer A");
    app.run(&[
        "clear-notifications",
        "--workspace",
        &workspace,
        "--through",
        &cutoff.to_string(),
    ]);
    let remaining = ids(&app.run(&["list-notifications"]));
    assert!(!remaining.contains(&a_id));
    assert_eq!(remaining, vec![foreign_id, b_id]);
    let a_id = app.notify(&a, "caller exact surface");
    app.call(&["notify", "--clear"], Some((&workspace, &a)));
    assert!(!ids(&app.run(&["list-notifications"])).contains(&a_id));
    assert_eq!(
        ids(&app.run(&["list-notifications"])),
        vec![foreign_id, b_id]
    );
    app.call(&["clear-notifications"], Some((&workspace, &a)));
    assert_eq!(ids(&app.run(&["list-notifications"])), vec![foreign_id]);
    app.call(
        &["mark-notification-read", "--id", &foreign_id.to_string()],
        Some(("invalid caller workspace", "invalid caller surface")),
    );
    assert_eq!(app.run(&["list-notifications"])["unread"], 0);
}

#[test]
fn notification_selector_conflicts_fail_before_socket_use() {
    for args in [
        vec!["notify", "--desktop", "yes"],
        vec!["notify", "--title", "a", "--title", "b"],
        vec!["notify", "--clear", "--body", "conflict"],
        vec!["notify", "--clear", "--clear"],
        vec!["mark-notification-read"],
        vec!["mark-notification-read", "--id", "1", "--all"],
        vec!["mark-notification-read", "--workspace", "1", "--all"],
        vec!["mark-notification-read", "--surface", "1"],
        vec!["dismiss-notification", "--id", "1", "--all-read"],
        vec!["dismiss-notification", "--all-read", "--workspace", "1"],
        vec!["clear-notifications", "--through", "1", "--through", "2"],
        vec!["clear-notifications", "--pane", "1"],
        vec!["clear-notifications", "--surface", "1"],
        vec!["notify", "--desktop", "false", "--desktop", "true"],
        vec!["notify", "--subtitle", "a", "--subtitle", "b"],
        vec!["notify", "--surface", "1", "--tab", "2"],
        vec!["mark-notification-read", "--id", "1", "--id", "2"],
        vec!["mark-notification-read", "--all", "--all"],
        vec!["dismiss-notification", "--all-read", "--all-read"],
        vec![
            "clear-notifications",
            "--expect-revision",
            "0",
            "--expect-revision",
            "1",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(["--socket", "relative-socket-must-not-be-used"])
            .args(&args)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(
            !error.contains("socket path must be absolute"),
            "connected before rejecting {args:?}: {error}"
        );
    }
}
