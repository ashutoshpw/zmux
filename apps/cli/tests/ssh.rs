//! Real loopback OpenSSH journey; requires sshd and ssh-keygen, never an external host.
#[path = "../../../crates/desktop-runtime/tests/support/desktop_snapshot.rs"]
mod desktop_snapshot;
use desktop_runtime::agent_lifecycle::{Activity, AlertKind, Connection, Provider};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    net::{TcpListener, TcpStream},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime as desktop_runtime;
use zmux_desktop_runtime::{
    Command as DesktopCommand, Desktop, Request as DesktopRequest, Shell, control::ControlServer,
    transfers::TransferCommand,
};
use zmux_workspace_core::{SessionId, SurfaceId};

struct Fixture {
    root: PathBuf,
    daemon: Child,
}
impl Fixture {
    fn start() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // macOS TMPDIR paths can exceed sockaddr_un's path limit once a socket is added.
        let root = PathBuf::from("/tmp").join(format!("zs{}-{nonce:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["host", "client key"] {
            let generated = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(root.join(name))
                .output()
                .expect("SSH fixtures require ssh-keygen");
            assert!(generated.status.success(), "ssh-keygen failed");
        }
        fs::copy(root.join("client key.pub"), root.join("authorized_keys")).unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let user = Command::new("id").arg("-un").output().unwrap();
        assert!(user.status.success());
        let user = String::from_utf8(user.stdout).unwrap().trim().to_owned();
        fs::create_dir(root.join("uploads")).unwrap();
        let forced = root.join("shell");
        fs::write(&forced, format!(r#"#!/bin/sh
cd '{root}' || exit 79
export TMPDIR='{root}/uploads'
case "$SSH_ORIGINAL_COMMAND" in
  *zmux-upload*)
    if [ -f '{root}/broken-ack' ]; then
      trap '' PIPE
      /bin/sh -c "$SSH_ORIGINAL_COMMAND" | head -c 0
      exit 0
    fi
    if [ -f '{root}/hangup-ack' ]; then
      /bin/sh -c 'printf() {{ command printf "$@"; kill -HUP "$$"; }}; eval "$SSH_ORIGINAL_COMMAND"' > '{root}/ack'
      result=$?
      cat '{root}/ack'
      exit "$result"
    fi
    if [ -f '{root}/slow' ]; then sleep 4; fi
    if [ -f '{root}/delay-ack' ]; then
      /bin/sh -c "$SSH_ORIGINAL_COMMAND" > '{root}/ack'
      result=$?
      cat '{root}/ack'
      touch '{root}/ack-ready'
      sleep 4
      exit "$result"
    fi
  ;;
esac
if [ -n "$SSH_ORIGINAL_COMMAND" ]; then exec /bin/sh -c "$SSH_ORIGINAL_COMMAND"; else exec /bin/sh -i; fi
"#, root=root.display())).unwrap();
        fs::set_permissions(&forced, fs::Permissions::from_mode(0o700)).unwrap();
        let server_config = root.join("server");
        fs::write(&server_config, format!("Port {port}\nListenAddress 127.0.0.1\nHostKey {}/host\nAuthorizedKeysFile {}/authorized_keys\nPidFile {}/pid\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nStrictModes no\nLogLevel ERROR\nPrintMotd no\nPrintLastLog no\nForceCommand {}\n", root.display(), root.display(), root.display(), forced.display())).unwrap();
        fs::write(root.join("client"), format!("Host fixture\n HostName 127.0.0.1\n Port {port}\n User {user}\n UserKnownHostsFile {}/known\n StrictHostKeyChecking accept-new\n BatchMode yes\n IdentitiesOnly yes\n ConnectTimeout 1\n ConnectionAttempts 1\n", root.display())).unwrap();
        let daemon = Command::new("/usr/sbin/sshd")
            .args(["-D", "-e", "-f"])
            .arg(server_config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("SSH fixtures require openssh-server (/usr/sbin/sshd)");
        let mut fixture = Self { root, daemon };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            if let Some(status) = fixture.daemon.try_wait().unwrap() {
                let mut error = String::new();
                fixture
                    .daemon
                    .stderr
                    .as_mut()
                    .unwrap()
                    .read_to_string(&mut error)
                    .unwrap();
                panic!("fixture sshd failed {status}: {error}");
            }
            assert!(Instant::now() < deadline, "fixture sshd did not listen");
            thread::sleep(Duration::from_millis(10));
        }
        fixture
    }
    fn stop(&mut self) {
        if self.daemon.try_wait().unwrap().is_none() {
            self.daemon.kill().unwrap();
            self.daemon.wait().unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("fixture cleanup: {error}");
        }
    }
}
fn invoke(socket: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("--socket")
        .arg(socket)
        .args(args)
        .env("HOME", socket.parent().unwrap())
        .env_remove("ZMUX_SOCKET_PASSWORD")
        .env_remove("ZMUX_SOCKET_PASSWORD_FILE")
        .env_remove("ZMUX_WORKSPACE_ID")
        .env_remove("ZMUX_SURFACE_ID")
        .env_remove("ZMUX_TAB_ID")
        .output()
        .unwrap()
}
fn json(socket: &Path, args: &[&str]) -> Value {
    let output = invoke(socket, args);
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn screen(socket: &Path, surface: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = invoke(socket, &["read-screen", "--surface", surface]);
        if output.status.success() {
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            if predicate(&value) {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "remote terminal did not reach expected state: {value}"
            );
        } else {
            assert!(
                Instant::now() < deadline,
                "remote screen unavailable: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn upload(socket: &Path, id: u64, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let uploads = json(socket, &["list-uploads"]);
        let value = uploads["uploads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["id"] == id)
            .unwrap();
        if predicate(value) {
            return value.clone();
        }
        assert!(
            Instant::now() < deadline,
            "upload did not reach expected state: {value}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_remote_provider(
    desktop: &Desktop,
    activity: Activity,
) -> desktop_runtime::agent_lifecycle::Agent {
    wait_remote_provider_kind(desktop, Provider::CodexRemote, activity)
}

fn wait_remote_provider_kind(
    desktop: &Desktop,
    provider: Provider,
    activity: Activity,
) -> desktop_runtime::agent_lifecycle::Agent {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let snapshot = desktop_snapshot::read_until(desktop, deadline);
        if let Some(agent) = snapshot
            .agent_lifecycle
            .iter()
            .find(|agent| agent.provider == provider && agent.activity == activity)
        {
            return agent.clone();
        }
        assert!(
            Instant::now() < deadline,
            "managed SSH {provider:?} lifecycle did not reach {activity:?}; snapshot={:?}",
            snapshot.agent_lifecycle
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn require_python3_for_managed_provider_tests() {
    let version = Command::new("python3")
        .arg("--version")
        .output()
        .expect("managed SSH provider tests require Python 3 on the test host and SSH account");
    assert!(
        version.status.success(),
        "managed SSH provider tests require Python 3; python3 --version failed"
    );
}

fn require_node_for_managed_opencode_test() {
    let version = Command::new("node")
        .arg("--version")
        .output()
        .expect("managed SSH OpenCode fixture requires Node.js (node)");
    assert!(
        version.status.success(),
        "managed SSH OpenCode fixture requires Node.js; node --version failed"
    );
}

fn codex_completion_count(desktop: &Desktop) -> usize {
    desktop_snapshot::read(desktop)
        .agent_alert_events
        .iter()
        .filter(|event| {
            event.provider == Provider::CodexRemote && event.kind == AlertKind::Completed
        })
        .count()
}

#[test]
fn managed_ssh_codex_uses_root_thread_events_and_ignores_stop_hook_decisions() {
    require_python3_for_managed_provider_tests();
    let mut fixture = Fixture::start();
    let socket = fixture.root.join("control.sock");
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();

    let bin = fixture.root.join("provider-bin");
    fs::create_dir(&bin).unwrap();
    let codex = bin.join("codex");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/provider-synthetic-codex.py"),
        &codex,
    )
    .unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o700)).unwrap();
    let state = fixture.root.join("provider-state");
    let report = fixture.root.join("provider-config-report");
    let calls = fixture.root.join("provider-launch-count");
    let stop_probe = fixture.root.join("provider-stop-probe");
    let stop_result = fixture.root.join("provider-stop-result");
    let release = fixture.root.join("provider-release");
    let app_server_socket = fixture.root.join("codex-app-server.sock");
    let app_server_pid = fixture.root.join("codex-app-server.pid");
    fs::write(&state, "idle").unwrap();

    let config = fixture.root.join("client").to_string_lossy().into_owned();
    let key = fixture
        .root
        .join("client key")
        .to_string_lossy()
        .into_owned();
    let created = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config,
            "--identity",
            &key,
            "--name",
            "Provider fixture",
            "--no-focus",
        ],
    );
    let surface = created["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    screen(&socket, surface, |snapshot| snapshot["state"] == "running");
    let command = format!(
        "export PATH='{}':$PATH; export ZMUX_FAKE_CODEX_STATE='{}'; export ZMUX_FAKE_CODEX_REPORT='{}'; export ZMUX_FAKE_CODEX_CALLS='{}'; export ZMUX_FAKE_CODEX_STOP_PROBE='{}'; export ZMUX_FAKE_CODEX_STOP_RESULT='{}'; export ZMUX_FAKE_CODEX_RELEASE='{}'; export ZMUX_FAKE_CODEX_SOCKET='{}'; export ZMUX_FAKE_CODEX_DAEMON_PID='{}'; codex\n",
        bin.display(),
        state.display(),
        report.display(),
        calls.display(),
        stop_probe.display(),
        stop_result.display(),
        release.display(),
        app_server_socket.display(),
        app_server_pid.display(),
    );
    json(&socket, &["send-text", "--surface", surface, &command]);
    screen(&socket, surface, |snapshot| {
        snapshot["contents"]
            .as_str()
            .is_some_and(|contents| contents.contains("CODEX_FIXTURE_READY"))
    });

    let initial = wait_remote_provider(&desktop, Activity::Idle);
    assert_eq!(initial.connection, Connection::Connected);
    assert_eq!(initial.prompt_id, None);
    let config_report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(config_report["existing_session_start_preserved"], true);
    assert_eq!(config_report["zmux_session_start_added"], true);
    assert_eq!(config_report["stop_override_added"], false);
    assert_eq!(codex_completion_count(&desktop), 0);

    fs::write(&state, "start").unwrap();
    let working = wait_remote_provider(&desktop, Activity::Working);
    assert_eq!(working.connection, Connection::Connected);
    assert_eq!(working.provider_session_id, initial.provider_session_id);
    assert!(working.prompt_id.is_some());

    // A Stop-hook result carrying both block and approve fields is rejected by
    // the SessionStart-only ingress; it cannot change this exact live turn.
    fs::write(&stop_probe, "invoke Stop hook").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !stop_result.is_file() {
        assert!(
            Instant::now() < deadline,
            "synthetic Codex Stop hook was not exercised"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_ne!(fs::read_to_string(&stop_result).unwrap(), "0");
    thread::sleep(Duration::from_millis(100));
    let still_working = wait_remote_provider(&desktop, Activity::Working);
    assert_eq!(still_working.prompt_id, working.prompt_id);
    assert_eq!(codex_completion_count(&desktop), 0);

    fs::write(&state, "complete").unwrap();
    let completed = wait_remote_provider(&desktop, Activity::Idle);
    assert_eq!(completed.connection, Connection::Connected);
    assert_eq!(completed.provider_session_id, initial.provider_session_id);
    assert_eq!(codex_completion_count(&desktop), 1);

    fs::write(&release, "release").unwrap();
    screen(&socket, surface, |snapshot| {
        snapshot["contents"]
            .as_str()
            .is_some_and(|contents| contents.contains("CODEX_FIXTURE_DONE"))
    });
    let lost = wait_remote_provider(&desktop, Activity::Unknown);
    assert_eq!(lost.connection, Connection::Unknown);
    assert_eq!(codex_completion_count(&desktop), 1);
    for (index, args) in [
        vec!["--cd", "project-B"],
        vec!["--profile=selected-profile"],
        vec!["-C", "project-B"],
        vec!["-p", "selected-profile"],
        vec!["mcp", "list"],
    ]
    .iter()
    .enumerate()
    {
        let marker = format!("REMOTE_CODEX_CONTEXT_{index}_DONE");
        let command = format!(
            "codex {}; printf 'REMOTE_CODEX_CONTEXT_%s_DONE\\n' '{index}'\n",
            args.join(" ")
        );
        json(&socket, &["send-text", "--surface", surface, &command]);
        screen(&socket, surface, |snapshot| {
            snapshot["contents"]
                .as_str()
                .is_some_and(|s| s.contains(&marker))
        });
        let report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(report["args"], serde_json::json!(args));
        assert_eq!(report["zmux_session_start_added"], false);
        assert_eq!(
            fs::read_to_string(&calls).unwrap().lines().count(),
            index + 2,
            "each remote passthrough invocation must launch exactly once"
        );
    }

    json(&socket, &["close-surface", surface]);
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fixture.stop();
}

#[test]
fn managed_ssh_opencode_tracks_root_events_refreshes_long_turns_and_preserves_config() {
    require_python3_for_managed_provider_tests();
    require_node_for_managed_opencode_test();
    let mut fixture = Fixture::start();
    let socket = fixture.root.join("control.sock");
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();

    let bin = fixture.root.join("provider-bin");
    fs::create_dir(&bin).unwrap();
    let opencode = bin.join("opencode");
    fs::write(
        &opencode,
        "#!/bin/sh\nexec node \"$FIXTURE_OPENCODE_RUNNER\" \"$FIXTURE_OPENCODE_PLUGIN_HOST\" \"$FIXTURE_OPENCODE_ROOT_SESSION\" \"$FIXTURE_OPENCODE_REPORT\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&opencode, fs::Permissions::from_mode(0o700)).unwrap();

    let config = fixture.root.join("user-opencode-config");
    let plugins = config.join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&plugins, fs::Permissions::from_mode(0o700)).unwrap();
    let config_contents = r#"{"model":"fixture/model","permission":{"*":"ask"}}"#;
    let user_plugin_contents = "export default async () => ({});\n";
    fs::write(config.join("opencode.json"), config_contents).unwrap();
    fs::write(plugins.join("user-plugin.js"), user_plugin_contents).unwrap();
    let runner = fixture.root.join("provider-opencode-runner.mjs");
    let host = fixture.root.join("provider-opencode-plugin-host.mjs");
    fs::write(
        &runner,
        include_str!("../../../crates/session-runtime/tests/fixtures/provider-opencode-runner.mjs"),
    )
    .unwrap();
    fs::write(
        &host,
        include_str!(
            "../../../crates/session-runtime/tests/fixtures/provider-opencode-plugin-host.mjs"
        ),
    )
    .unwrap();

    let report = fixture.root.join("opencode-report");
    let calls = fixture.root.join("opencode-calls");
    let root_session = "session:remote/root-opaque";
    let config_path = fixture.root.join("client").to_string_lossy().into_owned();
    let key = fixture
        .root
        .join("client key")
        .to_string_lossy()
        .into_owned();
    let created = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config_path,
            "--identity",
            &key,
            "--name",
            "OpenCode fixture",
            "--no-focus",
        ],
    );
    let surface = created["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    screen(&socket, surface, |snapshot| snapshot["state"] == "running");

    let command = format!(
        "export PATH='{}':$PATH; export OPENCODE_CONFIG_DIR='{}'; export FIXTURE_OPENCODE_RUNNER='{}'; export FIXTURE_OPENCODE_PLUGIN_HOST='{}'; export FIXTURE_OPENCODE_ROOT_SESSION='{}'; export FIXTURE_OPENCODE_REPORT='{}'; export FIXTURE_OPENCODE_CALLS='{}'; export FIXTURE_OPENCODE_USER_CONFIG='{}'; export FIXTURE_OPENCODE_USER_CONFIG_CONTENT='{}'; export FIXTURE_OPENCODE_USER_PLUGIN_CONTENT='{}'; export FIXTURE_OPENCODE_STANDALONE_SUPPORT=1; opencode --session {}\n",
        bin.display(),
        config.display(),
        runner.display(),
        host.display(),
        root_session,
        report.display(),
        calls.display(),
        config.display(),
        config_contents,
        user_plugin_contents,
        root_session,
    );
    json(&socket, &["send-text", "--surface", surface, &command]);
    screen(&socket, surface, |snapshot| {
        snapshot["contents"]
            .as_str()
            .is_some_and(|contents| contents.contains(r#"{"ready":true}"#))
    });

    let send = |operation: &str| {
        json(
            &socket,
            &[
                "send-text",
                "--surface",
                surface,
                &format!("{{\"op\":\"{operation}\"}}\n"),
            ],
        );
    };
    send("created");

    let initial_deadline = Instant::now() + Duration::from_secs(12);
    let initial = loop {
        let snapshot = desktop_snapshot::read_until(&desktop, initial_deadline);
        if let Some(agent) = snapshot.agent_lifecycle.iter().find(|agent| {
            agent.provider == Provider::OpenCodeRemote && agent.activity == Activity::Idle
        }) {
            break agent.clone();
        }
        if Instant::now() >= initial_deadline {
            let terminal = json(&socket, &["read-screen", "--surface", surface]);
            panic!(
                "managed SSH OpenCode did not publish its root baseline; lifecycle={:?}; terminal={:?}",
                snapshot.agent_lifecycle, terminal["contents"]
            );
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(initial.connection, Connection::Connected);
    assert_eq!(initial.surface_uuid.to_string(), surface);
    assert_eq!(initial.prompt_id, None);
    let launch_report: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        launch_report["args"],
        serde_json::json!(["--standalone", "--session", root_session])
    );
    assert_eq!(launch_report["config_directory_is_invocation"], true);
    assert_eq!(launch_report["config_is_symlink"], true);
    assert_eq!(launch_report["config_preserved"], true);
    assert_eq!(launch_report["user_plugin_is_symlink"], true);
    assert_eq!(launch_report["user_plugin_preserved"], true);
    assert_eq!(launch_report["zmux_plugin_count"], 1);
    assert_eq!(launch_report["zmux_plugin_mode"], 0o600);
    assert_eq!(
        fs::read(config.join("opencode.json")).unwrap(),
        config_contents.as_bytes()
    );
    assert_eq!(
        fs::read(plugins.join("user-plugin.js")).unwrap(),
        user_plugin_contents.as_bytes()
    );

    let root_agent = wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Idle);
    assert_eq!(root_agent.provider_session_id, initial.provider_session_id);
    send("foreign");
    thread::sleep(Duration::from_millis(100));
    let after_foreign = desktop_snapshot::read(&desktop);
    let remote_agents = after_foreign
        .agent_lifecycle
        .iter()
        .filter(|agent| agent.provider == Provider::OpenCodeRemote)
        .collect::<Vec<_>>();
    assert_eq!(remote_agents.len(), 1);
    assert_eq!(remote_agents[0].activity, Activity::Idle);
    assert!(after_foreign.agent_alert_events.is_empty());

    send("start");
    let working = wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Working);
    assert_eq!(working.provider_session_id, initial.provider_session_id);
    assert!(working.prompt_id.is_some());
    let long_run_started = Instant::now();
    while long_run_started.elapsed() < Duration::from_secs(31) {
        let snapshot = desktop_snapshot::read(&desktop);
        let agent = snapshot
            .agent_lifecycle
            .iter()
            .find(|agent| agent.provider == Provider::OpenCodeRemote)
            .expect("OpenCode root agent remains attached during its long turn");
        assert_eq!(agent.activity, Activity::Working);
        assert_eq!(agent.connection, Connection::Connected);
        assert!(snapshot.agent_alert_events.is_empty());
        thread::sleep(Duration::from_millis(250));
    }
    let refreshed =
        wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Working);
    assert_eq!(refreshed.prompt_id, working.prompt_id);
    assert_eq!(refreshed.connection, Connection::Connected);

    send("wait");
    let waiting = wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Waiting);
    assert_eq!(waiting.prompt_id, working.prompt_id);
    assert_eq!(
        desktop_snapshot::read(&desktop)
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::OpenCodeRemote && event.kind == AlertKind::NeedsUser
            })
            .count(),
        1
    );
    send("resume");
    wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Working);
    send("idle");
    wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Idle);
    assert_eq!(
        desktop_snapshot::read(&desktop)
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::OpenCodeRemote && event.kind == AlertKind::Completed
            })
            .count(),
        0,
        "an idle source snapshot is not completion evidence"
    );
    send("background");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        desktop_snapshot::read(&desktop)
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::OpenCodeRemote && event.kind == AlertKind::Completed
            })
            .count(),
        0
    );
    send("complete");
    let completion_deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = desktop_snapshot::read(&desktop);
        let completions = snapshot
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::OpenCodeRemote && event.kind == AlertKind::Completed
            })
            .count();
        if completions == 1 {
            break;
        }
        assert!(
            Instant::now() < completion_deadline,
            "OpenCode completion was not accepted once: {:?}",
            snapshot.agent_alert_events
        );
        thread::sleep(Duration::from_millis(10));
    }
    let completed = wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Idle);
    assert_eq!(completed.prompt_id, working.prompt_id);
    send("next-start");
    let second_turn =
        wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Working);
    assert_ne!(second_turn.prompt_id, working.prompt_id);
    send("interrupt");
    wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Idle);
    assert_eq!(
        desktop_snapshot::read(&desktop)
            .agent_alert_events
            .iter()
            .filter(|event| {
                event.provider == Provider::OpenCodeRemote && event.kind == AlertKind::Completed
            })
            .count(),
        1,
        "an interrupted OpenCode turn is never completion"
    );

    send("shutdown");
    let lost = wait_remote_provider_kind(&desktop, Provider::OpenCodeRemote, Activity::Unknown);
    assert_eq!(lost.connection, Connection::Unknown);

    let fallback = format!(
        "export FIXTURE_OPENCODE_STANDALONE_SUPPORT=0; opencode --session {}\n",
        root_session
    );
    json(&socket, &["send-text", "--surface", surface, &fallback]);
    screen(&socket, surface, |snapshot| {
        snapshot["contents"]
            .as_str()
            .is_some_and(|contents| contents.contains("OPENCODE_FALLBACK_LAUNCHED"))
    });
    let calls = fs::read_to_string(&calls)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "metadata probes do not launch the provider");
    assert_eq!(
        calls[1]["args"],
        serde_json::json!(["--session", root_session])
    );
    assert_eq!(calls[1]["config_directory_is_invocation"], false);
    assert_eq!(calls[1]["config_preserved"], true);
    assert_eq!(calls[1]["user_plugin_is_symlink"], false);
    assert_eq!(calls[1]["user_plugin_preserved"], true);
    assert_eq!(calls[1]["zmux_plugin_count"], 0);

    json(&socket, &["close-surface", surface]);
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
    fixture.stop();
}

#[test]
fn ssh_cli_opens_an_actual_remote_session_and_reopen_never_replays_its_command() {
    let mut fixture = Fixture::start();
    let socket = fixture.root.join("control.sock");
    let desktop = Arc::new(
        Desktop::new(Shell {
            program: "/bin/sh".into(),
            args: vec!["-i".into()],
            cwd: None,
        })
        .unwrap(),
    );
    let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let initial = loop {
        let initial = json(&socket, &["list-workspaces"]);
        if !initial["focused_workspace"].is_null() {
            break initial;
        }
        assert!(Instant::now() < deadline, "initial workspace not ready");
        thread::sleep(Duration::from_millis(10));
    };
    let config = fixture.root.join("client").to_string_lossy().into_owned();
    let key = fixture
        .root
        .join("client key")
        .to_string_lossy()
        .into_owned();
    let counter = fixture.root.join("executions");
    let startup = format!(
        "[ -t 0 ] || exit 77; printf 'run\\n' >> '{}'; printf 'REMOTE_START\\n'; IFS= read -r line; printf 'REMOTE_INPUT:%s\\n' \"$line\"",
        counter.display()
    );
    let created = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config,
            "--identity",
            &key,
            "--name",
            "Remote fixture",
            "--no-focus",
            "--command",
            &startup,
        ],
    );
    assert_eq!(created["focused_workspace"], initial["focused_workspace"]);
    let surface = created["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    let started = screen(&socket, surface, |s| {
        s["contents"].as_str().unwrap().contains("REMOTE_START") || s["state"] == "exited"
    });
    assert!(
        started["contents"]
            .as_str()
            .unwrap()
            .contains("REMOTE_START"),
        "remote terminal did not receive a PTY: {started}"
    );
    json(
        &socket,
        &["send-text", "--surface", surface, "hello remote\n"],
    );
    let exited = screen(&socket, surface, |s| s["state"] == "exited");
    assert_eq!(exited["exit_code"], 0);
    assert!(
        exited["contents"]
            .as_str()
            .unwrap()
            .contains("REMOTE_INPUT:hello remote")
    );
    assert_eq!(fs::read_to_string(&counter).unwrap(), "run\n");
    json(&socket, &["close-surface", surface]);
    let restored = json(&socket, &["reopen-closed"]);
    let restored_surface = restored["focused_surface_uuid"].as_str().unwrap();
    // Readiness is asynchronous; start an input only after a running PTY is observable.
    screen(&socket, restored_surface, |s| s["state"] == "running");
    json(
        &socket,
        &[
            "send-text",
            "--surface",
            restored_surface,
            "printf 'REOPEN_REMOTE_READY\\n'\n",
        ],
    );
    screen(&socket, restored_surface, |s| {
        s["contents"]
            .as_str()
            .unwrap()
            .lines()
            .any(|line| line.ends_with("REOPEN_REMOTE_READY"))
    });
    assert_eq!(fs::read_to_string(&counter).unwrap(), "run\n");

    // A new login can no longer authenticate. Uploads must reuse this live terminal's
    // private master rather than reconnecting with another identity/configuration.
    fs::write(fixture.root.join("authorized_keys"), b"").unwrap();
    let source = fixture.root.join("image ' $(touch injected)-画像.png");
    let contents: Vec<u8> = (0..=255).cycle().take(100_003).collect();
    fs::write(&source, &contents).unwrap();
    // The frozen source delegates nonrecursive uploads to SCP: directory inputs fail,
    // while a selected symlink to a regular file uploads its bytes under the alias name.
    // A bad selection after a valid file must fail preparation before any remote operation.
    let directory = fixture.root.join("folder with spaces");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("private.txt"), b"must not upload").unwrap();
    let directory_link = fixture.root.join("folder alias");
    symlink(&directory, &directory_link).unwrap();
    let dangling = fixture.root.join("missing alias");
    symlink(fixture.root.join("does not exist"), &dangling).unwrap();
    let fifo = fixture.root.join("named pipe");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let oversized = fixture.root.join("oversized.bin");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(zmux_remote_runtime::upload::MAX_UPLOAD_FILE_BYTES + 1)
        .unwrap();
    let oversized_link = fixture.root.join("large alias");
    symlink(&oversized, &oversized_link).unwrap();
    for (rejected, reason) in [
        (&directory, "directories and special files"),
        (&directory_link, "directories and special files"),
        (&dangling, "open upload source"),
        (&fifo, "directories and special files"),
        (&oversized_link, "64 MiB upload limit"),
    ] {
        let started = json(
            &socket,
            &[
                "paste-files",
                "--surface",
                restored_surface,
                "--",
                source.to_str().unwrap(),
                rejected.to_str().unwrap(),
            ],
        );
        let id = started["created_uploads"][0].as_u64().unwrap();
        let failed = upload(&socket, id, |u| u["source_released"] == true);
        assert_eq!(failed["state"], "failed", "{failed}");
        assert_eq!(failed["delivery"], "failed", "{failed}");
        assert!(
            failed["error"].as_str().unwrap().contains(reason),
            "{failed}"
        );
        assert_eq!(failed["bytes_sent"], 0);
        assert_eq!(failed["remote_result_uncertain"], false);
        assert!(failed["remote_paths"].as_array().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
            0
        );
    }
    let alias = fixture.root.join("linked ' 画像.png");
    symlink(&source, &alias).unwrap();
    let linked = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            alias.to_str().unwrap(),
        ],
    );
    let id = linked["created_uploads"][0].as_u64().unwrap();
    let linked = upload(&socket, id, |u| u["source_released"] == true);
    assert_eq!(linked["state"], "completed", "{linked}");
    assert_eq!(linked["delivery"], "inserted", "{linked}");
    assert_eq!(linked["bytes_sent"], contents.len());
    let linked_path = Path::new(linked["remote_paths"][0].as_str().unwrap());
    assert_eq!(linked_path.file_name(), alias.file_name());
    assert_eq!(fs::read(linked_path).unwrap(), contents);
    fs::remove_file(linked_path).unwrap();
    fs::remove_dir(linked_path.parent().unwrap()).unwrap();
    // Remove the inserted path from the still-idle interactive shell before its next command.
    json(
        &socket,
        &["send-text", "--surface", restored_surface, "\u{15}"],
    );
    let restored_surface_id = SurfaceId(restored["focused_surface"].as_u64().unwrap());
    let before = desktop_snapshot::read(&desktop);
    let restored_session_id = before
        .model
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.root.panes())
        .flat_map(|pane| &pane.surfaces)
        .find(|surface| surface.id == restored_surface_id)
        .and_then(|surface| surface.session_id)
        .unwrap();
    let stale = desktop
        .execute_transfer_receipt(DesktopRequest {
            version: 1,
            request_id: 9_001,
            payload: DesktopCommand::Transfer {
                action: TransferCommand::PasteFiles {
                    surface_id: restored_surface_id,
                    expected_session_id: Some(SessionId(restored_session_id.0 + 1)),
                    paths: vec![source.clone()],
                },
            },
        })
        .unwrap_or_else(|error| {
            panic!(
                "stale remote receipt failed before acknowledgement: {}, unknown={}",
                error.error, error.outcome_unknown
            )
        });
    assert!(
        stale
            .result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("session changed"))
    );
    assert!(stale.upload_id.is_none());
    assert_eq!(
        desktop_snapshot::read(&desktop).uploads.len(),
        before.uploads.len()
    );
    assert_eq!(
        fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
        0,
        "a stale native receipt started remote upload work"
    );
    json(
        &socket,
        &[
            "send-text",
            "--surface",
            restored_surface,
            "IFS= read -r pasted; eval \"set -- $pasted\"; printf 'PATH:%s\\n' \"$1\"\n",
        ],
    );
    let local_pane = initial["workspaces"][0]["root"]["pane"]["id"]
        .as_u64()
        .unwrap()
        .to_string();
    json(&socket, &["focus-pane", "--pane", &local_pane]);
    let created_upload = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    assert_eq!(
        created_upload["focused_workspace"],
        initial["focused_workspace"]
    );
    let upload_id = created_upload["created_uploads"][0].as_u64().unwrap();
    let complete = upload(&socket, upload_id, |u| u["delivery"] != "pending");
    assert_eq!(complete["state"], "completed", "{complete}");
    assert_eq!(complete["delivery"], "inserted", "{complete}");
    assert_eq!(complete["remote_result_uncertain"], false);
    assert_eq!(complete["bytes_sent"], contents.len());
    let remote_path = complete["remote_paths"][0].as_str().unwrap();
    assert_eq!(fs::read(remote_path).unwrap(), contents);
    json(&socket, &["send-text", "--surface", restored_surface, "\n"]);
    screen(&socket, restored_surface, |s| {
        s["contents"]
            .as_str()
            .unwrap()
            .replace('\n', "")
            .contains(&format!("PATH:{remote_path}"))
    });
    assert!(
        !fixture.root.join("injected").exists(),
        "a quoted filename became shell code"
    );
    let local_screen = json(&socket, &["read-screen", "--pane", &local_pane]);
    assert!(
        !local_screen["contents"]
            .as_str()
            .unwrap()
            .contains("zmux-upload."),
        "an upload was pasted into the newly focused local terminal"
    );
    fs::remove_file(remote_path).unwrap();
    fs::remove_dir(Path::new(remote_path).parent().unwrap()).unwrap();

    // The remote script has ACKed a complete file, while a wrapper delays process exit.
    // Cancellation must suppress insertion but retain the already acknowledged path.
    fs::write(fixture.root.join("delay-ack"), b"").unwrap();
    let late = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let late_id = late["created_uploads"][0].as_u64().unwrap();
    let ack_deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.root.join("ack-ready").exists() {
        assert!(Instant::now() < ack_deadline, "remote upload did not ACK");
        thread::sleep(Duration::from_millis(10));
    }
    let bytes = fs::read(fixture.root.join("ack")).unwrap();
    let acknowledged = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(fs::read(acknowledged).unwrap(), contents);
    json(&socket, &["cancel-upload", &late_id.to_string()]);
    let late = upload(&socket, late_id, |u| u["source_released"] == true);
    assert_eq!(late["state"], "cancelled");
    assert_eq!(late["delivery"], "cancelled");
    assert_eq!(late["source_released"], true);
    assert_eq!(late["remote_result_uncertain"], false);
    assert_eq!(
        late["remote_paths"][0], acknowledged,
        "late cancellation lost an acknowledged completed file: {late}"
    );
    fs::remove_file(acknowledged).unwrap();
    fs::remove_dir(Path::new(acknowledged).parent().unwrap()).unwrap();
    fs::remove_file(fixture.root.join("delay-ack")).unwrap();

    // Deliver HUP immediately after printf writes the ACK. An acknowledged file
    // must already be retained; the remote cleanup trap must no longer delete it.
    fs::write(fixture.root.join("hangup-ack"), b"").unwrap();
    let hangup = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let hangup_id = hangup["created_uploads"][0].as_u64().unwrap();
    let hangup = upload(&socket, hangup_id, |u| u["delivery"] != "pending");
    let bytes = fs::read(fixture.root.join("ack")).unwrap();
    let committed = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
    assert!(
        Path::new(committed).is_file(),
        "HUP deleted a file after its acknowledgement: {hangup}"
    );
    assert_eq!(hangup["remote_paths"][0], committed);
    assert_eq!(hangup["remote_result_uncertain"], false);
    assert_eq!(fs::read(committed).unwrap(), contents);
    fs::remove_file(committed).unwrap();
    fs::remove_dir(Path::new(committed).parent().unwrap()).unwrap();
    fs::remove_file(fixture.root.join("hangup-ack")).unwrap();

    // A failed acknowledgement write must retain the remote cleanup trap.
    fs::write(fixture.root.join("broken-ack"), b"").unwrap();
    let broken = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            source.to_str().unwrap(),
        ],
    );
    let broken_id = broken["created_uploads"][0].as_u64().unwrap();
    let broken = upload(&socket, broken_id, |u| u["state"] == "failed");
    assert_eq!(broken["delivery"], "failed");
    assert_eq!(broken["remote_result_uncertain"], true);
    assert_eq!(
        fs::read_dir(fixture.root.join("uploads")).unwrap().count(),
        0,
        "failed acknowledgement disabled remote cleanup"
    );
    fs::remove_file(fixture.root.join("broken-ack")).unwrap();

    // Stalling the remote consumer must not block cancellation or core navigation.
    fs::write(fixture.root.join("slow"), b"").unwrap();
    let large = fixture.root.join("large.bin");
    fs::write(&large, vec![37_u8; 8 * 1024 * 1024]).unwrap();
    let started = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            large.to_str().unwrap(),
        ],
    );
    let id = started["created_uploads"][0].as_u64().unwrap();
    upload(&socket, id, |u| u["bytes_sent"].as_u64().unwrap() > 0);
    json(&socket, &["status"]); // Must remain responsive while the remote consumer stalls.
    json(&socket, &["cancel-upload", &id.to_string()]);
    let cancelled = upload(&socket, id, |u| u["state"] == "cancelled");
    assert_eq!(cancelled["delivery"], "cancelled");
    assert!(cancelled["remote_paths"].as_array().unwrap().is_empty());

    let closing = json(
        &socket,
        &[
            "paste-files",
            "--surface",
            restored_surface,
            "--",
            large.to_str().unwrap(),
        ],
    );
    let closing_id = closing["created_uploads"][0].as_u64().unwrap();
    json(&socket, &["close-surface", restored_surface]);
    let closed = upload(&socket, closing_id, |u| u["state"] == "cancelled");
    assert_eq!(closed["delivery"], "cancelled");
    assert_eq!(closed["surface_id"], restored["focused_surface"]);
    fs::copy(
        fixture.root.join("client key.pub"),
        fixture.root.join("authorized_keys"),
    )
    .unwrap();
    fixture.stop();
    let failed = json(
        &socket,
        &[
            "ssh",
            "fixture",
            "--ssh-config",
            &config,
            "--identity",
            &key,
            "--command",
            &startup,
        ],
    );
    let failed_surface = failed["created_workspaces"][0]["surface_uuid"]
        .as_str()
        .unwrap();
    let failure = screen(&socket, failed_surface, |s| s["state"] == "exited");
    assert_eq!(failure["exit_code"], 255);
    assert_eq!(
        fs::read_to_string(&counter).unwrap(),
        "run\n",
        "connection failure must never run the command locally"
    );
    server.shutdown(Duration::from_secs(2)).unwrap();
    desktop.shutdown(Duration::from_secs(5)).unwrap();
}
