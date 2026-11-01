//! Owned plugin publication and nonblocking hook ingress through the shipped CLI.
//! Synthetic provider --version only; no Claude invocation or user configuration.
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, Request, Shell,
    agent_lifecycle::{
        Action as LifecycleAction, Event as LifecycleEvent, Provider, Request as LifecycleRequest,
        Target as LifecycleTarget, TurnStopState,
    },
    control::{CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, ControlServer},
};
use zmux_workspace_core::targets::TargetKind;
// A fork in another test thread can transiently inherit a writable executable
// copy before exec closes CLOEXEC descriptors. Keep executable publication and
// child lifetimes sequential within this test process rather than retrying exec.
static PROCESS_FIXTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Directory {
    path: PathBuf,
    _process: std::sync::MutexGuard<'static, ()>,
}

struct BundledCli {
    path: PathBuf,
    previous: Option<(Vec<u8>, u32)>,
}
impl BundledCli {
    fn new(contents_dir: &Path) -> Self {
        let source = PathBuf::from(env!("CARGO_BIN_EXE_zmux"));
        let path = contents_dir.join("Resources/bin/zmux");
        let previous = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                assert!(
                    metadata.is_file(),
                    "bundled CLI test path must be a regular file"
                );
                Some((fs::read(&path).unwrap(), metadata.permissions().mode()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("inspect bundled CLI test path: {error}"),
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(source, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self { path, previous }
    }
}
impl Drop for BundledCli {
    fn drop(&mut self) {
        if let Some((contents, mode)) = self.previous.take() {
            fs::write(&self.path, contents).unwrap();
            fs::set_permissions(&self.path, fs::Permissions::from_mode(mode)).unwrap();
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct RunningDesktop {
    desktop: Arc<Desktop>,
    server: Option<ControlServer>,
}

struct ProviderRelease(PathBuf);
impl Drop for ProviderRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"release");
    }
}

fn shell_integration_roots() -> std::collections::HashSet<PathBuf> {
    let prefix = format!("zmux-shell-{}-", std::process::id());
    fs::read_dir(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect()
}
impl RunningDesktop {
    fn shutdown(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown(Duration::from_secs(2)).unwrap();
        }
        self.desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}
impl Drop for RunningDesktop {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.shutdown(Duration::from_secs(2));
        }
        let _ = self.desktop.shutdown(Duration::from_secs(5));
    }
}
impl Directory {
    fn new() -> Self {
        let guard = PROCESS_FIXTURE.lock().unwrap();
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zmux-hooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            path: p,
            _process: guard,
        }
    }
    fn plugin(&self) -> PathBuf {
        self.path.join("owned-plugin")
    }
    fn provider(&self) -> PathBuf {
        let p = self.path.join("synthetic-provider");
        if p.exists() {
            return p;
        }
        fs::write(&p,"#!/bin/sh\nif [ \"$1\" = --version ]; then printf '2.1.301 (Claude Code)\\n'; else printf invoked > \"$FIXTURE_PROVIDER_CALLED\"; fi\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn command(&self, args: &[&str]) -> Command {
        let adapter = self.path.join("adapter");
        if !adapter.exists() {
            fs::copy(env!("CARGO_BIN_EXE_zmux"), &adapter).unwrap();
            fs::set_permissions(&adapter, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut c = Command::new(adapter);
        c.env("HOME", &self.path)
            .env("FIXTURE_PROVIDER_CALLED", self.path.join("provider-called"));
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("ZMUX_") {
                c.env_remove(name);
            }
        }
        c.args(args);
        c
    }
    fn install(&self) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                "install",
                "--directory",
                self.plugin().to_str().unwrap(),
                "--provider",
                self.provider().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
    fn call(&self, verb: &str) -> Value {
        decoded(
            self.command(&[
                "hooks",
                "claude",
                verb,
                "--directory",
                self.plugin().to_str().unwrap(),
            ])
            .output()
            .unwrap(),
        )
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn decoded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn lifecycle_action(socket: &PathBuf, control_request_id: u64, action: LifecycleAction) -> Value {
    let request = ControlRequest {
        version: CONTROL_VERSION,
        request_id: control_request_id,
        password: None,
        command: ControlCommand::AgentLifecycle {
            request: LifecycleRequest {
                request_id: Uuid::new_v4(),
                action,
            },
        },
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut response = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut response).unwrap();
    let response: ControlResponse = serde_json::from_slice(&response).unwrap();
    assert!(response.ok, "{:?}", response.error);
    response.result.unwrap()
}

fn run_terminal_command(command: CommandBuilder) -> (portable_pty::ExitStatus, String) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let status = child.wait().unwrap();
    drop(pair.master);
    let mut output = Vec::new();
    match reader.read_to_end(&mut output) {
        Ok(_) => (),
        Err(error) if error.raw_os_error() == Some(nix::errno::Errno::EIO as i32) => (),
        Err(error) => panic!("read automatic Claude PTY output: {error}"),
    }
    (status, String::from_utf8_lossy(&output).into_owned())
}

#[test]
fn owned_plugin_is_idempotent_and_uninstall_preserves_unrelated_provider_settings() {
    let dir = Directory::new();
    let unrelated = dir.path.join(".claude");
    fs::create_dir(&unrelated).unwrap();
    let sentinel = b"{\"unrelated\":true,\"hooks\":{\"Stop\":[{\"command\":\"keep me\"}]}}";
    fs::write(unrelated.join("settings.json"), sentinel).unwrap();
    let first = dir.install();
    assert_eq!(first["changed"], true);
    let second = dir.install();
    assert_eq!(second["changed"], false);
    assert_eq!(first["generation"], second["generation"]);
    assert_eq!(dir.call("status")["installed"], true);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], false);
    assert_eq!(dir.call("status")["installed"], false);
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
    assert_eq!(fs::read(unrelated.join("settings.json")).unwrap(), sentinel);
}
#[test]
fn changed_or_symlinked_plugin_files_are_retained_after_admissions_are_disabled() {
    for substitute in [false, true] {
        let dir = Directory::new();
        let installed = dir.install();
        let config = dir
            .plugin()
            .join(installed["generation"].as_str().unwrap())
            .join("hooks/hooks.json");
        let unrelated = dir.path.join("unrelated.json");
        fs::write(&unrelated, b"unrelated sentinel").unwrap();
        if substitute {
            fs::remove_file(&config).unwrap();
            symlink(&unrelated, &config).unwrap();
        } else {
            fs::write(&config, b"changed user file").unwrap();
        }
        let before = fs::read(&config).unwrap();
        let result = dir.call("uninstall");
        assert_eq!(result["disabled"], true);
        assert_eq!(result["cleanup_pending"], true);
        assert_eq!(fs::read(&config).unwrap(), before);
        assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated sentinel");
        let record: Value =
            serde_json::from_slice(&fs::read(dir.plugin().join("installation.json")).unwrap())
                .unwrap();
        assert_eq!(record["enabled"], false);
    }
}
#[test]
fn malformed_unrouted_and_oversized_hooks_are_nonblocking_noops() {
    let dir = Directory::new();
    for bytes in [b"not JSON".to_vec(), b"{}".to_vec(), vec![b'x'; 70_000]] {
        let mut child = dir
            .command(&["hooks", "claude", "event", "UserPromptSubmit"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let _ = input.write_all(&bytes);
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            serde_json::json!({})
        );
        assert!(output.stderr.is_empty());
    }
}
#[test]
fn headless_and_unsupported_launches_never_start_the_provider() {
    let dir = Directory::new();
    dir.install();
    for args in [
        vec![],
        vec!["--print", "prompt"],
        vec!["--settings", "{}"],
        vec!["resume"],
        vec!["--bare"],
    ] {
        let mut command = dir.command(&[
            "hooks",
            "claude",
            "run",
            "--directory",
            dir.plugin().to_str().unwrap(),
            "--surface",
            &uuid::Uuid::new_v4().to_string(),
            "--",
        ]);
        command.args(args);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(!dir.path.join("provider-called").exists());
    }
}

#[test]
fn automatic_compatibility_fallback_runs_once_before_lifecycle_setup() {
    let dir = Directory::new();
    let provider = dir.path.join("claude");
    fs::write(
        &provider,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then printf '99.0 (Claude Code)\\n'; exit 0; fi\nprintf '<%s>\\n' \"$@\" > \"$FIXTURE_AUTO_ARGS\"\nprintf '%s' \"${ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE-unset}\" > \"$FIXTURE_AUTO_ALERT_MODE\"\nprintf started > \"$FIXTURE_AUTO_CALLED\"\nprintf 'FALLBACK_PROVIDER_STARTED\\n'\nexit 23\n",
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let notice_plugin = dir.path.join("notice-plugin");
    fs::create_dir(&notice_plugin).unwrap();
    let report = dir.path.join("auto-arguments");
    let called = dir.path.join("auto-called");
    let alert_mode = dir.path.join("auto-alert-mode");
    let surface = uuid::Uuid::new_v4().to_string();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_zmux"));
    command.args([
        "hooks",
        "claude",
        "auto",
        "--root",
        dir.path.to_str().unwrap(),
        "--surface",
        &surface,
        "--notice-plugin",
        notice_plugin.to_str().unwrap(),
        "--",
        "--model",
        "sonnet",
        "literal $HOME ; prompt",
    ]);
    command.env("PATH", &dir.path);
    command.env("FIXTURE_AUTO_ARGS", &report);
    command.env("FIXTURE_AUTO_CALLED", &called);
    command.env("FIXTURE_AUTO_ALERT_MODE", &alert_mode);
    command.env("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE", "1");
    let (status, output) = run_terminal_command(command);
    assert_eq!(status.exit_code(), 23, "{output}");
    assert!(output.contains("automatic Claude lifecycle unavailable"));
    assert!(output.contains("FALLBACK_PROVIDER_STARTED"));
    assert_eq!(
        fs::read_to_string(report).unwrap(),
        format!(
            "<--plugin-dir>\n<{}>\n<--model>\n<sonnet>\n<literal $HOME ; prompt>\n",
            notice_plugin.display()
        )
    );
    assert_eq!(fs::read(called).unwrap(), b"started");
    assert_eq!(fs::read(alert_mode).unwrap(), b"unset");
    assert!(fs::read_dir(&dir.path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("claude-auto-")
    }));
}

#[test]
fn automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli() {
    let dir = Directory::new();
    let provider = dir.path.join("claude");
    let rustc = Command::new("rustc")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/claude-provider.rs"
        ))
        .arg("-o")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();

    let args_report = dir.path.join("auto-arguments");
    let provider_called = dir.path.join("auto-called");
    let provider_release = dir.path.join("auto-release");
    let hooks_done = dir.path.join("auto-hooks-done");
    let provider_env = dir.path.join("auto-provider-env");
    let mut paths = vec![
        dir.path.clone(),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).unwrap();
    let test_binary = std::env::current_exe().unwrap();
    let private_contents = dir.path.join("deps");
    fs::create_dir_all(&private_contents).unwrap();
    let private_test_binary = private_contents.join(test_binary.file_name().unwrap());
    fs::copy(&test_binary, &private_test_binary).unwrap();
    fs::set_permissions(&private_test_binary, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Command::new(&private_test_binary);
    child.args([
        "--ignored",
        "--exact",
        "automatic_install_to_runtime_lease_real_desktop_child",
        "--nocapture",
    ]);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("ZMUX_") {
            child.env_remove(name);
        }
    }
    child.env("HOME", &dir.path);
    child.env("PATH", path);
    child.env("FIXTURE_AUTO_ARGS", &args_report);
    child.env("FIXTURE_AUTO_CALLED", &provider_called);
    child.env("FIXTURE_AUTO_RELEASE", &provider_release);
    child.env("FIXTURE_AUTO_EMIT_HOOKS", "1");
    child.env("FIXTURE_AUTO_HOOKS_DONE", &hooks_done);
    child.env("FIXTURE_AUTO_ENV_REPORT", &provider_env);
    child.env_remove("XDG_RUNTIME_DIR");
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "child desktop lifecycle regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "invoked by the parent test with a private environment"]
fn automatic_install_to_runtime_lease_real_desktop_child() {
    let root = PathBuf::from(std::env::var_os("HOME").expect("private test HOME"));
    let contents_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let cli = BundledCli::new(&contents_dir);
    assert_eq!(
        cli.path,
        contents_dir.join("Resources/bin/zmux"),
        "the real Desktop must resolve the private bundled CLI used by this child"
    );
    let args_report = PathBuf::from(std::env::var_os("FIXTURE_AUTO_ARGS").unwrap());
    let provider_called = PathBuf::from(std::env::var_os("FIXTURE_AUTO_CALLED").unwrap());
    let provider_release = PathBuf::from(std::env::var_os("FIXTURE_AUTO_RELEASE").unwrap());
    let hooks_done = PathBuf::from(std::env::var_os("FIXTURE_AUTO_HOOKS_DONE").unwrap());
    let provider_env = PathBuf::from(std::env::var_os("FIXTURE_AUTO_ENV_REPORT").unwrap());
    let shell_rc = root.join("bashrc");
    fs::write(&shell_rc, "PS1=\nprintf 'AUTO_DESKTOP_SHELL_READY\\n'\n").unwrap();
    let existing_shell_roots = shell_integration_roots();
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                shell_rc.to_string_lossy().into_owned(),
                "-i".into(),
            ],
            cwd: Some(root.clone()),
        })
        .unwrap(),
    );
    let control_socket = root.join(".local/run/zmux/control.sock");
    let server = ControlServer::start(&control_socket, Arc::clone(&desktop)).unwrap();
    let mut running = RunningDesktop {
        desktop,
        server: Some(server),
    };
    let _release_on_drop = ProviderRelease(provider_release.clone());

    let startup_deadline = Instant::now() + Duration::from_secs(8);
    let (session_id, shell_root) = loop {
        let snapshot = running.desktop.snapshot().unwrap();
        let session_id = snapshot
            .model
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.root.panes())
            .flat_map(|pane| pane.surfaces.iter())
            .find_map(|surface| surface.session_id);
        if let Some(session_id) = session_id {
            let ready = snapshot.control_frames.iter().any(|frame| {
                frame.session_id == session_id
                    && frame.snapshot.as_ref().is_some_and(|terminal| {
                        terminal.state == zmux_session_runtime::State::Running
                            && terminal
                                .viewport
                                .contents
                                .contains("AUTO_DESKTOP_SHELL_READY")
                    })
            });
            let shell_root = shell_integration_roots()
                .difference(&existing_shell_roots)
                .find(|path| path.join("plugin/hooks/hooks.json").is_file())
                .cloned();
            if ready && let Some(shell_root) = shell_root {
                break (session_id, shell_root);
            }
        }
        assert!(
            Instant::now() < startup_deadline,
            "real Desktop terminal did not reach its interactive startup"
        );
        thread::sleep(Duration::from_millis(5));
    };

    running
        .desktop
        .submit(Request {
            version: 1,
            request_id: 41,
            payload: DesktopCommand::Input {
                session_id,
                text: "claude --model sonnet 'literal $HOME ; prompt'\n".into(),
            },
        })
        .unwrap();

    let setup_deadline = Instant::now() + Duration::from_secs(8);
    let auto_directory = loop {
        let candidate = fs::read_dir(&shell_root)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("claude-auto-")
            })
            .map(|entry| entry.path());
        if let Some(candidate) = candidate
            && candidate.join("installation.json").is_file()
        {
            break candidate;
        }
        if provider_called.exists() {
            let report = fs::read_to_string(&args_report).unwrap_or_default();
            panic!("Claude started before automatic plugin publication: {report}");
        }
        assert!(
            Instant::now() < setup_deadline,
            "real automatic launch did not publish its lifecycle plugin"
        );
        thread::sleep(Duration::from_millis(2));
    };

    // Race the shipped cleanup command against the real CLI's install-to-lease
    // interval. Its admission lock must keep cleanup out until the lease exists.
    let cleanup_before_lease = Command::new(&cli.path)
        .args([
            "hooks",
            "claude",
            "cleanup",
            "--root",
            shell_root.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let provider_deadline = Instant::now() + Duration::from_secs(8);
    while !provider_called.exists() {
        assert!(
            Instant::now() < provider_deadline,
            "real automatic launch did not spawn the synthetic provider"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let cleanup_before_lease = cleanup_before_lease.wait_with_output().unwrap();
    if cleanup_before_lease.status.success() {
        let first_cleanup: Value = serde_json::from_slice(&cleanup_before_lease.stdout).unwrap();
        assert_eq!(first_cleanup["removed"], 0);
    } else {
        assert!(
            String::from_utf8_lossy(&cleanup_before_lease.stderr)
                .contains("temporarily unavailable"),
            "unexpected cleanup failure: {}",
            String::from_utf8_lossy(&cleanup_before_lease.stderr)
        );
    }
    assert!(auto_directory.is_dir());

    let marker_deadline = Instant::now() + Duration::from_secs(3);
    while auto_directory.join("launch-unknown").exists() {
        assert!(
            Instant::now() < marker_deadline,
            "provider identity was not confirmed while its runtime lease was held"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let cleanup_during_provider = Command::new(&cli.path)
        .args([
            "hooks",
            "claude",
            "cleanup",
            "--root",
            shell_root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cleanup_during_provider.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup_during_provider.stderr)
    );
    let second_cleanup: Value = serde_json::from_slice(&cleanup_during_provider.stdout).unwrap();
    let auto_name = auto_directory.file_name().unwrap().to_string_lossy();
    assert_eq!(second_cleanup["removed"], 0);
    assert!(
        second_cleanup["retained"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| { name.as_str() == Some(auto_name.as_ref()) })
    );
    assert!(
        auto_directory.is_dir(),
        "runtime lease did not retain the generation"
    );
    let retained_installation: Value =
        serde_json::from_slice(&fs::read(auto_directory.join("installation.json")).unwrap())
            .unwrap();
    assert_eq!(
        retained_installation["enabled"], true,
        "active-provider cleanup must retain lifecycle admission as well as plugin files"
    );

    let arguments: Vec<_> = fs::read_to_string(&args_report)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        arguments
            .iter()
            .filter(|arg| arg.as_str() == "<--plugin-dir>")
            .count(),
        2
    );
    let lifecycle_path = arguments[1]
        .strip_prefix('<')
        .unwrap()
        .strip_suffix('>')
        .unwrap();
    assert!(
        PathBuf::from(lifecycle_path)
            .join("hooks/hooks.json")
            .is_file()
    );
    assert_eq!(
        arguments[3],
        format!("<{}>", shell_root.join("plugin").display())
    );
    assert_eq!(arguments[4], "<--session-id>");
    assert_eq!(arguments[6], "<--model>");
    assert_eq!(arguments[7], "<sonnet>");
    assert_eq!(arguments[8], "<literal $HOME ; prompt>");

    let hooks_deadline = Instant::now() + Duration::from_secs(8);
    while !hooks_done.exists() {
        assert!(
            Instant::now() < hooks_deadline,
            "real provider did not execute the configured lifecycle and OSC hooks"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let provider_env = fs::read_to_string(provider_env).unwrap();
    assert!(
        provider_env
            .lines()
            .any(|line| line == format!("root={}", shell_root.display())),
        "provider did not inherit its exact session root: {provider_env}"
    );
    assert!(
        provider_env
            .lines()
            .any(|line| line == "lifecycle_alerts=1"),
        "provider did not inherit the lifecycle alert marker: {provider_env}"
    );

    let no_prompt_deadline = Instant::now() + Duration::from_secs(5);
    let no_prompt_snapshot = loop {
        let snapshot = running.desktop.snapshot().unwrap();
        let attention_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude needs attention"
            })
            .count();
        let completion_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude finished its turn"
            })
            .count();
        let failure_notice = snapshot.notifications.iter().any(|notification| {
            notification.title == "Claude" && notification.body == "Claude stopped with an error"
        });
        assert!(
            snapshot.agent_alert_events.is_empty(),
            "the no-prompt interleaving must not create a lifecycle alert: {:?}",
            snapshot.agent_alert_events
        );
        if attention_notices >= 1 && completion_notices >= 1 && failure_notice {
            break snapshot;
        }
        assert!(
            Instant::now() < no_prompt_deadline,
            "generic notices were lost before the lifecycle owner could admit a no-prompt event: {:?}",
            snapshot
                .notifications
                .iter()
                .map(|notification| (&notification.title, &notification.body))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(5));
    };
    let unknown_agent = no_prompt_snapshot
        .agent_lifecycle
        .iter()
        .find(|agent| agent.provider == Provider::ClaudeLocal)
        .unwrap_or_else(|| {
            let agents: Vec<_> = no_prompt_snapshot
                .agent_lifecycle
                .iter()
                .map(|agent| {
                    (
                        agent.provider,
                        agent.activity,
                        agent.connection,
                        agent.prompt_id,
                    )
                })
                .collect();
            let terminal_output: Vec<_> = no_prompt_snapshot
                .frames
                .iter()
                .map(|frame| {
                    (
                        frame.session_id,
                        frame
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.viewport.contents.clone()),
                    )
                })
                .collect();
            panic!(
                "SessionStart should register the production local lifecycle owner; agents={agents:?}, last_error={:?}, terminal_output={terminal_output:?}",
                no_prompt_snapshot.last_error
            )
        });
    assert_eq!(unknown_agent.prompt_id, None);
    assert_eq!(
        unknown_agent.activity,
        zmux_desktop_runtime::agent_lifecycle::Activity::Unknown
    );

    let initial = running.desktop.snapshot().unwrap();
    let model = &initial.model;
    let workspace = model.workspaces.first().unwrap();
    let pane = workspace.root.panes()[0];
    let surface = pane.selected();
    let target = LifecycleTarget {
        identity_namespace: model.identity_namespace,
        surface_id: surface.id,
        surface_uuid: model.object_uuid(TargetKind::Surface, surface.id.0),
        session_id: surface.session_id.unwrap(),
    };
    let runtime_id = Uuid::new_v4();
    let generation = Uuid::new_v4();
    let registration = lifecycle_action(
        &control_socket,
        42,
        LifecycleAction::Register {
            target,
            provider: Provider::ClaudeLocal,
            runtime_id,
            generation,
            provider_session_id: Uuid::new_v4(),
        },
    );
    let capability = registration["capability"]
        .as_str()
        .expect("lifecycle registration capability")
        .parse::<Uuid>()
        .unwrap();
    let prompt_id = Uuid::new_v4();
    for (request_id, sequence, event) in [
        (43, 1, LifecycleEvent::PromptSubmitted { prompt_id }),
        (44, 2, LifecycleEvent::NeedsUser),
        (
            45,
            3,
            LifecycleEvent::TurnStopped {
                state: TurnStopState::Idle,
            },
        ),
    ] {
        lifecycle_action(
            &control_socket,
            request_id,
            LifecycleAction::Event {
                capability,
                runtime_id,
                generation,
                sequence,
                event,
            },
        );
    }

    let alerts_deadline = Instant::now() + Duration::from_secs(5);
    let notice_snapshot = loop {
        let snapshot = running.desktop.snapshot().unwrap();
        let needs_user = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.kind == zmux_desktop_runtime::agent_lifecycle::AlertKind::NeedsUser
            })
            .count();
        let completed = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.kind == zmux_desktop_runtime::agent_lifecycle::AlertKind::Completed
            })
            .count();
        let failure_notice = snapshot.notifications.iter().any(|notification| {
            notification.title == "Claude" && notification.body == "Claude stopped with an error"
        });
        let attention_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude needs attention"
            })
            .count();
        let completion_notices = snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude finished its turn"
            })
            .count();
        if needs_user == 1
            && completed == 1
            && failure_notice
            && attention_notices >= 1
            && completion_notices >= 1
        {
            break snapshot;
        }
        assert!(
            Instant::now() < alerts_deadline,
            "lifecycle events and preserved terminal notices were not all delivered: events={:?}, notifications={:?}, agents={:?}",
            snapshot
                .agent_alert_events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            snapshot
                .notifications
                .iter()
                .map(|notification| (
                    &notification.title,
                    &notification.body,
                    &notification.agent_alert_kind
                ))
                .collect::<Vec<_>>(),
            snapshot
                .agent_lifecycle
                .iter()
                .map(|agent| (agent.activity, agent.connection, agent.prompt_id))
                .collect::<Vec<_>>()
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("needs_user"))
            .count(),
        1,
        "needs-user lifecycle alert was duplicated"
    );
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| notification.agent_alert_kind.as_deref() == Some("completed"))
            .count(),
        1,
        "completion lifecycle alert was duplicated"
    );
    assert_eq!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude"
                    && notification.body == "Claude stopped with an error"
            })
            .count(),
        1,
        "StopFailure OSC notice should remain available"
    );
    assert!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude needs attention"
            })
            .count()
            >= 1,
        "main-session and subagent permission hooks should leave a visible notice (identical notices may coalesce)"
    );
    assert!(
        notice_snapshot
            .notifications
            .iter()
            .filter(|notification| {
                notification.title == "Claude" && notification.body == "Claude finished its turn"
            })
            .count()
            >= 1,
        "main-session and subagent completion hooks should leave a visible notice (identical notices may coalesce)"
    );

    fs::write(&provider_release, b"release").unwrap();
    let cleanup_deadline = Instant::now() + Duration::from_secs(8);
    while auto_directory.exists() {
        assert!(
            Instant::now() < cleanup_deadline,
            "automatic plugin cleanup did not remove the generation after provider exit"
        );
        thread::sleep(Duration::from_millis(5));
    }
    running.shutdown();
}

#[test]
fn codex_lifecycle_install_is_explicitly_rejected() {
    let dir = Directory::new();
    let output = dir
        .command(&["hooks", "codex", "install", "--directory", "/tmp/nope"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Codex provider lifecycle hooks are unsupported")
    );
    assert!(!dir.path.join("provider-called").exists());
}

#[test]
fn uninstall_retains_identical_bytes_at_an_unowned_replacement_inode() {
    use std::os::unix::fs::MetadataExt;
    let dir = Directory::new();
    let installed = dir.install();
    let config = dir
        .plugin()
        .join(installed["generation"].as_str().unwrap())
        .join("hooks/hooks.json");
    let inode = fs::metadata(&config).unwrap().ino();
    let replacement = config.with_extension("replacement");
    fs::write(&replacement, fs::read(&config).unwrap()).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(replacement, &config).unwrap();
    assert_ne!(fs::metadata(&config).unwrap().ino(), inode);
    let result = dir.call("uninstall");
    assert_eq!(result["disabled"], true);
    assert_eq!(result["cleanup_pending"], true);
    assert!(
        config.exists(),
        "same bytes do not prove ownership of a replacement file"
    );
}
#[test]
fn interrupted_uninstall_removes_only_remaining_verified_owned_artifacts() {
    let dir = Directory::new();
    let installed = dir.install();
    let generation = dir.plugin().join(installed["generation"].as_str().unwrap());
    // A prior cleanup may have unlinked one verified file before interruption.
    fs::remove_file(generation.join("hooks/hooks.json")).unwrap();
    let orphan = dir.plugin().join("unpublished-generation");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join("keep"), b"unknown staging evidence").unwrap();
    let result = dir.call("uninstall");
    assert_eq!(result["cleanup_pending"], false);
    assert!(!generation.exists());
    assert_eq!(
        fs::read(orphan.join("keep")).unwrap(),
        b"unknown staging evidence"
    );
    assert_eq!(dir.call("uninstall")["cleanup_pending"], false);
}
