//! Actual commands end immediately, without a bound/rendered/input release gate.
use super::*;

fn completed_once(code: i32, startup: bool) {
    let fixture = Fixture::new();
    let counter = fixture.root.join("executed-once");
    let body = fixture.root.join("command-body");
    let script = format!("printf x >> '{}'; exit {code}", counter.display());
    let mut environment = Vec::new();
    environment.push(("SHELL", "/bin/sh".into()));
    let command = if startup {
        let startup_dir = fixture.root.join("startup");
        fs::create_dir(&startup_dir).unwrap();
        fs::write(startup_dir.join(".zshenv"), &script).unwrap();
        let shell = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/usr/bin/zsh"
        };
        assert!(
            std::path::Path::new(shell).is_file(),
            "startup-file gate requires actual zsh"
        );
        environment.push(("SHELL", shell.into()));
        environment.push(("ZDOTDIR", startup_dir.into_os_string()));
        format!("printf should-not-run > '{}'", body.display())
    } else {
        script
    };
    let mut client = Client::with_environment(
        &fixture,
        &[
            "local-zellij",
            "start",
            "immediate",
            "--headless",
            "--command",
            &command,
        ],
        &environment,
    );
    let observed = client.finish();
    let records = fixture.records().expect("actual owner reservation");
    eprintln!(
        "Immediate workload receipt: {}",
        serde_json::json!({
        "expected_exit":code,"observed_exit":observed,"startup_file":startup,"records":records,
        "output":String::from_utf8_lossy(&client.output)})
    );
    assert_eq!(
        fs::read(&counter).unwrap(),
        b"x",
        "actual user code must execute once"
    );
    assert!(
        !body.exists(),
        "startup-file exit must prevent command-body execution"
    );
    assert_eq!(
        observed, code,
        "immediate workload terminal status was lost"
    );
    let owner = records[0]["id"].as_str().unwrap();
    let status = fixture.output(&["local-zellij", "status", "--id", owner]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["completion_authenticated"], true, "{status}");
    assert_eq!(status["consumption_confirmed"], true, "{status}");
    let terminal = status["terminal_receipt"]
        .get("receipt")
        .unwrap_or(&status["terminal_receipt"]);
    assert_eq!(
        terminal["workload_exit"],
        serde_json::json!({"kind":"exited","value":code})
    );
    let generation = terminal["binding"]["owner_generation"].as_str().unwrap();
    let cleanup = fixture.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        owner,
        "--generation",
        generation,
    ]);
    assert!(
        cleanup.status.success(),
        "{}",
        String::from_utf8_lossy(&cleanup.stderr)
    );
    assert_eq!(
        fs::read(&counter).unwrap(),
        b"x",
        "inspection/cleanup must never replay command"
    );
}

#[test]
fn immediate_exit_zero_is_bound_and_authenticated_without_input_or_render_wait() {
    completed_once(0, false);
}

#[test]
fn immediate_exit_seven_is_bound_and_authenticated_without_input_or_render_wait() {
    completed_once(7, false);
}

#[test]
fn selected_shell_startup_exit_is_authenticated_without_running_command_body() {
    completed_once(7, true);
}
