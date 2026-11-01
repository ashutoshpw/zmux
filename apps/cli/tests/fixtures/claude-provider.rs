use std::{
    env, fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn hook_command(config: &Path, event: &str) -> String {
    let contents = fs::read_to_string(config).expect("read Claude plugin hook configuration");
    let event_key = format!("\"{event}\":[");
    let event_offset = contents
        .find(&event_key)
        .unwrap_or_else(|| panic!("{event} hook missing from {}", config.display()));
    let event_contents = &contents[event_offset..];
    let command_key = "\"command\":\"";
    let command_start = event_contents
        .find(command_key)
        .unwrap_or_else(|| panic!("{event} hook command missing from {}", config.display()))
        + command_key.len();
    let mut command = String::new();
    let mut escaped = false;
    for character in event_contents[command_start..].chars() {
        if escaped {
            command.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return command;
        } else {
            command.push(character);
        }
    }
    panic!("unterminated {event} hook command in {}", config.display());
}

fn run_notice_hook(config: &Path, event: &str, input: &str) {
    let command = hook_command(config, event);
    let mut child = Command::new("/bin/sh")
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start session notice hook");
    child
        .stdin
        .take()
        .expect("notice hook stdin")
        .write_all(input.as_bytes())
        .expect("write notice hook input");
    let output = child.wait_with_output().expect("wait for notice hook");
    assert!(
        output.status.success(),
        "{event} notice hook failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_lifecycle_hook(config: &Path, event: &str, input: &str) {
    // Lifecycle plugin commands use the fixed `hooks claude event <event>`
    // adapter contract. Read the executable from the published hook config.
    let command = hook_command(config, event);
    let mut child = Command::new(command)
        .args(["hooks", "claude", "event", event])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start production lifecycle hook command");
    child
        .stdin
        .take()
        .expect("lifecycle hook stdin")
        .write_all(input.as_bytes())
        .expect("write lifecycle hook input");
    let output = child.wait_with_output().expect("wait for lifecycle hook");
    assert!(
        output.status.success(),
        "{event} lifecycle hook failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if event == "SessionStart" {
        assert!(
            output.stderr.is_empty(),
            "SessionStart lifecycle admission failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--version")
    {
        println!("2.1.301 (Claude Code)");
        return;
    }

    let report_path = env::var_os("FIXTURE_AUTO_ARGS").expect("arguments report path");
    let called = env::var_os("FIXTURE_AUTO_CALLED").expect("provider started path");
    let release = env::var_os("FIXTURE_AUTO_RELEASE").expect("provider release path");
    let report = arguments
        .iter()
        .map(|argument| format!("<{argument}>\n"))
        .collect::<String>();
    fs::write(report_path, report).unwrap();
    fs::write(called, b"started").unwrap();
    if env::var_os("FIXTURE_AUTO_EMIT_HOOKS").is_some() {
        let plugin_dirs: Vec<_> = arguments
            .windows(2)
            .filter(|pair| pair[0] == "--plugin-dir")
            .map(|pair| Path::new(&pair[1]).to_path_buf())
            .collect();
        assert_eq!(
            plugin_dirs.len(),
            2,
            "expected lifecycle and notice plugins"
        );
        let notice_config = plugin_dirs[1].join("hooks/hooks.json");
        let lifecycle_config = plugin_dirs[0].join("hooks/hooks.json");
        let session_id = arguments
            .windows(2)
            .find(|pair| pair[0] == "--session-id")
            .map(|pair| pair[1].as_str())
            .expect("automatic launch session id");
        run_lifecycle_hook(
            &lifecycle_config,
            "SessionStart",
            &format!(
                "{{\"hook_event_name\":\"SessionStart\",\"session_id\":\"{session_id}\",\"source\":\"startup\"}}"
            ),
        );
        let main_attention = format!(
            "{{\"hook_event_name\":\"Notification\",\"session_id\":\"{session_id}\",\"notification_type\":\"permission_prompt\"}}"
        );
        run_notice_hook(
            &notice_config,
            "Notification",
            &main_attention,
        );
        run_notice_hook(
            &notice_config,
            "Notification",
            &format!(
                "{{\"hook_event_name\":\"Notification\",\"session_id\":\"{session_id}\",\"notification_type\":\"permission_prompt\",\"agent_id\":\"subagent-1\"}}"
            ),
        );
        run_notice_hook(
            &notice_config,
            "Stop",
            &format!("{{\"hook_event_name\":\"Stop\",\"session_id\":\"{session_id}\"}}"),
        );
        run_notice_hook(
            &notice_config,
            "Stop",
            &format!(
                "{{\"hook_event_name\":\"Stop\",\"session_id\":\"{session_id}\",\"agent_id\":\"subagent-1\"}}"
            ),
        );
        run_notice_hook(
            &notice_config,
            "StopFailure",
            &format!("{{\"hook_event_name\":\"StopFailure\",\"session_id\":\"{session_id}\",\"error\":\"fixture\"}}"),
        );
        let env_report =
            env::var_os("FIXTURE_AUTO_ENV_REPORT").expect("provider environment report");
        let root = env::var("ZMUX_SHELL_INTEGRATION_DIRECTORY")
            .expect("private shell root restored to provider");
        let lifecycle_alerts = env::var("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE")
            .expect("lifecycle alert marker restored to provider");
        let cli = env::var("ZMUX_CLI_PATH").expect("bundled CLI route");
        let route = env::var("ZMUX_CLAUDE_EVENT_SOCKET").expect("local event route");
        fs::write(
            env_report,
            format!(
                "root={root}\nlifecycle_alerts={lifecycle_alerts}\ncli={cli}\nroute={route}\nprovider_session={session_id}\nprovider_pid={}\n",
                std::process::id()
            ),
        )
        .unwrap();
        fs::write(
            env::var_os("FIXTURE_AUTO_NO_PROMPT_DONE").expect("no-prompt hook completion marker"),
            b"done",
        )
        .unwrap();
        let tracked_release =
            env::var_os("FIXTURE_AUTO_TRACKED_RELEASE").expect("tracked-hook release marker");
        while !Path::new(&tracked_release).exists() {
            thread::sleep(Duration::from_millis(2));
        }
        // Unreadable input has no validated Notification subtype, and
        // auth_success is a valid but non-attention subtype; neither should
        // fabricate an attention fallback.
        run_notice_hook(&notice_config, "Notification", "{\"hook_event_name\":\"Notification\"");
        run_notice_hook(
            &notice_config,
            "Notification",
            &format!(
                "{{\"hook_event_name\":\"Notification\",\"session_id\":\"{session_id}\",\"notification_type\":\"auth_success\"}}"
            ),
        );
        // The subtype is known even though typed lifecycle admission fails,
        // so keep one visual-only fallback for this permission matcher.
        run_notice_hook(
            &notice_config,
            "Notification",
            "{\"hook_event_name\":\"Notification\",\"notification_type\":\"permission_prompt\"}",
        );
        run_lifecycle_hook(
            &lifecycle_config,
            "UserPromptSubmit",
            &format!(
                "{{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"{session_id}\",\"prompt_id\":\"123e4567-e89b-42d3-a456-426614174000\",\"permission_mode\":\"default\",\"prompt\":\"private prompt text\",\"tool_input\":{{\"command\":\"private tool data\"}}}}"
            ),
        );
        run_notice_hook(
            &notice_config,
            "Notification",
            &format!(
                "{{\"hook_event_name\":\"Notification\",\"session_id\":\"{session_id}\",\"notification_type\":\"permission_prompt\"}}"
            ),
        );
        run_notice_hook(
            &notice_config,
            "Stop",
            &format!("{{\"hook_event_name\":\"Stop\",\"session_id\":\"{session_id}\"}}"),
        );
        run_notice_hook(
            &notice_config,
            "Notification",
            &format!(
                "{{\"hook_event_name\":\"Notification\",\"session_id\":\"{session_id}\",\"notification_type\":\"idle_prompt\"}}"
            ),
        );
        // A delayed Stop can explicitly report that the same turn continued
        // after idle_prompt. Keep it Working without a false finished fallback.
        run_notice_hook(
            &notice_config,
            "Stop",
            &format!(
                "{{\"hook_event_name\":\"Stop\",\"session_id\":\"{session_id}\",\"stop_hook_active\":true}}"
            ),
        );
        fs::write(
            env::var_os("FIXTURE_AUTO_HOOKS_DONE").expect("hook completion marker"),
            b"done",
        )
        .unwrap();
    }
    while !Path::new(&release).exists() {
        thread::sleep(Duration::from_millis(10));
    }
    std::process::exit(23);
}
