//! Real interactive startup and hook-byte contract; no Claude account is used.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_session_runtime::{
    Launch, Session, SessionError, ShutdownPolicy, State, shell_integration,
};
use zmux_terminal_adapter::{MAX_PROGRAM_OPEN_REQUESTS, Size};

struct Directory(PathBuf);
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zmux-shell-contract-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn wait_for_session_output(session: &Session, expected: &str, context: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!("{context}: session failed: {:?}", snapshot.failure)
            }
            Ok(snapshot) if snapshot.viewport.contents.contains(expected) => return,
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("{context}: read session snapshot: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "{context}: session output did not contain {expected:?}"
        );
        thread::yield_now();
    }
}

fn wait_for_file(path: &std::path::Path, context: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "{context}: {} was not created",
            path.display()
        );
        thread::yield_now();
    }
}

#[test]
fn managed_ssh_bootstrap_fits_a_single_exec_argument() {
    let command =
        shell_integration::remote_command(&shell_integration::OpenRequestNonce::generate());
    let status = Command::new("/bin/true")
        .arg(&command)
        .status()
        .expect("managed SSH command must fit the platform exec argument limit");
    assert!(status.success());
    assert!(
        command.len() <= 64 * 1024,
        "managed SSH command is {} bytes, above the 64 KiB transport budget",
        command.len()
    );
}

fn plugin_host_line(lines: &mpsc::Receiver<std::io::Result<String>>, context: &str) -> String {
    lines
        .recv_timeout(Duration::from_secs(3))
        .unwrap_or_else(|error| panic!("{context}: no plugin host output: {error}"))
        .unwrap_or_else(|error| panic!("{context}: read plugin host output: {error}"))
}

struct PluginHostChild(Child);
impl Drop for PluginHostChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct OpenCodeServerChild(Child);
impl Drop for OpenCodeServerChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn opencode_plugin_loads_and_reports_only_exact_root_lifecycle_callbacks() {
    assert!(
        Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
        "OpenCode plugin callback integration tests require Node.js"
    );

    let root = PathBuf::from("/tmp")
        .canonicalize()
        .expect("resolve a short local Unix socket directory")
        .join(format!(
            "zprovider-plugin-{}",
            uuid::Uuid::new_v4().simple()
        ));
    fs::create_dir(&root).expect("create private plugin host directory");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let _directory = Directory(root.clone());
    let config = root.join("config");
    let plugins = config.join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&plugins, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(config.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let plugin = plugins.join("zmux-provider-fixture.js");
    fs::write(
        &plugin,
        include_str!("../src/shell/provider-opencode-plugin.js"),
    )
    .unwrap();
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o600)).unwrap();
    let host = root.join("provider-opencode-plugin-host.mjs");
    fs::write(
        &host,
        include_str!("fixtures/provider-opencode-plugin-host.mjs"),
    )
    .unwrap();
    let socket = root.join("events.sock");
    let root_session = "session:root/opaque";
    let mut child = PluginHostChild(
        Command::new("node")
            .arg(&host)
            .arg(&plugin)
            .arg(&socket)
            .arg(root_session)
            .env("ZMUX_PROVIDER_EVENT_SOCKET", &socket)
            .env("ZMUX_OPENCODE_EXPECTED_SESSION_ID", root_session)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the real OpenCode provider plugin in its callback host"),
    );
    let stdout = BufReader::new(child.0.stdout.take().unwrap());
    let (line_sender, lines) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in stdout.lines() {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut input = child.0.stdin.take().unwrap();
    assert_eq!(
        plugin_host_line(&lines, "plugin setup"),
        r#"{"ready":true}"#
    );

    let send = |input: &mut std::process::ChildStdin, operation: &str| {
        writeln!(input, "{{\"op\":\"{operation}\"}}").unwrap();
        input.flush().unwrap();
    };
    send(&mut input, "created");
    let baseline: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "root baseline")).unwrap();
    assert_eq!(baseline["observation"]["session_id"], root_session);
    assert_eq!(baseline["observation"]["state"], "idle");
    assert_eq!(baseline["observation"]["transition"], "snapshot");
    assert_eq!(baseline["observation"]["marker"], "baseline");
    assert_eq!(baseline["observation"].as_object().unwrap().len(), 7);

    // A foreign root event is followed by a prompt. The next producer frame
    // must belong to the exact expected root and carry an opaque, non-UUID ID.
    send(&mut input, "foreign");
    send(&mut input, "start");
    let working: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "root prompt callback")).unwrap();
    assert_eq!(working["observation"]["session_id"], root_session);
    assert_eq!(working["observation"]["turn_id"], "turn/opaque:1");
    assert_eq!(working["observation"]["state"], "working");
    assert_eq!(working["observation"]["transition"], "state_changed");
    assert_eq!(working["observation"]["marker"], "live");

    send(&mut input, "wait");
    let waiting: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "permission callback")).unwrap();
    assert_eq!(waiting["observation"]["state"], "waiting");
    assert_eq!(waiting["observation"]["turn_id"], "turn/opaque:1");
    send(&mut input, "resume");
    let resumed: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "permission reply callback")).unwrap();
    assert_eq!(resumed["observation"]["state"], "working");
    send(&mut input, "form-wait");
    let form_waiting: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "OpenCode form callback")).unwrap();
    assert_eq!(form_waiting["observation"]["state"], "waiting");
    send(&mut input, "form-resume");
    let form_resumed: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "OpenCode form reply callback")).unwrap();
    assert_eq!(form_resumed["observation"]["state"], "working");
    send(&mut input, "idle");
    let idle: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "idle source snapshot")).unwrap();
    assert_eq!(idle["observation"]["state"], "idle");
    assert_eq!(idle["observation"]["transition"], "snapshot");
    send(&mut input, "background");
    assert!(
        matches!(
            lines.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "idle is not completion while provider background work remains"
    );
    send(&mut input, "complete");
    let complete: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "experimental idle callback")).unwrap();
    assert_eq!(complete["observation"]["state"], "idle");
    assert_eq!(complete["observation"]["transition"], "turn_completed");
    send(&mut input, "next-start");
    let next_working: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "second OpenCode root turn")).unwrap();
    assert_eq!(next_working["observation"]["turn_id"], "turn/opaque:2");
    send(&mut input, "interrupt");
    let interrupted: serde_json::Value =
        serde_json::from_str(&plugin_host_line(&lines, "OpenCode interruption")).unwrap();
    assert_eq!(interrupted["observation"]["transition"], "turn_interrupted");
    assert_eq!(interrupted["observation"]["turn_id"], "turn/opaque:2");
    for line in [
        baseline,
        working,
        waiting,
        resumed,
        form_waiting,
        form_resumed,
        idle,
        complete,
        next_working,
        interrupted,
    ] {
        let encoded = line.to_string();
        assert!(!encoded.contains("private prompt"));
        assert!(!encoded.contains("permission/opaque"));
        assert!(!encoded.contains("form/opaque"));
        assert_eq!(line["observation"].as_object().unwrap().len(), 7);
    }

    send(&mut input, "shutdown");
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "OpenCode plugin host did not exit"
        );
        thread::yield_now();
    };
    assert!(status.success());
    reader.join().unwrap();
}

#[test]
#[ignore = "requires installed OpenCode v2.2.1; uses only synthetic local server auth, no provider account or model turn"]
fn installed_opencode_plugin_observes_a_real_root_session_without_provider_authentication() {
    let executable =
        std::env::var_os("ZMUX_OPENCODE_EXECUTABLE").unwrap_or_else(|| "opencode".into());
    let version = Command::new(&executable)
        .arg("--version")
        .output()
        .expect("run the installed OpenCode version probe");
    assert!(version.status.success());
    let version_text = format!(
        "{}{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
    assert!(
        version_text.contains("v2.2.1"),
        "installed OpenCode probe expects v2.2.1, got {version_text:?}"
    );

    let directory = Directory::new();
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
    let home = directory.0.join("home");
    let project = directory.0.join("project");
    let config = directory.0.join("config");
    let plugins = config.join("plugins");
    fs::create_dir_all(home.join(".config")).unwrap();
    fs::create_dir_all(home.join(".local/share")).unwrap();
    fs::create_dir_all(home.join(".cache")).unwrap();
    fs::create_dir(&project).unwrap();
    fs::create_dir_all(&plugins).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&plugins, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(config.join("opencode.json"), br#"{"autoupdate":false}"#).unwrap();
    let plugin = plugins.join("zmux-provider-acceptance.js");
    fs::write(
        &plugin,
        include_str!("../src/shell/provider-opencode-plugin.js"),
    )
    .unwrap();
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o600)).unwrap();

    let socket = directory.0.join("events.sock");
    let events = UnixListener::bind(&socket).expect("bind the private provider event route");
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    events.set_nonblocking(true).unwrap();

    let reservation =
        TcpListener::bind(("127.0.0.1", 0)).expect("reserve a local OpenCode API port");
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let port = address.port().to_string();
    let output_path = directory.0.join("opencode-server.log");
    let output = fs::File::create(&output_path).unwrap();
    let error_output = output.try_clone().unwrap();
    // The isolated server's local HTTP Basic credentials are synthetic. No
    // OpenCode provider account, API key, prompt, or model turn is involved.
    let server_password = "zmux-local-acceptance-only";
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(&executable);
    command
        .arg("--print-logs")
        .arg("serve")
        .arg("--hostname")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(&port)
        .current_dir(&project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("OPENCODE_CONFIG_DIR", &config)
        .env("OPENCODE_SERVER_PASSWORD", server_password)
        .env("ZMUX_PROVIDER_EVENT_SOCKET", &socket)
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(error_output));
    let mut server = OpenCodeServerChild(
        command
            .spawn()
            .expect("start the installed OpenCode standalone API server"),
    );

    fn http_request(
        address: std::net::SocketAddr,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Option<(u16, Vec<u8>)> {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(300)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Basic b3BlbmNvZGU6em11eC1sb2NhbC1hY2NlcHRhbmNlLW9ubHk=\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .ok()?;
        stream.write_all(body).ok()?;
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        let separator = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")?;
        let headers = String::from_utf8_lossy(&response[..separator]);
        let status = headers
            .lines()
            .next()?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        let raw_body = &response[separator + 4..];
        if headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("transfer-encoding: chunked"))
        {
            let mut decoded = Vec::new();
            let mut rest = raw_body;
            loop {
                let line_end = rest.windows(2).position(|window| window == b"\r\n")?;
                let size_text = std::str::from_utf8(&rest[..line_end]).ok()?;
                let size = usize::from_str_radix(size_text.split(';').next()?, 16).ok()?;
                rest = &rest[line_end + 2..];
                if size == 0 {
                    break;
                }
                if rest.len() < size + 2 || &rest[size..size + 2] != b"\r\n" {
                    return None;
                }
                decoded.extend_from_slice(&rest[..size]);
                rest = &rest[size + 2..];
            }
            return Some((status, decoded));
        }
        Some((status, raw_body.to_vec()))
    }

    let health_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            panic!(
                "OpenCode standalone server exited before readiness: {status}; log={}",
                fs::read_to_string(&output_path).unwrap_or_default()
            );
        }
        if http_request(address, "GET", "/global/health", b"")
            .is_some_and(|(status, _)| status == 200)
        {
            break;
        }
        assert!(
            Instant::now() < health_deadline,
            "OpenCode standalone server did not become ready: {}",
            fs::read_to_string(&output_path).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(25));
    }

    let (spec_status, spec_body) = http_request(address, "GET", "/openapi.json", b"")
        .expect("read the installed OpenCode API schema");
    assert_eq!(
        spec_status, 200,
        "OpenCode OpenAPI endpoint was unavailable"
    );
    let spec: serde_json::Value =
        serde_json::from_slice(&spec_body).expect("decode installed OpenCode API schema");
    assert!(
        spec["paths"]["/api/session"]["post"].is_object(),
        "OpenCode v2.2.1 OpenAPI schema omitted POST /api/session"
    );

    let plugin_deadline = Instant::now() + Duration::from_secs(5);
    let loaded_plugins = loop {
        let (plugin_status, plugin_body) = http_request(address, "GET", "/api/plugin", b"")
            .expect("read the installed OpenCode plugin inventory");
        assert_eq!(
            plugin_status, 200,
            "OpenCode plugin inventory was unavailable"
        );
        let loaded_plugins: serde_json::Value =
            serde_json::from_slice(&plugin_body).expect("decode OpenCode plugin inventory");
        let provider_plugin = loaded_plugins["data"].as_array().and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin["id"] == "zmux.provider.lifecycle")
        });
        if provider_plugin.is_some() || Instant::now() >= plugin_deadline {
            break loaded_plugins;
        }
        thread::sleep(Duration::from_millis(25));
    };
    assert!(
        loaded_plugins["data"]
            .as_array()
            .is_some_and(|plugins| plugins.iter().any(|plugin| {
                plugin["id"] == "zmux.provider.lifecycle" && plugin["state"]["status"] == "active"
            })),
        "invocation-scoped provider plugin did not load: {loaded_plugins}; logs={}",
        fs::read_to_string(&output_path).unwrap_or_default()
    );

    let (status, response) = http_request(
        address,
        "POST",
        "/api/session",
        br#"{"title":"Zmux empty provider acceptance session"}"#,
    )
    .expect("create an empty provider session through the OpenCode API");
    assert!(
        (200..300).contains(&status),
        "OpenCode POST /api/session returned {status}; response bytes={}",
        response.len(),
    );
    let created: serde_json::Value =
        serde_json::from_slice(&response).expect("decode OpenCode session response");
    let session_id = created["id"]
        .as_str()
        .or_else(|| created["data"]["id"].as_str())
        .expect("OpenCode API returned the created root session ID");

    let deadline = Instant::now() + Duration::from_secs(10);
    let observation = loop {
        match events.accept() {
            Ok((mut stream, _)) => {
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).unwrap();
                let line = bytes
                    .split(|byte| *byte == b'\n')
                    .find(|line| !line.is_empty())
                    .expect("OpenCode plugin wrote one provider observation");
                break serde_json::from_slice::<serde_json::Value>(line).unwrap();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("accept OpenCode plugin observation: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "OpenCode v2 plugin did not observe session.created: {}",
            fs::read_to_string(&output_path).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(observation["kind"], "opencode_observation");
    assert_eq!(observation["observation"]["provider"], "opencode");
    assert_eq!(observation["observation"]["scope"], "root");
    assert_eq!(observation["observation"]["session_id"], session_id);
    assert_eq!(
        observation["observation"]["turn_id"],
        serde_json::Value::Null
    );
    assert_eq!(observation["observation"]["state"], "idle");
    assert_eq!(observation["observation"]["transition"], "snapshot");
    assert_eq!(observation["observation"]["marker"], "baseline");
    assert_eq!(observation["observation"].as_object().unwrap().len(), 7);

    server.0.kill().unwrap();
    server.0.wait().unwrap();
}

#[test]
fn local_provider_shell_functions_preserve_codex_and_opencode_arguments() {
    let directory = Directory::new();
    let cli = directory.0.join("zmux-provider-cli");
    fs::write(
        &cli,
        "#!/bin/sh\nprintf '<%s>\\n' \"$@\" >> \"$ZMUX_AUTO_PROVIDER_REPORT\"\nexit 37\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    let report = directory.0.join("provider-cli-arguments");
    let surface = uuid::Uuid::new_v4().to_string();
    let startup = directory.0.join("provider-shrc");
    fs::write(&startup, "PS1=; printf 'PROVIDER_SHELL_READY\\n'\n").unwrap();
    let (launch, lease) = shell_integration::prepare(Launch {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: Some(directory.0.clone()),
        size: Size {
            rows: 12,
            cols: 100,
        },
    })
    .unwrap();
    let mut session = Session::spawn_with_resource(
        launch,
        &[
            ("HOME".into(), directory.0.to_string_lossy().into_owned()),
            ("ENV".into(), startup.to_string_lossy().into_owned()),
            ("ZMUX_LOCAL_CLAUDE_LIFECYCLE".into(), "1".into()),
            ("ZMUX_CLI_PATH".into(), cli.to_string_lossy().into_owned()),
            ("ZMUX_SURFACE_ID".into(), surface.clone()),
            (
                "ZMUX_AUTO_PROVIDER_REPORT".into(),
                report.to_string_lossy().into_owned(),
            ),
        ],
        lease,
    )
    .unwrap();
    wait_for_session_output(&session, "PROVIDER_SHELL_READY", "provider shell startup");

    session
        .send_input(b"codex --model o3 'literal $HOME ; prompt'\n")
        .unwrap();
    let first_deadline = Instant::now() + Duration::from_secs(5);
    while !report.is_file() {
        assert!(
            Instant::now() < first_deadline,
            "Codex shell wrapper did not dispatch"
        );
        thread::yield_now();
    }
    session
        .send_input(b"opencode --session 'session:opaque' 'literal $HOME ; prompt'\n")
        .unwrap();
    let expected_lines = 24;
    let second_deadline = Instant::now() + Duration::from_secs(5);
    let lines = loop {
        let contents = fs::read_to_string(&report).unwrap();
        let lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
        if lines.len() >= expected_lines {
            break lines;
        }
        assert!(
            Instant::now() < second_deadline,
            "OpenCode shell wrapper did not dispatch"
        );
        thread::yield_now();
    };

    let integration_root = lines[5]
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .map(PathBuf::from)
        .expect("provider wrapper received the private session integration path");
    assert!(integration_root.is_absolute());
    assert!(
        integration_root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("zmux-shell-"))
    );
    let root_metadata = fs::metadata(&integration_root).unwrap();
    assert!(root_metadata.is_dir());
    assert_eq!(root_metadata.permissions().mode() & 0o777, 0o700);

    for (offset, provider) in [(0, "codex"), (12, "opencode")] {
        assert_eq!(lines[offset], "<hooks>");
        assert_eq!(lines[offset + 1], "<provider>");
        assert_eq!(lines[offset + 2], "<auto>");
        assert_eq!(lines[offset + 3], format!("<{provider}>"));
        assert_eq!(lines[offset + 4], "<--root>");
        assert_eq!(
            lines[offset + 5],
            format!("<{}>", integration_root.display())
        );
        assert_eq!(lines[offset + 6], "<--surface>");
        assert_eq!(lines[offset + 7], format!("<{surface}>"));
        assert_eq!(lines[offset + 8], "<-->");
        assert_eq!(
            lines[offset + 9],
            if provider == "codex" {
                "<--model>"
            } else {
                "<--session>"
            }
        );
        assert_eq!(
            lines[offset + 10],
            if provider == "codex" {
                "<o3>"
            } else {
                "<session:opaque>"
            }
        );
        assert_eq!(lines[offset + 11], "<literal $HOME ; prompt>");
    }
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn shell_integration_canonicalizes_a_symlinked_temp_directory() {
    let directory = Directory::new();
    let real_temp = directory.0.join("real-temp");
    fs::create_dir(&real_temp).unwrap();
    fs::set_permissions(&real_temp, fs::Permissions::from_mode(0o700)).unwrap();
    let temp_alias = directory.0.join("temp-alias");
    symlink(&real_temp, &temp_alias).unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "shell_integration_canonicalizes_a_symlinked_temp_directory_child",
            "--nocapture",
        ])
        .env("TMPDIR", &temp_alias)
        .env("FIXTURE_REAL_TEMP", &real_temp)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "symlinked temp child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "invoked by the parent test with a private symlinked TMPDIR"]
fn shell_integration_canonicalizes_a_symlinked_temp_directory_child() {
    let expected = PathBuf::from(std::env::var_os("FIXTURE_REAL_TEMP").unwrap())
        .canonicalize()
        .unwrap();
    let (launch, lease) = shell_integration::prepare(Launch {
        program: "/bin/bash".into(),
        args: vec!["-i".into()],
        cwd: Some(expected.clone()),
        size: Size {
            rows: 12,
            cols: 100,
        },
    })
    .unwrap();
    let root = PathBuf::from(&launch.args[3]);
    assert!(
        root.starts_with(&expected),
        "{} not under {}",
        root.display(),
        expected.display()
    );
    assert_ne!(
        root,
        PathBuf::from(std::env::var_os("TMPDIR").unwrap()).join(root.file_name().unwrap())
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    drop(lease);
}

#[test]
fn interactive_startup_keeps_user_configuration_and_notification_opt_out() {
    let directory = Directory::new();
    let provider = directory.0.join("claude");
    fs::write(
        &provider,
        r#"#!/bin/sh
if [ "$1" = --version ]; then printf 'VERSION_PASSTHROUGH\n'; exit 0; fi
[ "$STARTUP_VALUE" = preserved ] || exit 77
[ "$1" = --plugin-dir ] || exit 78
[ -r "$2/hooks/hooks.json" ] || exit 79
shift 2
[ "$1" = 'literal $HOME ; argument' ] || exit 80
printf 'PROVIDER_ARGUMENTS_PRESERVED\n'
"#,
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let rc = format!(
        "export PATH=\"{}:$PATH\"\nexport STARTUP_VALUE=preserved\nPS1=\nprintf 'USER_STARTUP_LOADED\\n'\n",
        directory.0.display()
    );
    for name in ["bashrc", "shrc", ".zshrc"] {
        fs::write(directory.0.join(name), &rc).unwrap();
    }
    let mut cases = vec![
        (
            "/bin/bash",
            vec![
                "--noprofile".into(),
                "--rcfile".into(),
                directory.0.join("bashrc").to_str().unwrap().into(),
                "-i".into(),
            ],
        ),
        ("/bin/sh", vec!["-i".into()]),
    ];
    if cfg!(target_os = "macos") {
        cases.push(("/bin/zsh", vec!["-i".into()]));
    }
    for (program, args) in cases {
        let (launch, lease) = shell_integration::prepare(Launch {
            program: program.into(),
            args,
            cwd: Some(directory.0.clone()),
            size: Size {
                rows: 12,
                cols: 100,
            },
        })
        .unwrap();
        assert!(lease.is_some());
        let root = directory.0.to_str().unwrap().to_string();
        let mut session = Session::spawn_with_resource(
            launch,
            &[
                ("HOME".into(), root.clone()),
                ("ZDOTDIR".into(), root),
                (
                    "ENV".into(),
                    directory.0.join("shrc").to_str().unwrap().into(),
                ),
            ],
            lease,
        )
        .unwrap();
        let wait = |needle: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(frame) = session.snapshot() {
                    assert_ne!(frame.state, State::Failed, "{:?}", frame.failure);
                    if frame.viewport.contents.contains(needle) {
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "{program} did not show {needle}");
                thread::sleep(Duration::from_millis(5));
            }
        };
        wait("USER_STARTUP_LOADED");
        session
            .send_input(b"claude 'literal $HOME ; argument'\n")
            .unwrap();
        wait("PROVIDER_ARGUMENTS_PRESERVED");
        session
            .send_input(b"ZMUX_CLAUDE_INTEGRATION_DISABLED=1 claude --version\n")
            .unwrap();
        wait("VERSION_PASSTHROUGH");
        session.shutdown(Duration::from_secs(3)).unwrap();
    }
}

#[test]
fn integrated_shells_emit_prompt_boundaries_after_user_hooks_without_relaunching_commands() {
    let directory = Directory::new();
    let fish_config = directory.0.join(".config/fish");
    fs::create_dir_all(&fish_config).unwrap();
    fs::write(
        directory.0.join(".bashrc"),
        "PROMPT_COMMAND='printf x >> \"$ZMUX_PROMPT_HOOK_REPORT\"'\nPS1='zmux> '\n",
    )
    .unwrap();
    fs::write(
        directory.0.join(".zshrc"),
        "PROMPT='zmux> '\nprecmd() { printf x >> \"$ZMUX_PROMPT_HOOK_REPORT\"; }\n",
    )
    .unwrap();
    fs::write(
        fish_config.join("config.fish"),
        "function fish_prompt\n    printf x >> \"$ZMUX_PROMPT_HOOK_REPORT\"\n    printf 'zmux> '\nend\n",
    )
    .unwrap();

    let mut tested = 0;
    for (shell, args) in [
        ("bash", vec!["-i"]),
        ("zsh", vec!["-i"]),
        ("fish", vec!["--interactive"]),
    ] {
        if !Command::new(shell)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            eprintln!("{shell} is not installed; integrated prompt coverage skipped");
            continue;
        }
        tested += 1;
        let prompt_report = directory.0.join(format!("{shell}-prompt-hooks"));
        let command_report = directory.0.join(format!("{shell}-command-runs"));
        let (launch, lease) = shell_integration::prepare(Launch {
            program: shell.into(),
            args: args.into_iter().map(str::to_owned).collect(),
            cwd: Some(directory.0.clone()),
            size: Size { rows: 8, cols: 72 },
        })
        .unwrap();
        let mut session = Session::spawn_with_resource(
            launch,
            &[
                ("HOME".into(), directory.0.to_string_lossy().into_owned()),
                ("ZDOTDIR".into(), directory.0.to_string_lossy().into_owned()),
                (
                    "ZMUX_PROMPT_HOOK_REPORT".into(),
                    prompt_report.to_string_lossy().into_owned(),
                ),
                (
                    "ZMUX_COMMAND_RUN_REPORT".into(),
                    command_report.to_string_lossy().into_owned(),
                ),
            ],
            lease,
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        let initial_epoch = loop {
            let snapshot = session.snapshot().unwrap();
            assert_ne!(
                snapshot.state,
                State::Failed,
                "{shell}: {:?}",
                snapshot.failure
            );
            if snapshot.viewport.program_status.command_epoch.0 > 0 && prompt_report.is_file() {
                break snapshot.viewport.program_status.command_epoch.0;
            }
            assert!(
                Instant::now() < deadline,
                "{shell} prompt marker did not run"
            );
            thread::sleep(Duration::from_millis(3));
        };
        let prompt_count_before = fs::read_to_string(&prompt_report).unwrap().len();
        session
            .send_input(
                b"printf '\\033]7501;state=working:id=shell-cycle\\007'; printf x >> \"$ZMUX_COMMAND_RUN_REPORT\"\n",
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let snapshot = session.snapshot().unwrap();
            assert_ne!(
                snapshot.state,
                State::Failed,
                "{shell}: {:?}",
                snapshot.failure
            );
            let prompt_count = fs::read_to_string(&prompt_report).map_or(0, |text| text.len());
            let status_is_retired = !snapshot
                .viewport
                .program_status
                .records
                .iter()
                .any(|record| record.id.as_ref() == "shell-cycle");
            if snapshot.viewport.program_status.command_epoch.0 > initial_epoch
                && prompt_count > prompt_count_before
                && command_report.is_file()
                && status_is_retired
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{shell} command prompt cycle did not complete"
            );
            thread::sleep(Duration::from_millis(3));
        }
        assert_eq!(
            fs::read_to_string(&command_report).unwrap(),
            "x",
            "{shell} command ran more than once"
        );
        session.shutdown(Duration::from_secs(3)).unwrap();
    }
    assert!(tested > 0, "no supported interactive shell is installed");
}

#[test]
fn automatic_local_claude_wrapper_dispatches_once_with_notice_plugin_and_original_args() {
    let directory = Directory::new();
    let provider = directory.0.join("claude");
    fs::write(
        &provider,
        "#!/bin/sh\nprintf called > \"$ZMUX_AUTO_PROVIDER_CALLED\"\n",
    )
    .unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let cli = directory.0.join("zmux-test-cli");
    fs::write(
        &cli,
        "#!/bin/sh\ncase \"$3\" in\n auto) report_tmp=\"$ZMUX_AUTO_CLI_REPORT.tmp\"; printf '<%s>\\n' \"$@\" > \"$report_tmp\" || exit 92; mv \"$report_tmp\" \"$ZMUX_AUTO_CLI_REPORT\" || exit 93; printf 'AUTO_DISPATCH_OK\\n'; exit 37 ;;\n cleanup) exit 0 ;;\n *) exit 91 ;;\nesac\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    let startup = directory.0.join("shrc");
    fs::write(
        &startup,
        format!(
            "PATH=\"{}:$PATH\"; export PATH; PS1=; printf 'AUTO_SHELL_READY\\n'\n",
            directory.0.display()
        ),
    )
    .unwrap();
    let report = directory.0.join("auto-cli-args");
    let provider_called = directory.0.join("provider-called");
    let surface = uuid::Uuid::new_v4().to_string();
    let (launch, lease) = shell_integration::prepare(Launch {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        cwd: Some(directory.0.clone()),
        size: Size {
            rows: 12,
            cols: 100,
        },
    })
    .unwrap();
    let mut session = Session::spawn_with_resource(
        launch,
        &[
            ("HOME".into(), directory.0.to_string_lossy().into_owned()),
            ("ENV".into(), startup.to_string_lossy().into_owned()),
            ("ZMUX_LOCAL_CLAUDE_LIFECYCLE".into(), "1".into()),
            ("ZMUX_CLI_PATH".into(), cli.to_string_lossy().into_owned()),
            ("ZMUX_SURFACE_ID".into(), surface.clone()),
            (
                "ZMUX_AUTO_CLI_REPORT".into(),
                report.to_string_lossy().into_owned(),
            ),
            (
                "ZMUX_AUTO_PROVIDER_CALLED".into(),
                provider_called.to_string_lossy().into_owned(),
            ),
        ],
        lease,
    )
    .unwrap();
    wait_for_session_output(&session, "AUTO_SHELL_READY", "automatic shell startup");
    session
        .send_input(b"claude --model sonnet 'literal $HOME ; prompt'\n")
        .unwrap();
    wait_for_file(&report, "Claude wrapper dispatch");
    let arguments = fs::read_to_string(report).unwrap();
    let lines: Vec<_> = arguments.lines().collect();
    assert_eq!(lines[0], "<hooks>");
    assert_eq!(lines[1], "<claude>");
    assert_eq!(lines[2], "<auto>");
    assert_eq!(lines[3], "<--root>");
    assert_eq!(lines[5], "<--surface>");
    assert_eq!(lines[6], format!("<{surface}>"));
    assert_eq!(lines[7], "<--notice-plugin>");
    let session_root = lines[4]
        .strip_prefix('<')
        .unwrap()
        .strip_suffix('>')
        .unwrap();
    assert_eq!(lines[8], format!("<{session_root}/plugin>"));
    assert_eq!(lines[9], "<-->");
    assert_eq!(lines[10], "<--model>");
    assert_eq!(lines[11], "<sonnet>");
    assert_eq!(lines[12], "<literal $HOME ; prompt>");
    assert!(
        !provider_called.exists(),
        "failed adapter dispatch was replayed"
    );
    wait_for_session_output(&session, "AUTO_DISPATCH_OK", "automatic CLI result");
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn automatic_local_claude_fish_wrapper_dispatches_when_fish_is_installed() {
    if !Command::new("fish")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("fish is not installed; conditional fish wrapper coverage skipped");
        return;
    }

    let directory = Directory::new();
    let provider = directory.0.join("claude");
    fs::write(&provider, "#!/bin/sh\nexit 99\n").unwrap();
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700)).unwrap();
    let cli = directory.0.join("zmux-test-cli");
    fs::write(
        &cli,
        "#!/bin/sh\nprintf '<%s>\\n' \"$@\" > \"$ZMUX_AUTO_CLI_REPORT\"\nprintf 'FISH_AUTO_DISPATCH_OK\\n'\nexit 37\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    let report = directory.0.join("fish-auto-cli-args");
    let provider_called = directory.0.join("fish-provider-called");
    let surface = uuid::Uuid::new_v4().to_string();
    let (launch, lease) = shell_integration::prepare(Launch {
        program: "fish".into(),
        args: vec!["--interactive".into()],
        cwd: Some(directory.0.clone()),
        size: Size {
            rows: 12,
            cols: 100,
        },
    })
    .unwrap();
    let mut session = Session::spawn_with_resource(
        launch,
        &[
            ("HOME".into(), directory.0.to_string_lossy().into_owned()),
            ("ZMUX_LOCAL_CLAUDE_LIFECYCLE".into(), "1".into()),
            ("ZMUX_CLI_PATH".into(), cli.to_string_lossy().into_owned()),
            ("ZMUX_SURFACE_ID".into(), surface.clone()),
            (
                "ZMUX_AUTO_CLI_REPORT".into(),
                report.to_string_lossy().into_owned(),
            ),
            (
                "ZMUX_AUTO_PROVIDER_CALLED".into(),
                provider_called.to_string_lossy().into_owned(),
            ),
        ],
        lease,
    )
    .unwrap();
    session
        .send_input(b"claude --model sonnet 'literal $HOME ; prompt'\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !report.exists() {
        let snapshot = session.snapshot().unwrap();
        assert_ne!(snapshot.state, State::Failed, "{:?}", snapshot.failure);
        assert!(
            Instant::now() < deadline,
            "fish Claude wrapper did not dispatch"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let lines: Vec<_> = fs::read_to_string(&report)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(lines[0], "<hooks>");
    assert_eq!(lines[1], "<claude>");
    assert_eq!(lines[2], "<auto>");
    assert_eq!(lines[3], "<--root>");
    let session_root = lines[4]
        .strip_prefix('<')
        .unwrap()
        .strip_suffix('>')
        .unwrap();
    assert_eq!(lines[5], "<--surface>");
    assert_eq!(lines[6], format!("<{surface}>"));
    assert_eq!(lines[7], "<--notice-plugin>");
    assert_eq!(lines[8], format!("<{session_root}/plugin>"));
    assert_eq!(lines[9], "<-->");
    assert_eq!(lines[10], "<--model>");
    assert_eq!(lines[11], "<sonnet>");
    assert_eq!(lines[12], "<literal $HOME ; prompt>");
    assert!(!provider_called.exists(), "failed dispatch replayed Claude");
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn opener_survives_startup_path_rebuild_and_routes_only_explicit_requests() {
    let directory = Directory::new();
    let startup = directory.0.join("bashrc");
    let report = directory.0.join("opener-report");
    let fake_bin = directory.0.join("fake-bin");
    fs::create_dir(&fake_bin).unwrap();
    let delegate_report = directory.0.join("delegate-report");
    let fake_open = fake_bin.join("open");
    fs::write(
        &fake_open,
        "#!/bin/sh\nprintf '<%s>\\n' \"$@\" > \"$ZMUX_DELEGATE_REPORT\"\n",
    )
    .unwrap();
    fs::set_permissions(&fake_open, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        &startup,
        "PATH=\"$ZMUX_FAKE_BIN:/usr/bin:/bin\"\nexport PATH\nPS1=\nprintf 'USER_STARTUP_READY\\n'\n",
    )
    .unwrap();
    let nonce = shell_integration::OpenRequestNonce::generate();
    let (launch, lease) = shell_integration::prepare_with_nonce(
        Launch {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                startup.to_str().unwrap().into(),
                "-i".into(),
            ],
            cwd: Some(directory.0.clone()),
            size: Size {
                rows: 12,
                cols: 100,
            },
        },
        &nonce,
    )
    .unwrap();
    let mut session = Session::spawn_with_open_requests(
        launch,
        &[
            ("ZMUX_OPENER_REPORT".into(), report.to_str().unwrap().into()),
            ("ZMUX_FAKE_BIN".into(), fake_bin.to_str().unwrap().into()),
            (
                "ZMUX_DELEGATE_REPORT".into(),
                delegate_report.to_str().unwrap().into(),
            ),
        ],
        &[],
        ShutdownPolicy::Kill,
        None,
        Some(shell_integration::OpenRequestRegistration::new(
            nonce,
            shell_integration::OpenRequestSource::Local,
        )),
        lease,
    )
    .unwrap();
    assert!(session.open_request_registration().is_some());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = session.snapshot().unwrap();
        assert_ne!(snapshot.state, State::Failed, "{:?}", snapshot.failure);
        if snapshot.viewport.contents.contains("USER_STARTUP_READY") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shell startup marker was not printed"
        );
        thread::sleep(Duration::from_millis(5));
    }
    session
        .send_input(
            b"printf '%s\\n' \"$BROWSER\" \"$(command -v open)\" > \"$ZMUX_OPENER_REPORT\"\n",
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !report.exists() {
        assert!(
            Instant::now() < deadline,
            "shell did not report opener paths"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let report_text = fs::read_to_string(report).unwrap();
    let mut report_lines = report_text.lines();
    let browser = report_lines.next().unwrap();
    let open_path = report_lines.next().unwrap();
    assert!(browser.ends_with("/bin/zmux-open-url"), "BROWSER={browser}");
    assert!(
        open_path.ends_with("/bin/open"),
        "open resolved to {open_path}"
    );

    session
        .send_input(b"open --fixture 'literal ; argument'\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !delegate_report.exists() {
        assert!(
            Instant::now() < deadline,
            "unsupported opener arguments were not delegated"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fs::read_to_string(delegate_report).unwrap(),
        "<--fixture>\n<literal ; argument>\n"
    );

    // A mismatched nonce burst must be discarded before parser queue admission.
    // The PTY stays live and the burst cannot consume the bounded matching-request queue.
    let mismatched_requests =
        "\\033]777;zmux-open;v1;00000000000000000000000000000000;aHR0cHM6Ly9leGFtcGxlLmNvbQ\\007"
            .repeat(MAX_PROGRAM_OPEN_REQUESTS + 1);
    let mismatched_burst = format!(
        "printf '{}'\nprintf 'MISMATCHED_NONCE_BURST_SENT\\n'\n",
        mismatched_requests
    );
    session.send_input(mismatched_burst.as_bytes()).unwrap();
    wait_for_contents(&session, "MISMATCHED_NONCE_BURST_SENT");
    assert_eq!(session.snapshot().unwrap().state, State::Running);
    assert!(session.take_open_urls().unwrap().is_empty());

    // Malformed and mismatched single events also remain ignored before valid helpers.
    let open_commands =
        "printf '\\033]777;zmux-open;v1;%s;aHR0cHM6Ly9leGFtcGxlLmNvbQ=\\007' \"$ZMUX_OPEN_URL_NONCE\"\n\
         printf '\\033]777;zmux-open;v1;00000000000000000000000000000000;aHR0cHM6Ly9leGFtcGxlLmNvbQ\\007'\n\
         open https://open.example.test/path\n\
         \"$BROWSER\" https://browser.example.test/login\n"
            .to_owned();
    session.send_input(open_commands.as_bytes()).unwrap();
    let mut opened = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while opened.len() < 2 {
        match session.take_open_urls() {
            Ok(urls) => opened.extend(urls.into_iter().map(|url| url.as_str().to_owned())),
            Err(SessionError::Busy) => {}
            Err(error) => {
                panic!("session stopped before accepting explicit opener requests: {error}")
            }
        }
        assert!(
            Instant::now() < deadline,
            "explicit opener requests were not drained"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        opened,
        [
            "https://open.example.test/path",
            "https://browser.example.test/login"
        ]
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn opener_requests_without_session_registration_are_ignored() {
    let directory = Directory::new();
    let startup = directory.0.join("bashrc");
    fs::write(&startup, "PS1=\nprintf 'USER_STARTUP_READY\\n'\n").unwrap();
    let nonce = "00000000000000000000000000000000";
    let (launch, lease) = shell_integration::prepare_with_nonce(
        Launch {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                startup.to_str().unwrap().into(),
                "-i".into(),
            ],
            cwd: Some(directory.0.clone()),
            size: Size {
                rows: 12,
                cols: 100,
            },
        },
        &shell_integration::OpenRequestNonce::generate(),
    )
    .unwrap();
    // A caller-supplied nonce cannot stand in for the missing in-memory binding.
    let mut session = Session::spawn_with_open_requests(
        launch,
        &[
            ("ZMUX_OPEN_URL_NONCE".into(), nonce.into()),
            (
                "ZMUX_OUTPUT_MARKER".into(),
                "UNREGISTERED_REQUEST_SENT".into(),
            ),
        ],
        &[],
        ShutdownPolicy::Kill,
        None,
        None,
        lease,
    )
    .unwrap();
    assert!(session.open_request_registration().is_none());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = session.snapshot().unwrap();
        assert_ne!(snapshot.state, State::Failed, "{:?}", snapshot.failure);
        if snapshot.viewport.contents.contains("USER_STARTUP_READY") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shell startup marker was not printed"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let unregistered_requests =
        format!("\\033]777;zmux-open;v1;{nonce};aHR0cHM6Ly9leGFtcGxlLmNvbQ\\007")
            .repeat(MAX_PROGRAM_OPEN_REQUESTS + 1);
    let unregistered_burst = format!(
        "printf '{}'\nprintf '%s\\n' \"$ZMUX_OUTPUT_MARKER\"\n",
        unregistered_requests
    );
    session.send_input(unregistered_burst.as_bytes()).unwrap();
    wait_for_contents(&session, "UNREGISTERED_REQUEST_SENT");
    assert_eq!(session.snapshot().unwrap().state, State::Running);
    assert!(session.take_open_urls().unwrap().is_empty());
    session.shutdown(Duration::from_secs(3)).unwrap();
}

fn wait_for_contents(session: &Session, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = session.snapshot().unwrap();
        assert_ne!(snapshot.state, State::Failed, "{:?}", snapshot.failure);
        if snapshot.viewport.contents.contains(marker) {
            return;
        }
        assert!(Instant::now() < deadline, "shell did not print {marker}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn opener_preserves_browser_values_set_by_user_startup() {
    let directory = Directory::new();
    let startup = directory.0.join("bashrc");
    let report = directory.0.join("browser-overrides");
    fs::write(
        &startup,
        "PATH=/usr/bin:/bin\nBROWSER=''\nGH_BROWSER='startup-browser'\nexport PATH BROWSER GH_BROWSER\nPS1=\nprintf 'USER_STARTUP_READY\\n'\n",
    )
    .unwrap();
    let nonce = shell_integration::OpenRequestNonce::generate();
    let (launch, lease) = shell_integration::prepare_with_nonce(
        Launch {
            program: "/bin/bash".into(),
            args: vec![
                "--noprofile".into(),
                "--rcfile".into(),
                startup.to_str().unwrap().into(),
                "-i".into(),
            ],
            cwd: Some(directory.0.clone()),
            size: Size {
                rows: 12,
                cols: 100,
            },
        },
        &nonce,
    )
    .unwrap();
    let mut session = Session::spawn_with_open_requests(
        launch,
        &[("ZMUX_OPENER_REPORT".into(), report.to_str().unwrap().into())],
        &[],
        ShutdownPolicy::Kill,
        None,
        Some(shell_integration::OpenRequestRegistration::new(
            nonce,
            shell_integration::OpenRequestSource::Local,
        )),
        lease,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = session.snapshot().unwrap();
        assert_ne!(snapshot.state, State::Failed, "{:?}", snapshot.failure);
        if snapshot.viewport.contents.contains("USER_STARTUP_READY") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shell startup marker was not printed"
        );
        thread::sleep(Duration::from_millis(5));
    }
    session
        .send_input(b"printf '%s\\n' \"$BROWSER\" \"$GH_BROWSER\" > \"$ZMUX_OPENER_REPORT\"\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !report.exists() {
        assert!(
            Instant::now() < deadline,
            "shell did not report browser overrides"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fs::read_to_string(report).unwrap(), "\nstartup-browser\n");
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn posix_owner_and_group_hangup_clean_hooks_before_the_foreground_shell_exits() {
    for group in [false, true] {
        let directory = Directory::new();
        let report = directory.0.join("lease-root");
        // Optional local portability reproduction with an independently installed
        // older Bash. CI uses the platform's actual /bin/bash without an override.
        let bash = std::env::var("ZMUX_TEST_BASH").unwrap_or_else(|_| "/bin/bash".into());
        let (mut launch, lease) = shell_integration::prepare(Launch {
            program: bash.clone(),
            args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
            cwd: Some(directory.0.clone()),
            size: Size {
                rows: 12,
                cols: 100,
            },
        })
        .unwrap();
        // Exercise Darwin's Bash POSIX-shell wait semantics on Linux too. Keep the
        // Rust cleanup lease outside the worker so it cannot mask shell cleanup.
        launch.program = bash;
        launch.args.insert(0, "--posix".into());
        let mut session = Session::spawn_with_environment(
            launch,
            &[
                (
                    "ZMUX_FIXTURE_LEASE_REPORT".into(),
                    report.to_str().unwrap().into(),
                ),
                (
                    "PERL5OPT".into(),
                    "-MZmuxPrivateBootstrapMustNotLoad".into(),
                ),
                ("PERL5LIB".into(), directory.0.to_str().unwrap().into()),
                (
                    "ZMUX_FIXTURE_PERL_LIBRARY".into(),
                    directory.0.to_str().unwrap().into(),
                ),
            ],
        )
        .unwrap();
        let wait = |label: &str, predicate: fn(&str) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if session
                    .snapshot()
                    .is_ok_and(|s| predicate(&s.viewport.contents))
                {
                    break;
                }
                if Instant::now() >= deadline {
                    let observed = session.snapshot().map(|s| {
                        (
                            s.state,
                            s.failure.clone(),
                            s.viewport.contents.chars().take(1800).collect::<String>(),
                        )
                    });
                    panic!("PTY lifecycle fixture did not execute {label}: {observed:?}");
                }
                thread::sleep(Duration::from_millis(5));
            }
        };
        session.send_input(b"[ \"$PERL5OPT\" = -MZmuxPrivateBootstrapMustNotLoad ] && [ \"$PERL5LIB\" = \"$ZMUX_FIXTURE_PERL_LIBRARY\" ] || exit 77; stty -echo; printf '%s' \"$ZMUX_SHELL_INTEGRATION_DIRECTORY\" > \"$ZMUX_FIXTURE_LEASE_REPORT\"; printf '\\033[2J\\033[HHANGUP_READY'\n").unwrap();
        // Cleared screen prefix excludes echoed command text and proves the preceding
        // lease-path write completed before the controlled HUP.
        wait("ready", |text| text.starts_with("HANGUP_READY"));
        let root = PathBuf::from(fs::read_to_string(report).unwrap());
        assert!(root.join("plugin/hooks/hooks.json").is_file());
        // The notification relay must preserve ordinary Ctrl-C handling and PTY input.
        session.send_input(b"sleep 30\n").unwrap();
        thread::sleep(Duration::from_millis(100));
        session
            .send_input(b"\x03printf '\\nINTERRUPT_RECOVERED\\n'\n")
            .unwrap();
        wait("Ctrl-C recovery", |text| {
            text.contains("INTERRUPT_RECOVERED")
        });
        let pid = nix::unistd::Pid::from_raw(session.process_id() as i32);
        let signal = nix::sys::signal::Signal::SIGHUP;
        if group {
            nix::sys::signal::killpg(pid, signal)
        } else {
            nix::sys::signal::kill(pid, signal)
        }
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while root.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let cleaned = !root.exists();
        session.shutdown(Duration::from_secs(3)).unwrap();
        assert!(lease.is_some());
        assert!(
            cleaned,
            "PTY group hangup deferred hook cleanup while its foreground shell was alive"
        );
    }
}

#[test]
fn opencode_fifo_rejects_frames_larger_than_the_reported_atomic_bound() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = Directory::new();
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).unwrap();
    let events = directory.0.join("agent-events");
    let loss = directory.0.join("agent-events-loss");
    assert!(
        Command::new("mkfifo")
            .args(["-m", "600"])
            .args([&events, &loss])
            .status()
            .unwrap()
            .success()
    );
    let mut event_reader = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(&events)
        .unwrap();
    let mut loss_reader = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(&loss)
        .unwrap();
    let generation = directory.0.join("agent-lifecycle-generation");
    fs::write(&generation, format!("{}\n", uuid::Uuid::new_v4())).unwrap();
    fs::set_permissions(&generation, fs::Permissions::from_mode(0o600)).unwrap();
    let bin = directory.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let getconf = bin.join("getconf");
    fs::write(&getconf, "#!/bin/sh\nprintf '512\\n'\n").unwrap();
    fs::set_permissions(&getconf, fs::Permissions::from_mode(0o700)).unwrap();
    let plugin = directory.0.join("plugin.mjs");
    fs::write(
        &plugin,
        include_str!("../src/shell/provider-opencode-plugin.js"),
    )
    .unwrap();
    let host = directory.0.join("host.mjs");
    fs::write(
        &host,
        include_str!("fixtures/provider-opencode-plugin-host.mjs"),
    )
    .unwrap();
    let root_session = format!("session:{}", "x".repeat(247));
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut child = PluginHostChild(
        Command::new("node")
            .args([
                host.as_os_str(),
                plugin.as_os_str(),
                std::ffi::OsStr::new("unused"),
                std::ffi::OsStr::new(&root_session),
                std::ffi::OsStr::new("remote"),
            ])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env_remove("ZMUX_PROVIDER_EVENT_SOCKET")
            .env("ZMUX_SHELL_INTEGRATION_DIRECTORY", &directory.0)
            .env(
                "ZMUX_AGENT_LIFECYCLE_NONCE",
                uuid::Uuid::new_v4().simple().to_string(),
            )
            .env("ZMUX_OPENCODE_EXPECTED_SESSION_ID", &root_session)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = BufReader::new(child.0.stdout.take().unwrap());
    let (sender, lines) = mpsc::sync_channel(4);
    let reader = thread::spawn(move || {
        for line in stdout.lines().take(8) {
            if sender.try_send(line).is_err() {
                break;
            }
        }
    });
    assert_eq!(
        plugin_host_line(&lines, "atomic FIFO plugin setup"),
        "{\"ready\":true}"
    );
    writeln!(child.0.stdin.as_mut().unwrap(), "{{\"op\":\"created\"}}").unwrap();
    // The initial no-turn baseline fits 512 bytes. Adding the real prompt's
    // turn identity makes the following observation exceed that host limit.
    let baseline_deadline = Instant::now() + Duration::from_secs(3);
    let mut baseline = Vec::new();
    let mut bytes = [0; 4096];
    while !baseline.ends_with(b"\n") {
        match event_reader.read(&mut bytes) {
            Ok(count) => baseline.extend_from_slice(&bytes[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("read initial atomic baseline: {error}"),
        }
        assert!(
            baseline.len() <= 512,
            "initial baseline should fit the host limit"
        );
        assert!(
            Instant::now() < baseline_deadline,
            "no initial FIFO baseline"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let baseline: serde_json::Value = serde_json::from_slice(&baseline).unwrap();
    assert_eq!(baseline["provider_observation"]["session_id"], root_session);
    assert_eq!(baseline["provider_observation"]["state"], "idle");
    writeln!(child.0.stdin.as_mut().unwrap(), "{{\"op\":\"start\"}}").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match loss_reader.read(&mut bytes) {
            Ok(count) if count > 0 => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("read bounded provider loss notice: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "oversized atomic FIFO frame was not rejected"
        );
        thread::sleep(Duration::from_millis(5));
    }
    match event_reader.read(&mut bytes) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        result => panic!("an oversized frame reached the shared FIFO: {result:?}"),
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    reader.join().unwrap();
}
