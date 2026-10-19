//! Real interactive startup and hook-byte contract; no Claude account is used.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
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
