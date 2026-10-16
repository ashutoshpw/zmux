//! Real interactive startup and hook-byte contract; no Claude account is used.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_session_runtime::{Launch, Session, State, shell_integration};
use zmux_terminal_adapter::Size;

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
fn posix_group_hangup_cleans_hooks_before_the_foreground_shell_exits() {
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
        &[(
            "ZMUX_FIXTURE_LEASE_REPORT".into(),
            report.to_str().unwrap().into(),
        )],
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
    session.send_input(b"stty -echo; printf '%s' \"$ZMUX_SHELL_INTEGRATION_DIRECTORY\" > \"$ZMUX_FIXTURE_LEASE_REPORT\"; printf '\\033[2J\\033[HHANGUP_READY'\n").unwrap();
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
    nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(session.process_id() as i32),
        nix::sys::signal::Signal::SIGHUP,
    )
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
