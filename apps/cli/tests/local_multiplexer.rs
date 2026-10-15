//! Public headless lifecycle and real PTY attach; never touches the default tmux server.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime as desktop_runtime;
use zmux_session_runtime::{Launch, Session, State};
struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    verified_profile: std::sync::OnceLock<()>,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/tmp").join(format!("zmc-{}", uuid_string()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(".config"))
            .unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(".config/zmux"))
            .unwrap();
        let binary = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("tmux"))
            .find(|p| p.is_file())
            .expect("real tmux is required");
        Self {
            root: root.canonicalize().unwrap(),
            binary,
            verified_profile: std::sync::OnceLock::new(),
        }
    }
    fn socket(&self) -> PathBuf {
        self.root.join(".config/zmux/local-tmux/server.sock")
    }
    fn environment(&self) -> Vec<(String, String)> {
        vec![
            (
                "ZMUX_LOCAL_TMUX_ROOT".into(),
                self.root
                    .join(".config/zmux/local-tmux")
                    .to_str()
                    .unwrap()
                    .into(),
            ),
            (
                "ZMUX_LOCAL_TMUX_BIN".into(),
                self.binary.to_str().unwrap().into(),
            ),
            (
                "ZMUX_SOCKET_PATH".into(),
                "relative-unavailable-app-socket".into(),
            ),
            (
                "ZMUX_SOCKET_PASSWORD_FILE".into(),
                self.root.join("missing-password").to_str().unwrap().into(),
            ),
            (
                "ZMUX_WORKSPACE_ID".into(),
                "invalid-caller-workspace".into(),
            ),
            ("ZMUX_SURFACE_ID".into(), "invalid-caller-surface".into()),
        ]
    }
    fn output(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(args)
            .envs(self.environment())
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        self.verify_profile();
        let o = self.output(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn attach(&self, args: &[&str]) -> Session {
        self.verify_profile();
        Session::spawn_with_environment(
            Launch {
                program: env!("CARGO_BIN_EXE_zmux").into(),
                args: args.iter().map(|s| (*s).into()).collect(),
                cwd: Some(self.root.clone()),
                size: zmux_session_runtime::default_size(),
            },
            &self.environment(),
        )
        .unwrap()
    }
    fn verify_profile(&self) {
        self.verified_profile.get_or_init(|| {
            // Refuse to spawn any owner/client if a configuration regression
            // silently selected the real user's default installation instead.
            let output = self.output(&["local-tmux", "list"]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(listing["socket_path"], self.socket().to_str().unwrap());
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if fs::symlink_metadata(self.socket()).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = Command::new(&self.binary)
                .args(["-f", "/dev/null", "-S"])
                .arg(self.socket())
                .arg("kill-server")
                .output();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn uuid_string() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "public tmux lifecycle timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn headless_cli_attaches_detaches_and_reattaches_the_same_live_shell_without_app_socket() {
    let f = Fixture::new();
    let pidfile = f.root.join("owner-pid");
    let command = format!(
        "printf '%s' \"$$\" > '{}.pending'; mv '{}.pending' '{}'; exec /bin/sh -i",
        pidfile.to_str().unwrap(),
        pidfile.to_str().unwrap(),
        pidfile.to_str().unwrap()
    );
    let created = f.json(&[
        "local-tmux",
        "start",
        "work",
        "--detached",
        "--cwd",
        f.root.to_str().unwrap(),
        "--command",
        &command,
    ]);
    let id = created["session"]["id"].as_str().unwrap();
    assert_eq!(created["session"]["state"], "live");
    assert!(
        fs::symlink_metadata(f.socket())
            .unwrap()
            .file_type()
            .is_socket()
    );
    wait(|| pidfile.is_file());
    let pid = fs::read_to_string(&pidfile).unwrap();
    assert!(!pid.is_empty());
    assert_eq!(
        f.json(&["local-tmux", "list", "--json"])["sessions"][0]["id"],
        id
    );
    let mut first = f.attach(&["local-tmux", "attach", "--id", id, "--headless"]);
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    let first_input = f.root.join("first-client-input");
    let input = format!(
        "printf '%s' accepted > '{}.pending'; mv '{}.pending' '{}'\r",
        first_input.to_str().unwrap(),
        first_input.to_str().unwrap(),
        first_input.to_str().unwrap()
    );
    first.send_input(input.as_bytes()).unwrap();
    wait(|| fs::read_to_string(&first_input).is_ok_and(|value| value == "accepted"));
    assert_eq!(
        f.json(&["local-tmux", "detach", "--id", id])["detached_clients"],
        1
    );
    wait(|| first.snapshot().is_ok_and(|s| s.state == State::Exited));
    first.shutdown(Duration::from_secs(2)).unwrap();
    assert_eq!(
        f.json(&["local-tmux", "status", "--id", id])["session"]["state"],
        "live"
    );
    assert_eq!(fs::read_to_string(&pidfile).unwrap(), pid);
    let mut second = f.attach(&["tmux", "attach", "work", "--headless"]);
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    let confirmation = f.root.join("reattached-pid");
    let command = format!(
        "printf '%s' \"$$\" > '{}.pending'; mv '{}.pending' '{}'\r",
        confirmation.to_str().unwrap(),
        confirmation.to_str().unwrap(),
        confirmation.to_str().unwrap()
    );
    second.send_input(command.as_bytes()).unwrap();
    wait(|| confirmation.is_file());
    assert_eq!(fs::read_to_string(confirmation).unwrap(), pid);
    let child_status = f.root.join("owner-status");
    let command = format!(
        "'{}' local-tmux status --id {} > '{}.pending'; mv '{}.pending' '{}'\r",
        env!("CARGO_BIN_EXE_zmux"),
        id,
        child_status.display(),
        child_status.display(),
        child_status.display()
    );
    second.send_input(command.as_bytes()).unwrap();
    wait(|| child_status.is_file());
    let child_status: Value = serde_json::from_slice(&fs::read(child_status).unwrap()).unwrap();
    assert_eq!(child_status["session"]["id"], id);
    second.shutdown(Duration::from_secs(2)).unwrap();
    wait(|| {
        f.json(&["local-tmux", "status", "work"])["session"]["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    });
    assert_eq!(
        f.json(&["local-tmux", "close", "--id", id])["closed_id"],
        id
    );
    assert!(
        f.json(&["local-tmux", "list"])["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn offline_help_and_invalid_gui_options_alias_or_client_forms_do_not_open_app_or_profile() {
    let f = Fixture::new();
    for command in ["local-tmux", "tmux"] {
        let o = f.output(&[command, "--help"]);
        assert!(o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("--headless"));
    }
    for args in [
        vec!["local-tmux", "start", "work", "--detached", "--no-focus"],
        vec!["local-tmux", "attach", "work", "--focus", "maybe"],
        vec!["tmux", "list"],
        vec!["local-tmux", "detach", "work", "--client", "one"],
        vec![
            "local-tmux",
            "attach",
            "work",
            "--headless",
            "--workspace",
            "1",
        ],
        vec!["local-tmux", "start", "bad/name", "--detached"],
        vec!["local-tmux", "cleanup", "work"],
        vec!["local-tmux", "list", "--id", "bad"],
        vec!["local-tmux", "detach", "work", "--all", "--all"],
        vec!["local-tmux", "status", "work", "--id", "bad"],
    ] {
        let o = f.output(&args);
        assert!(!o.status.success(), "{args:?}");
        let error = String::from_utf8_lossy(&o.stderr);
        assert!(!error.contains("socket path must be absolute"));
        assert!(!error.contains("credential"));
        assert!(
            !f.root.join(".config/zmux/local-tmux").exists(),
            "profile touched for {args:?}"
        );
    }
    let o = f.output(&["tmux", "attach", "work", "--headless"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("requires terminal stdin and stdout"));
    assert!(!f.root.join(".config/zmux/local-tmux").exists());
    let invalid_root = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args(["local-tmux", "list"])
        .envs(f.environment())
        .env("ZMUX_LOCAL_TMUX_ROOT", "relative-profile")
        .output()
        .unwrap();
    assert!(!invalid_root.status.success());
    assert!(String::from_utf8_lossy(&invalid_root.stderr).contains("must be absolute paths"));
    assert!(!f.root.join(".config/zmux/local-tmux").exists());
}

struct Gui {
    desktop: std::sync::Arc<zmux_desktop_runtime::Desktop>,
    control: zmux_desktop_runtime::control::ControlServer,
    socket: PathBuf,
}
impl Gui {
    fn start(f: &Fixture) -> Self {
        f.verify_profile();
        use zmux_desktop_runtime::{Desktop, DesktopOptions, Shell, local_tmux::Profile};
        let parent = f.root.join(".config/zmux");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(f.root.join(".config"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let profile = zmux_remote_runtime::local_multiplexer::LocalTmux::new(
            parent.join("local-tmux"),
            f.binary.clone(),
        )
        .unwrap();
        let desktop = std::sync::Arc::new(
            Desktop::with_options(
                Shell {
                    program: "/bin/sh".into(),
                    args: vec!["-i".into()],
                    cwd: Some(f.root.clone()),
                },
                DesktopOptions {
                    state_directory: Some(f.root.join("state")),
                    local_tmux_profile: Profile::Explicit(profile),
                },
            )
            .unwrap(),
        );
        let socket = f.root.join("app.sock");
        let control = zmux_desktop_runtime::control::ControlServer::start_with_policy(
            &socket,
            desktop.clone(),
            zmux_desktop_runtime::control_policy::ControlPolicy::password(
                zmux_desktop_runtime::control_policy::ControlAccess::Full,
                zmux_desktop_runtime::control_policy::SocketPassword::new("tmux-test-only".into())
                    .unwrap(),
            ),
        )
        .unwrap();
        let gui = Self {
            desktop,
            control,
            socket,
        };
        wait(|| {
            gui.desktop.snapshot().is_ok_and(|s| {
                !s.persistence.startup_pending
                    && !s.browser_profiles.loading
                    && s.pending_launches == 0
                    && !s.model.workspaces.is_empty()
            })
        });
        gui
    }
    fn command(&self, f: &Fixture, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zmux"));
        command
            .args([
                "--socket",
                self.socket.to_str().unwrap(),
                "--password",
                "tmux-test-only",
                "--timeout",
                "10000",
            ])
            .args(args)
            .envs(f.environment())
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .env_remove("ZMUX_LOCAL_TMUX")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        command
    }
    fn output(&self, f: &Fixture, args: &[&str]) -> Output {
        self.command(f, args).output().unwrap()
    }
    fn json(&self, f: &Fixture, args: &[&str]) -> Value {
        let out = self.output(f, args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn snapshot(&self) -> std::sync::Arc<zmux_desktop_runtime::Snapshot> {
        desktop_snapshot::read(&self.desktop)
    }
    fn surface(&self, id: &str) -> u64 {
        self.snapshot()
            .model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .find(|s| {
                s.local_tmux
                    .as_ref()
                    .is_some_and(|i| i.id.to_string() == id)
            })
            .unwrap()
            .id
            .0
    }
}
impl Drop for Gui {
    fn drop(&mut self) {
        if let Err(e) = self.control.shutdown(Duration::from_secs(3)) {
            eprintln!("tmux fixture control shutdown: {e}");
        }
        if let Err(e) = self.desktop.shutdown(Duration::from_secs(5)) {
            eprintln!("tmux fixture desktop shutdown: {e}");
        }
    }
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn owner_cli(gui: &Gui, f: &Fixture, surface: u64, label: &str, args: &[String]) -> (i32, String) {
    let report = f.root.join(label);
    let command = format!(
        "{} --socket {} --password tmux-test-only {} > {} 2>&1; rc=$?; printf '%s' \"$rc\" > {}.pending; mv {}.pending {}.status\r",
        shell_quote(env!("CARGO_BIN_EXE_zmux")),
        shell_quote(gui.socket.to_str().unwrap()),
        args.iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" "),
        shell_quote(report.to_str().unwrap()),
        shell_quote(report.to_str().unwrap()),
        shell_quote(report.to_str().unwrap()),
        shell_quote(report.to_str().unwrap())
    );
    gui.json(
        f,
        &["send-text", "--surface", &surface.to_string(), &command],
    );
    let status = PathBuf::from(format!("{}.status", report.display()));
    wait(|| status.exists());
    (
        fs::read_to_string(status).unwrap().parse().unwrap(),
        fs::read_to_string(report).unwrap(),
    )
}
#[test]
fn app_cli_reuses_clients_reopens_and_restores_exact_owner_without_replaying_startup() {
    let f = Fixture::new();
    let gui = Gui::start(&f);
    let before = gui.snapshot();
    let old_focus = before.model.workspaces[0].root.panes()[0].selected_surface;
    let count = f.root.join("startup-count");
    let pidfile = f.root.join("durable-pid");
    let command = format!(
        "printf x >> {}; printf '%s' \"$$\" > {}.pending; mv {}.pending {}; exec /bin/sh -i",
        shell_quote(count.to_str().unwrap()),
        shell_quote(pidfile.to_str().unwrap()),
        shell_quote(pidfile.to_str().unwrap()),
        shell_quote(pidfile.to_str().unwrap())
    );
    let started = gui.json(
        &f,
        &[
            "local-tmux",
            "start",
            "persistent",
            "--cwd",
            f.root.to_str().unwrap(),
            "--command",
            &command,
            "--no-focus",
        ],
    );
    assert_eq!(started["state"], "completed");
    assert_eq!(started["owner_outcome"], "created");
    let id = started["identity"]["id"].as_str().unwrap();
    let original = gui.surface(id);
    assert_eq!(
        gui.snapshot().model.workspaces[0].root.panes()[0].selected_surface,
        old_focus
    );
    wait(|| fs::read_to_string(&count).is_ok_and(|s| s == "x"));
    let pid = fs::read_to_string(&pidfile).unwrap();
    let reused = gui.json(&f, &["tmux", "attach", "--id", id, "--no-focus"]);
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["surface"], started["surface"]);
    let extra = gui.json(
        &f,
        &[
            "local-tmux",
            "attach",
            "--id",
            id,
            "--new-client",
            "--no-focus",
        ],
    );
    assert_eq!(extra["reused"], false);
    assert_ne!(extra["surface"], started["surface"]);
    assert_eq!(
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let ambiguous = f.root.join("ambiguous-caller");
    let shell = format!(
        "{} --socket {} --password tmux-test-only set-status wrong ambiguous > {} 2>&1; printf '%s' \"$?\" > {}.status\r",
        shell_quote(env!("CARGO_BIN_EXE_zmux")),
        shell_quote(gui.socket.to_str().unwrap()),
        shell_quote(ambiguous.to_str().unwrap()),
        shell_quote(ambiguous.to_str().unwrap())
    );
    gui.json(
        &f,
        &["send-text", "--surface", &original.to_string(), &shell],
    );
    wait(|| PathBuf::from(format!("{}.status", ambiguous.display())).exists());
    assert_ne!(
        fs::read_to_string(format!("{}.status", ambiguous.display())).unwrap(),
        "0"
    );
    assert!(
        fs::read_to_string(&ambiguous)
            .unwrap()
            .contains("multiple app surfaces")
    );
    let implicit_panes = owner_cli(
        &gui,
        &f,
        original,
        "ambiguous-panes",
        &["list-panes".into()],
    );
    let explicit_notify = owner_cli(
        &gui,
        &f,
        original,
        "explicit-notify",
        &[
            "notify".into(),
            "--workspace".into(),
            started["workspace"].as_str().unwrap().into(),
            "--title".into(),
            "Explicit owner".into(),
        ],
    );
    assert!(
        implicit_panes.0 != 0 && explicit_notify.0 == 0,
        "implicit panes={implicit_panes:?}, explicit notify={explicit_notify:?}"
    );
    gui.json(
        &f,
        &[
            "close-surface",
            "--surface",
            extra["surface"].as_str().unwrap(),
        ],
    );
    wait(|| {
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    gui.json(&f, &["close-surface", "--surface", &original.to_string()]);
    wait(|| {
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    });
    gui.json(&f, &["reopen-closed"]);
    wait(|| {
        gui.snapshot().pending_launches == 0
            && gui
                .snapshot()
                .model
                .workspaces
                .iter()
                .flat_map(|w| w.root.panes())
                .flat_map(|p| &p.surfaces)
                .any(|s| {
                    s.local_tmux
                        .as_ref()
                        .is_some_and(|i| i.id.to_string() == id)
                })
    });
    let reopened = gui.surface(id);
    assert_ne!(reopened, original);
    assert_eq!(fs::read_to_string(&pidfile).unwrap(), pid);
    assert_eq!(fs::read_to_string(&count).unwrap(), "x");
    gui.json(&f, &["new-workspace", "Moved host"]);
    let moved_pane = gui
        .snapshot()
        .model
        .workspaces
        .iter()
        .find(|w| w.name == "Moved host")
        .unwrap()
        .focused_pane
        .0
        .to_string();
    gui.json(
        &f,
        &["move-surface", &reopened.to_string(), &moved_pane, "1"],
    );
    let moved = gui.json(&f, &["tmux", "attach", "--id", id, "--no-focus"]);
    assert_eq!(
        moved["reused"], true,
        "exact surface UUID must follow its workspace move"
    );
    assert_eq!(
        moved["surface"],
        gui.snapshot()
            .model
            .object_uuid(zmux_workspace_core::targets::TargetKind::Surface, reopened)
            .to_string()
    );
    let scoped_panes = owner_cli(&gui, &f, reopened, "moved-panes", &["list-panes".into()]);
    assert_eq!(scoped_panes.0, 0, "{}", scoped_panes.1);
    let scoped_panes: Value = serde_json::from_str(&scoped_panes.1).unwrap();
    assert_eq!(scoped_panes["panes"].as_array().unwrap().len(), 1);
    assert_eq!(
        scoped_panes["panes"][0]["pane_id"],
        moved_pane.parse::<u64>().unwrap()
    );
    let own_notice = owner_cli(
        &gui,
        &f,
        reopened,
        "moved-notify",
        &["notify".into(), "--title".into(), "Moved owner".into()],
    );
    assert_eq!(own_notice.0, 0, "{}", own_notice.1);
    let cleared = owner_cli(
        &gui,
        &f,
        reopened,
        "moved-clear",
        &["clear-notifications".into()],
    );
    assert_eq!(cleared.0, 0, "{}", cleared.1);
    let remaining = gui.json(&f, &["list-notifications"]);
    assert_eq!(
        remaining["notifications"].as_array().unwrap().len(),
        1,
        "managed caller clear must preserve the other workspace notification: {remaining}"
    );
    assert_eq!(remaining["notifications"][0]["title"], "Explicit owner");
    // Invoke the actual CLI inside the still-running shell. Its original app IDs
    // are deliberately invalid; managed-pane identity must route the new surface.
    let report = f.root.join("caller-result");
    let cli = format!(
        "env ZMUX_WORKSPACE_ID=stale ZMUX_SURFACE_ID=stale {} --socket {} --password tmux-test-only set-status tmux-caller rebound > {} 2>&1; printf done > {}.done\r",
        shell_quote(env!("CARGO_BIN_EXE_zmux")),
        shell_quote(gui.socket.to_str().unwrap()),
        shell_quote(report.to_str().unwrap()),
        shell_quote(report.to_str().unwrap())
    );
    gui.json(&f, &["send-text", "--surface", &reopened.to_string(), &cli]);
    wait(|| {
        report.with_extension("done").exists()
            || PathBuf::from(format!("{}.done", report.display())).exists()
    });
    let caller: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(caller["statuses"][0]["value"], "rebound");
    let save = gui.json(&f, &["save-session"]);
    let save_id = save["operation_id"].as_u64().unwrap().to_string();
    wait(|| gui.json(&f, &["session-status", "--id", &save_id])["state"] == "completed");
    let bytes = fs::read_to_string(f.root.join("state/session.json")).unwrap();
    assert!(bytes.contains("local_tmux_terminal"));
    assert!(!bytes.contains("startup-count"));
    assert!(!bytes.contains("tmux-test-only"));
    assert!(!bytes.contains("--command"));
    let epoch = started["epoch"].as_str().unwrap().to_owned();
    let operation_id = started["operation_id"].as_u64().unwrap().to_string();
    drop(gui);
    wait(|| {
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    });
    assert_eq!(
        f.json(&["local-tmux", "status", "--id", id])["session"]["state"],
        "live"
    );
    let restored = Gui::start(&f);
    assert_eq!(restored.surface(id), reopened);
    assert_eq!(
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fs::read_to_string(&pidfile).unwrap(), pid);
    assert_eq!(fs::read_to_string(&count).unwrap(), "x");
    assert!(
        !restored
            .output(
                &f,
                &[
                    "local-tmux",
                    "operation-status",
                    "--id",
                    &operation_id,
                    "--epoch",
                    &epoch
                ]
            )
            .status
            .success()
    );
    f.json(&["local-tmux", "close", "--id", id]);
}

#[test]
fn app_policy_and_terminal_capacity_reject_start_before_creating_an_owner() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy, SocketPassword};
    let f = Fixture::new();
    let gui = Gui::start(&f);
    gui.control
        .set_policy(ControlPolicy::password(
            ControlAccess::ReadOnly,
            SocketPassword::new("tmux-test-only".into()).unwrap(),
        ))
        .unwrap();
    let denied = gui.output(&f, &["local-tmux", "start", "forbidden"]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    assert!(!f.socket().exists());
    gui.control
        .set_policy(ControlPolicy::password(
            ControlAccess::Full,
            SocketPassword::new("tmux-test-only".into()).unwrap(),
        ))
        .unwrap();
    for _ in 1..zmux_desktop_runtime::MAX_TERMINALS {
        gui.json(&f, &["new-surface"]);
    }
    wait(|| gui.snapshot().pending_launches == 0);
    let full = gui.output(&f, &["local-tmux", "start", "overflow"]);
    assert!(!full.status.success());
    assert!(String::from_utf8_lossy(&full.stderr).contains("capacity"));
    assert!(!f.socket().exists());
    assert!(
        !f.root
            .join(".config/zmux/local-tmux/registry.json")
            .exists()
    );
}

#[test]
fn pending_app_start_reports_surviving_owner_when_its_exact_destination_closes() {
    use std::process::Stdio;
    let f = Fixture::new();
    let gui = Gui::start(&f);
    f.json(&["local-tmux", "list"]);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.root.join(".config/zmux/local-tmux/registry.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let cli = gui
        .command(
            &f,
            &[
                "local-tmux",
                "start",
                "survivor",
                "--command",
                "exec sleep 60",
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait(|| {
        gui.snapshot()
            .local_tmux_operations
            .iter()
            .any(|r| r.state == "pending")
    });
    let workspace = gui.snapshot().model.workspaces[0].id.0.to_string();
    gui.json(&f, &["close-workspace", "--workspace", &workspace]);
    drop(lock);
    let result = cli.wait_with_output().unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("closed while tmux operation was pending"),
        "{error}"
    );
    assert!(error.contains("created"), "{error}");
    let list = f.json(&["local-tmux", "list"]);
    assert_eq!(list["sessions"][0]["name"], "survivor");
    assert_eq!(list["sessions"][0]["state"], "live");
    assert!(
        list["sessions"][0]["clients"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(gui.snapshot().model.workspaces.is_empty());
}

#[test]
fn app_restore_rejects_replacement_server_without_replaying_commands_or_retargeting() {
    let f = Fixture::new();
    let gui = Gui::start(&f);
    let marker = f.root.join("must-run-once");
    let command = format!(
        "printf x >> {}; exec sleep 60",
        shell_quote(marker.to_str().unwrap())
    );
    gui.json(&f, &["local-tmux", "start", "old", "--command", &command]);
    let save = gui.json(&f, &["save-session"]);
    let save_id = save["operation_id"].as_u64().unwrap().to_string();
    wait(|| gui.json(&f, &["session-status", "--id", &save_id])["state"] == "completed");
    drop(gui);
    let saved = fs::read(f.root.join("state/session.json")).unwrap();
    let tmux = |args: &[&str]| {
        Command::new(&f.binary)
            .args(["-f", "/dev/null", "-S"])
            .arg(f.socket())
            .args(args)
            .env_remove("TMUX")
            .output()
            .unwrap()
    };
    assert!(tmux(&["kill-server"]).status.success());
    wait(|| std::os::unix::net::UnixStream::connect(f.socket()).is_err());
    assert!(
        tmux(&["new-session", "-d", "-s", "replacement", "exec sleep 60"])
            .status
            .success()
    );
    fs::set_permissions(f.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let restarted = Gui::start(&f);
    let snapshot = restarted.snapshot();
    assert!(
        snapshot
            .model
            .workspaces
            .iter()
            .flat_map(|w| w.root.panes())
            .flat_map(|p| &p.surfaces)
            .all(|s| s.local_tmux.is_none())
    );
    assert!(
        snapshot
            .persistence
            .error
            .as_ref()
            .is_some_and(|e| e.contains("stale")),
        "{:?}",
        snapshot.persistence.error
    );
    assert_eq!(fs::read_to_string(&marker).unwrap(), "x");
    assert_eq!(fs::read(f.root.join("state/session.json")).unwrap(), saved);
    assert!(
        tmux(&["has-session", "-t", "=replacement"])
            .status
            .success()
    );
}

#[test]
fn four_blocked_app_operations_bound_workers_without_blocking_workspace_commands() {
    use std::process::Stdio;
    let f = Fixture::new();
    let gui = Gui::start(&f);
    f.json(&["local-tmux", "list"]);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.root.join(".config/zmux/local-tmux/registry.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let clients: Vec<_> = (0..4)
        .map(|i| {
            gui.command(
                &f,
                &[
                    "local-tmux",
                    "start",
                    &format!("bounded{i}"),
                    "--command",
                    "exec sleep 60",
                    "--no-focus",
                ],
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
        })
        .collect();
    wait(|| {
        gui.snapshot()
            .local_tmux_operations
            .iter()
            .filter(|r| r.state == "pending")
            .count()
            == 4
    });
    let rejected = gui.output(&f, &["local-tmux", "start", "excess"]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("four local tmux operations"));
    gui.json(&f, &["new-workspace", "Still responsive"]);
    drop(lock);
    for client in clients {
        let output = client.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["state"], "completed");
    }
    let list = f.json(&["local-tmux", "list"]);
    assert_eq!(list["sessions"].as_array().unwrap().len(), 4);
    assert!(
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["clients"].as_array().unwrap().len() == 1)
    );
}

#[test]
fn post_spawn_registry_write_failure_retires_client_and_preserves_live_owner() {
    use zmux_remote_runtime::local_multiplexer::MAX_REGISTRY_BYTES;
    let f = Fixture::new();
    let gui = Gui::start(&f);
    let started = gui.json(
        &f,
        &[
            "local-tmux",
            "start",
            "surviving",
            "--command",
            "exec sleep 60",
        ],
    );
    let id = started["identity"]["id"].as_str().unwrap();
    let prior =
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"][0]["process_id"]
            .clone();
    let path = f.root.join(".config/zmux/local-tmux/registry.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    registry["sessions"][0]
        .as_object_mut()
        .unwrap()
        .remove("gui_binding");
    let template = registry["sessions"][0].clone();
    // A valid bounded registry with stale records can leave too little room for
    // a GUI hint. Reads/guarded attach succeed; the real post-attach write fails.
    for i in 1..64 {
        let mut record = template.clone();
        record["id"] = Value::String(format!("00000000-0000-4000-8000-{i:012x}"));
        record["name"] = Value::String(format!("padding-{i}"));
        record["tmux_session_id"] = Value::String(format!("${}", 10000 + i));
        record["cwd"] = Value::String("/".into());
        registry["sessions"].as_array_mut().unwrap().push(record);
        let available = MAX_REGISTRY_BYTES - 16 - serde_json::to_vec(&registry).unwrap().len();
        let padding = if available <= 4095 { available } else { 3500 };
        registry["sessions"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["cwd"] = Value::String(format!("/{}", "x".repeat(padding)));
        if available <= 4095 {
            break;
        }
    }
    let bytes = serde_json::to_vec(&registry).unwrap();
    assert_eq!(bytes.len(), MAX_REGISTRY_BYTES - 16);
    fs::write(&path, &bytes).unwrap();
    let failed = gui.output(&f, &["local-tmux", "attach", "--id", id, "--new-client"]);
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(error.contains("registry exceeds its byte bound"), "{error}");
    assert!(
        error.contains("durable owner was not terminated"),
        "{error}"
    );
    wait(|| {
        f.json(&["local-tmux", "status", "--id", id])["session"]["clients"]
            .as_array()
            .unwrap()
            .len()
            == 1
    });
    let remaining = f.json(&["local-tmux", "status", "--id", id]);
    assert_eq!(remaining["session"]["state"], "live");
    assert_eq!(remaining["session"]["clients"][0]["process_id"], prior);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(gui.snapshot().control_frames.iter().any(|frame| {
        frame
            .failure
            .as_ref()
            .is_some_and(|e| e.contains("registry exceeds its byte bound"))
    }));
    assert!(gui.snapshot().cleanup_error.is_none());
}
