//! Workspace metadata contracts through the shipped CLI and real control server.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
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
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);
struct App {
    desktop: Arc<Desktop>,
    server: ControlServer,
    socket: PathBuf,
}
impl App {
    fn new() -> Self {
        Self::with_cwd(None)
    }
    fn with_cwd(cwd: Option<PathBuf>) -> Self {
        // Parallel macOS tests can observe the same wall-clock tick.
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let socket = std::env::temp_dir()
            .join(format!(
                "zm{:x}{:x}-{sequence:x}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("c");
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/bin/sh".into(),
                args: vec!["-i".into()],
                cwd,
            })
            .unwrap(),
        );
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        let app = Self {
            desktop,
            server,
            socket,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.run(&["list-workspaces"])["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        app
    }
    fn output(&self, args: &[&str], caller: Option<(&str, &str)>) -> Output {
        self.command(args, caller).output().unwrap()
    }
    fn command(&self, args: &[&str], caller: Option<(&str, &str)>) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_zmux"));
        c.arg("--socket")
            .arg(&self.socket)
            .args(args)
            .env_remove("ZMUX_WORKSPACE_ID")
            .env_remove("ZMUX_SURFACE_ID")
            .env_remove("ZMUX_TAB_ID")
            .env_remove("ZMUX_SOCKET_PASSWORD")
            .env_remove("ZMUX_SOCKET_PASSWORD_FILE");
        if let Some((w, s)) = caller {
            c.env("ZMUX_WORKSPACE_ID", w).env("ZMUX_SURFACE_ID", s);
        }
        c
    }
    fn call(&self, args: &[&str], caller: Option<(&str, &str)>) -> Value {
        let o = self.output(args, caller);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn run(&self, args: &[&str]) -> Value {
        self.call(args, None)
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.server.shutdown(Duration::from_secs(2)).unwrap();
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
        fs::remove_dir(self.socket.parent().unwrap()).unwrap();
    }
}
fn id(v: &Value, key: &str) -> String {
    v[key].as_u64().unwrap().to_string()
}

struct GitFixture {
    root: PathBuf,
    repository: PathBuf,
    bin: PathBuf,
}
impl GitFixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("zmux-pr-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repo");
        let bin = root.join("bin");
        fs::create_dir_all(&repository).unwrap();
        fs::create_dir(&bin).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec![
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(arguments)
                    .current_dir(&repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let fixture = Self {
            root,
            repository,
            bin,
        };
        fixture.gh("case \"$1 $2\" in\n'repo view') printf '%s\\n' '{\"nameWithOwner\":\"acme/widget\",\"parent\":null}' ;;\n'pr view') printf '%s\\n' '{\"number\":17,\"url\":\"https://github.com/acme/widget/pull/17\"}' ;;\n*) exit 9 ;;\nesac\n");
        fs::set_permissions(fixture.bin.join("gh"), fs::Permissions::from_mode(0o700)).unwrap();
        fixture
    }
    fn gh(&self, body: &str) {
        fs::write(self.bin.join("gh"), format!("#!/bin/sh\n{body}")).unwrap();
    }
    fn command(&self, app: &App, args: &[&str], caller: Option<(&str, &str)>) -> Command {
        let mut command = app.command(args, caller);
        command.current_dir(&self.repository).env(
            "PATH",
            format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
        );
        command
    }
    fn run(&self, app: &App, args: &[&str], caller: Option<(&str, &str)>) -> Value {
        let output = self.command(app, args, caller).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for GitFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn pull_request_handoff_checks_repository_and_caller_without_changing_focus_or_other_metadata() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let git = GitFixture::new();
    let app = App::with_cwd(Some(git.repository.clone()));
    let first = app.run(&["new-workspace", "PR owner"]);
    let workspace = id(&first, "focused_workspace");
    let surface = id(&first, "focused_surface");
    app.run(&[
        "set-status",
        "build",
        "preserved",
        "--workspace",
        &workspace,
    ]);
    let foreground = app.run(&["new-workspace", "foreground"]);
    let attached = git.run(
        &app,
        &[
            "pr",
            "https://github.com/acme/widget/pull/17",
            "--workspace",
            &workspace,
        ],
        None,
    );
    assert_eq!(attached["pull_request"]["number"], 17);
    assert_eq!(
        attached["pull_request"]["url"],
        "https://github.com/acme/widget/pull/17"
    );
    assert_eq!(attached["statuses"][0]["value"], "preserved");
    let numbered = git.run(&app, &["pr", "17"], Some((&workspace, &surface)));
    assert_eq!(numbered["pull_request"], attached["pull_request"]);
    let mut isolated = git.command(&app, &["pr", "17", "--workspace", &workspace], None);
    isolated
        .env("GIT_DIR", "/not-the-workspace")
        .env("GIT_WORK_TREE", "/not-the-workspace")
        .env("GH_REPO", "foreign/repo")
        .env("GH_HOST", "github.evil.test");
    let output = isolated.output().unwrap();
    assert!(
        output.status.success(),
        "ambient tool override escaped isolation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for input in [
        "0",
        "17junk",
        "017",
        "18",
        "https://github.com/acme/widget/pull/18446744073709551616",
        "https://github.com/acme/widget/pull/17?x=y",
        "https://github.com/acme/widget/pull/17/",
        "https://github.com.evil.test/acme/widget/pull/17",
        "https://github.com/acme/other/pull/17",
    ] {
        assert!(
            !git.command(&app, &["pr", input, "--workspace", &workspace], None)
                .output()
                .unwrap()
                .status
                .success(),
            "{input}"
        );
    }
    assert!(
        !git.command(&app, &["pr", "17"], None)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        app.run(&["list-status", "--workspace", &workspace])["pull_request"],
        attached["pull_request"]
    );
    assert_eq!(
        app.run(&["list-workspaces"])["focused_workspace"],
        foreground["focused_workspace"]
    );
    let foreign = GitFixture::new();
    assert!(
        !foreign
            .command(&app, &["pr", "17", "--workspace", &workspace], None)
            .output()
            .unwrap()
            .status
            .success(),
        "foreign checkout was accepted for the workspace"
    );
    assert!(
        Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "https://github.com/acme/pull.git"
            ])
            .current_dir(&git.repository)
            .status()
            .unwrap()
            .success()
    );
    git.gh("case \"$1 $2\" in\n'repo view') printf '%s\\n' '{\"nameWithOwner\":\"acme/pull\",\"parent\":{\"name\":\"upstream\",\"owner\":{\"login\":\"acme\"}}}' ;;\n'pr view') printf '%s\\n' '{\"number\":17,\"url\":\"https://github.com/acme/pull/pull/17\"}' ;;\nesac\n");
    let named_pull = git.run(
        &app,
        &[
            "pr",
            "https://github.com/acme/pull/pull/17",
            "--workspace",
            &workspace,
        ],
        None,
    );
    assert_eq!(
        named_pull["pull_request"]["url"],
        "https://github.com/acme/pull/pull/17"
    );
    git.gh("case \"$1 $2\" in\n'repo view') printf '%s\\n' '{\"nameWithOwner\":\"acme/pull\",\"parent\":{\"name\":\"upstream\",\"owner\":{\"login\":\"acme\"}}}' ;;\n'pr view') printf '%s\\n' '{\"number\":17,\"url\":\"https://github.com/acme/upstream/pull/17\"}' ;;\nesac\n");
    let parent = git.run(&app, &["pr", "17", "--workspace", &workspace], None);
    assert_eq!(
        parent["pull_request"]["url"],
        "https://github.com/acme/upstream/pull/17"
    );
    app.server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let denied = git
        .command(&app, &["pr", "clear", "--workspace", &workspace], None)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("read-only"));
    app.server
        .set_policy(ControlPolicy::same_user(ControlAccess::Full))
        .unwrap();
    let cleared = git.run(&app, &["pr", "--workspace", &workspace, "clear"], None);
    assert!(cleared["pull_request"].is_null());
    assert_eq!(cleared["statuses"][0]["value"], "preserved");
}

#[test]
fn pull_request_lookup_deadlines_and_failure_keep_metadata_and_stop_pipe_holding_descendants() {
    let git = GitFixture::new();
    let app = App::with_cwd(Some(git.repository.clone()));
    let first = app.run(&["new-workspace", "PR lookup owner"]);
    let workspace = id(&first, "focused_workspace");
    let attached = git.run(&app, &["pr", "17", "--workspace", &workspace], None);
    let child = git.root.join("descendant");
    for body in [
        format!(
            "sleep 60 &\nprintf '%s' \"$!\" > '{}'\nexit 0\n",
            child.display()
        ),
        format!(
            "exec 1>&- 2>&-\nsleep 60 &\nprintf '%s' \"$!\" > '{}'\nwait\n",
            child.display()
        ),
    ] {
        git.gh(&body);
        let before = Instant::now();
        let output = git
            .command(
                &app,
                &["--timeout", "250", "pr", "17", "--workspace", &workspace],
                None,
            )
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(before.elapsed() < Duration::from_secs(3));
        assert!(String::from_utf8_lossy(&output.stderr).contains("deadline"));
        let pid: u32 = fs::read_to_string(&child).unwrap().parse().unwrap();
        let state = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .unwrap();
        let state = String::from_utf8(state.stdout).unwrap();
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "owned descendant still executes: {pid} {state}"
        );
        assert_eq!(
            app.run(&["list-status", "--workspace", &workspace])["pull_request"],
            attached["pull_request"]
        );
    }
    git.gh(&format!("case \"$1 $2\" in\n'repo view') sleep 60 </dev/null >/dev/null 2>&1 & printf '%s' \"$!\" > '{}'; printf '%s\\n' '{{\"nameWithOwner\":\"acme/widget\",\"parent\":null}}' ;;\n'pr view') printf '%s\\n' '{{\"number\":17,\"url\":\"https://github.com/acme/widget/pull/17\"}}' ;;\nesac\n", child.display()));
    git.run(&app, &["pr", "17", "--workspace", &workspace], None);
    let pid = fs::read_to_string(&child).unwrap();
    let state = Command::new("ps")
        .args(["-p", &pid, "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(state.stdout).unwrap();
    assert!(
        state.trim().is_empty() || state.trim().starts_with('Z'),
        "closed-stdio descendant survived successful lookup"
    );
    for body in [
        "printf 'PRIVATE_AUTH_FIXTURE' >&2; exit 7\n",
        "head -c 70000 /dev/zero\n",
    ] {
        git.gh(body);
        let output = git
            .command(&app, &["pr", "17", "--workspace", &workspace], None)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_AUTH_FIXTURE"));
        assert_eq!(
            app.run(&["list-status", "--workspace", &workspace])["pull_request"],
            attached["pull_request"]
        );
    }
    let ready = git.root.join("cancel-ready");
    git.gh(&format!(
        "sleep 60 &\nprintf '%s' \"$!\" > '{}'\ntouch '{}'\nwait\n",
        child.display(),
        ready.display()
    ));
    let mut cli = git
        .command(&app, &["pr", "17", "--workspace", &workspace], None)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(cli.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    while cli.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let output = cli.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled"));
    let pid = fs::read_to_string(&child).unwrap();
    let state = Command::new("ps")
        .args(["-p", &pid, "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(state.stdout).unwrap();
    assert!(state.trim().is_empty() || state.trim().starts_with('Z'));
    assert_eq!(
        app.run(&["list-status", "--workspace", &workspace])["pull_request"],
        attached["pull_request"]
    );
    // Only the second Git query probes the optional target directory. A
    // nonrepository may be skipped, but permission/stream/encoding failures
    // must not disappear behind a misleading worktree-mismatch error.
    use std::os::unix::fs::PermissionsExt;
    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let counter = git.root.join("git-counter");
    for (failure, expected) in [
        (
            "printf 'fatal: permission failure' >&2; exit 128",
            "Git/GitHub lookup failed",
        ),
        (
            "head -c 70000 /dev/zero; exit 0",
            "lookup stream exceeded64KiB",
        ),
        ("printf '\\377'; exit 0", "lookup returned invalid UTF-8"),
    ] {
        fs::write(&counter, "0").unwrap();
        fs::write(git.bin.join("git"), format!("#!/bin/sh\nn=$(cat '{}'); n=$((n+1)); printf '%s' \"$n\" > '{}'\nif [ \"$n\" = 2 ]; then {failure}; fi\nexec '{}' \"$@\"\n", counter.display(), counter.display(), real_git.display())).unwrap();
        fs::set_permissions(git.bin.join("git"), fs::Permissions::from_mode(0o700)).unwrap();
        let output = git
            .command(&app, &["pr", "17", "--workspace", &workspace], None)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            app.run(&["list-status", "--workspace", &workspace])["pull_request"],
            attached["pull_request"]
        );
    }
}

#[test]
fn slow_pull_request_lookup_pins_the_original_workspace_instead_of_reusing_its_index() {
    let git = GitFixture::new();
    let app = App::with_cwd(Some(git.repository.clone()));
    let initial = app.run(&["list-workspaces"]);
    let workspace = id(&initial, "focused_workspace");
    let ready = git.root.join("ready");
    let release = git.root.join("release");
    git.gh(&format!("case \"$1 $2\" in\n'repo view') touch '{}'; while [ ! -f '{}' ]; do sleep .01; done; printf '%s\\n' '{{\"nameWithOwner\":\"acme/widget\",\"parent\":null}}' ;;\n'pr view') printf '%s\\n' '{{\"number\":17,\"url\":\"https://github.com/acme/widget/pull/17\"}}' ;;\nesac\n", ready.display(), release.display()));
    let mut child = git
        .command(&app, &["pr", "17", "--workspace", "index:1"], None)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "gh lookup never started");
        thread::sleep(Duration::from_millis(5));
    }
    app.run(&["close-workspace", "--workspace", &workspace]);
    let replacement = app.run(&["new-workspace", "replacement"]);
    fs::write(&release, "release").unwrap();
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "slow lookup did not finish");
        thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let state = app.run(&["list-status"]);
    assert!(state["pull_request"].is_null());
    assert_eq!(state["workspace_id"], replacement["focused_workspace"]);
}
#[test]
fn current_inspects_hidden_tabs_with_exact_owners_without_inventing_active_work() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let a = App::new();
    let initial = a.run(&["list-workspaces"]);
    let w = id(&initial, "focused_workspace");
    let pane = initial["workspaces"][0]["root"]["pane"]["id"]
        .as_u64()
        .unwrap()
        .to_string();
    let hidden = initial["workspaces"][0]["root"]["pane"]["selected_surface"]
        .as_u64()
        .unwrap();
    a.run(&["new-surface", &pane]);
    a.run(&["set-status", "build", "compiling", "--workspace", &w]);
    a.run(&["set-progress", "0.5", "--workspace", &w]);
    a.run(&["log", "private log text", "--workspace", &w]);
    a.run(&[
        "notify",
        "--workspace",
        &w,
        "--desktop",
        "false",
        "--title",
        "Observation",
    ]);
    a.run(&[
        "notify",
        "--workspace",
        &w,
        "--surface",
        &hidden.to_string(),
        "--desktop",
        "false",
        "--title",
        "Exact hidden owner",
    ]);
    let foreground = a.run(&["new-workspace", "foreground"]);
    let session_id = initial["workspaces"][0]["root"]["pane"]["surfaces"][0]["session_id"]
        .as_u64()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = desktop_snapshot::read_until(&a.desktop, deadline);
        if snapshot.control_frames.iter().any(|frame| {
            frame.session_id.0 == session_id
                && frame
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.state == zmux_session_runtime::State::Running)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "hidden session never published running"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let before = a.run(&["list-workspaces"]);
    a.server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    let report = a.call(&["current", "--json"], Some((&w, &hidden.to_string())));
    assert_eq!(report["total_surfaces"], 3);
    assert_eq!(report["returned_surfaces"], 3);
    assert_eq!(report["truncated"], false);
    assert_eq!(report["identity_namespace"], before["identity_namespace"]);
    assert_eq!(report["model_revision"], before["model_revision"]);
    assert_eq!(report["freshness"]["kind"], "cached_publication");
    assert!(report["freshness"]["observed_at"].is_null());
    let item = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["surface"]["id"] == hidden)
        .unwrap();
    assert_eq!(item["workspace"]["id"].as_u64().unwrap().to_string(), w);
    assert_eq!(item["pane"]["id"].as_u64().unwrap().to_string(), pane);
    assert_eq!(item["selected"], false);
    assert_eq!(item["logical_focus"], false);
    assert_eq!(item["activity"]["state"], "unknown");
    assert_eq!(item["session"]["evidence"], "desktop.control_frames");
    assert_eq!(item["session"]["scope"], "local_session_child");
    assert_eq!(item["session"]["state"], "running");
    let identity = before["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["kind"] == "surface" && target["id"] == hidden)
        .unwrap();
    assert_eq!(item["surface"]["uuid"], identity["uuid"]);
    assert_eq!(item["surface"]["reference"], identity["reference"]);
    let metadata = &report["workspaces"][0]["metadata"];
    assert_eq!(metadata["status_entries"], 1);
    assert_eq!(metadata["progress_value"], 0.5);
    assert_eq!(metadata["retained_logs"], 1);
    assert_eq!(report["workspaces"][0]["unread_notifications"], 2);
    assert_eq!(item["unread_notifications"], 1);
    assert_eq!(report["workspaces"][1]["unread_notifications"], 0);
    assert!(!report.to_string().contains("private log text"));
    let limited = a.run(&["current", "--limit", "1"]);
    assert_eq!(limited["total_surfaces"], 3);
    assert_eq!(limited["returned_surfaces"], 1);
    assert_eq!(limited["truncated"], true);
    assert_eq!(
        limited["items"][0]["surface"],
        report["items"][0]["surface"]
    );
    let after = a.run(&["list-workspaces"]);
    assert_eq!(after["model_revision"], before["model_revision"]);
    assert_eq!(after["focused_workspace"], foreground["focused_workspace"]);
    assert_eq!(after["workspaces"], before["workspaces"]);
}
#[test]
fn current_keeps_browser_activity_unknown_and_validates_limits_at_the_wire_owner() {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };
    let a = App::new();
    a.run(&["browser", "open", "https://example.test", "--no-focus"]);
    let report = a.run(&["current", "--limit", "200"]);
    let browser = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "browser")
        .unwrap();
    assert!(browser["session"].is_null());
    assert!(browser["session_id"].is_null());
    assert_eq!(browser["activity"]["state"], "unknown");
    for args in [
        vec!["current", "--limit", "0"],
        vec!["current", "--limit", "201"],
        vec!["current", "--limit", "-1"],
        vec!["current", "--limit", "bad"],
        vec!["current", "--limit"],
        vec!["current", "--limit", "1", "--limit", "2"],
        vec!["current", "--json", "--json"],
        vec!["current", "--workspace", "1"],
    ] {
        let output = a.output(&args, None);
        assert!(!output.status.success(), "{args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("unknown command"));
    }
    for (command, expected_limit) in [
        (serde_json::json!({"method":"current"}), Some(100)),
        (
            serde_json::json!({"method":"current","limit":200}),
            Some(200),
        ),
        (serde_json::json!({"method":"current","limit":0}), None),
        (serde_json::json!({"method":"current","limit":201}), None),
    ] {
        let mut stream = UnixStream::connect(&a.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let request = serde_json::to_vec(&serde_json::json!({
            "version":2,"request_id":99,"command":command,
        }))
        .unwrap();
        stream
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&request).unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let length = u32::from_be_bytes(length) as usize;
        assert!(length <= zmux_desktop_runtime::control::MAX_CONTROL_RESPONSE_BYTES);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).unwrap();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(limit) = expected_limit {
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(response["result"]["limit"], limit);
        } else {
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"]["code"], "invalid_current_limit");
        }
    }
}
#[test]
fn metadata_channels_target_the_caller_workspace_and_preserve_foreground_focus() {
    let a = App::new();
    let first = a.run(&["new-workspace", "metadata owner"]);
    let w = id(&first, "focused_workspace");
    let s = id(&first, "focused_surface");
    let second = a.run(&["new-workspace", "foreground"]);
    let foreground = id(&second, "focused_workspace");
    a.call(
        &[
            "set-status",
            "build",
            "compiling",
            "--icon",
            "hammer",
            "--color",
            "#ff9500",
            "--priority",
            "80",
        ],
        Some((&w, &s)),
    );
    let initial = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(initial["statuses"][0]["icon"], "hammer");
    assert_eq!(initial["statuses"][0]["color"], "#ff9500");
    a.run(&["set-status", "deploy", "waiting", "--workspace", &w]);
    a.run(&[
        "set-status",
        "build",
        "done",
        "--workspace",
        &w,
        "--priority",
        "80",
    ]);
    let status = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(status["workspace_id"].as_u64().unwrap().to_string(), w);
    assert_eq!(status["statuses"][0]["key"], "build");
    assert_eq!(status["statuses"][0]["value"], "done");
    assert_eq!(status["statuses"].as_array().unwrap().len(), 2);
    assert!(
        a.run(&["list-status"])["statuses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    a.run(&[
        "set-progress",
        "0.5",
        "--workspace",
        &w,
        "--label",
        "Building",
    ]);
    assert_eq!(
        a.run(&["list-status", "--workspace", &w])["progress"]["value"],
        0.5
    );
    for message in ["first", "second", "third"] {
        a.run(&[
            "log",
            message,
            "--workspace",
            &w,
            "--level",
            "success",
            "--source",
            "build",
        ]);
    }
    let log = a.run(&["list-log", "--workspace", &w, "--limit", "2"]);
    assert_eq!(
        log["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["second", "third"]
    );
    assert_eq!(log["logs"][0]["level"], "success");
    assert_eq!(
        id(&a.run(&["list-workspaces"]), "focused_workspace"),
        foreground
    );
    a.run(&["new-surface", &id(&first, "focused_pane")]);
    a.run(&["move-surface", &s, &id(&second, "focused_pane"), "1"]);
    assert_eq!(
        a.run(&["list-status", "--workspace", &w])["statuses"][0]["key"],
        "build"
    );
    assert!(
        a.run(&["list-status", "--workspace", &foreground])["statuses"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    a.run(&["clear-status", "build", "--workspace", &w]);
    a.run(&["clear-progress", "--workspace", &w]);
    a.run(&["clear-log", "--workspace", &w]);
    let state = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(state["statuses"].as_array().unwrap().len(), 1);
    assert!(state["progress"].is_null());
    assert!(state["logs"].as_array().unwrap().is_empty());
}
#[test]
fn metadata_rejects_bad_values_and_stale_or_wrong_owner_context_without_mutation() {
    let a = App::new();
    let first = a.run(&["new-workspace", "owner"]);
    let w = id(&first, "focused_workspace");
    let other = a.run(&["new-workspace", "other"]);
    let s = id(&other, "focused_surface");
    a.run(&["set-status", "safe", "unchanged", "--workspace", &w]);
    for args in [
        vec!["set-progress", "NaN"],
        vec!["set-progress", "1.01"],
        vec!["set-progress", "-0.1"],
        vec!["set-status", "x", "y", "--color", "red"],
        vec!["set-status", "x", "y", "--priority", "bad"],
        vec!["log", "bad", "--level", "fatal"],
        vec!["list-log", "--limit", "0"],
        vec!["list-status", "--workspace", &w, "--workspace", &w],
    ] {
        assert!(!a.output(&args, None).status.success(), "{args:?}");
    }
    assert!(
        !a.output(&["set-status", "bad", "owner"], Some((&w, &s)))
            .status
            .success()
    );
    assert!(
        !a.output(&["clear-status", "safe", "--workspace", "999999"], None)
            .status
            .success()
    );
    let rows = a.run(&["list-status", "--workspace", &w]);
    assert_eq!(rows["statuses"].as_array().unwrap().len(), 1);
    assert_eq!(rows["statuses"][0]["value"], "unchanged");
}
#[test]
fn readonly_socket_can_inspect_metadata_but_cannot_change_any_channel() {
    use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
    let a = App::new();
    a.run(&["set-status", "build", "preserved"]);
    a.run(&["log", "preserved"]);
    a.server
        .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
        .unwrap();
    assert_eq!(a.run(&["list-status"])["statuses"][0]["value"], "preserved");
    assert_eq!(a.run(&["list-log"])["logs"][0]["message"], "preserved");
    for args in [
        vec!["set-status", "x", "y"],
        vec!["clear-status", "build"],
        vec!["set-progress", "0.5"],
        vec!["clear-progress"],
        vec!["log", "denied"],
        vec!["clear-log"],
    ] {
        let o = a.output(&args, None);
        assert!(!o.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("read-only"));
    }
    assert_eq!(a.run(&["list-status"])["statuses"][0]["value"], "preserved");
    assert_eq!(a.run(&["list-log"])["logs"].as_array().unwrap().len(), 1);
}

#[test]
fn escaped_metadata_budget_rejection_keeps_accepted_records_readable() {
    use zmux_desktop_runtime::{
        Command as NativeCommand, Request,
        metadata::{
            LogLevel, MAX_LOG_ENTRIES, MAX_LOG_MESSAGE_BYTES, MAX_NAME_BYTES, MAX_RETAINED_BYTES,
            MAX_STATUS_ENTRIES, MAX_TEXT_BYTES, MetadataAction,
        },
        targets::{TargetOperation, TargetRequest},
    };
    let a = App::new();
    let mut next = 0;
    let mut send = |action| {
        next += 1;
        a.desktop
            .submit(Request {
                version: 1,
                request_id: next,
                payload: NativeCommand::Targeted {
                    request: TargetRequest {
                        operation: TargetOperation::Metadata { action },
                        ..Default::default()
                    },
                },
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let s = desktop_snapshot::read_until(&a.desktop, deadline);
            if s.last_request_id == next {
                break s;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    };
    for n in 0..MAX_STATUS_ENTRIES {
        let result = send(MetadataAction::SetStatus {
            key: format!("{n:03}{}", "\\".repeat(MAX_NAME_BYTES - 3)),
            value: "\\".repeat(MAX_TEXT_BYTES),
            icon: Some("\\".repeat(MAX_NAME_BYTES)),
            color: Some("#abcdef".into()),
            priority: i32::MIN,
        });
        assert!(result.last_error.is_none(), "{:?}", result.last_error);
    }
    let progress = send(MetadataAction::SetProgress {
        value: 0.5,
        label: Some("\\".repeat(MAX_TEXT_BYTES)),
    });
    assert!(progress.last_error.is_none());
    let mut accepted = 0;
    let mut rejected = false;
    for _ in 0..MAX_LOG_ENTRIES {
        let before = desktop_snapshot::read(&a.desktop);
        let result = send(MetadataAction::Log {
            message: "\\".repeat(MAX_LOG_MESSAGE_BYTES),
            level: LogLevel::Progress,
            source: Some("\\".repeat(MAX_NAME_BYTES)),
        });
        if let Some(error) = &result.last_error {
            assert!(error.contains("byte capacity"), "{error}");
            assert_eq!(
                serde_json::to_value(&result.workspace_metadata).unwrap(),
                serde_json::to_value(&before.workspace_metadata).unwrap()
            );
            rejected = true;
            break;
        }
        accepted += 1;
        assert!(
            serde_json::to_vec(&result.workspace_metadata)
                .unwrap()
                .len()
                <= MAX_RETAINED_BYTES
        );
    }
    assert!(rejected && accepted > 0 && accepted < MAX_LOG_ENTRIES);
    let listed = a.run(&["list-log"]);
    assert_eq!(listed["logs"].as_array().unwrap().len(), accepted);
    assert_eq!(
        listed["statuses"].as_array().unwrap().len(),
        MAX_STATUS_ENTRIES
    );
    assert_eq!(
        listed["logs"][0]["message"].as_str().unwrap().len(),
        MAX_LOG_MESSAGE_BYTES
    );
}
