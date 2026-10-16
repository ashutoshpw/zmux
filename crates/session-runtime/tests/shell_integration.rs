//! Real interactive startup and hook-byte contract; no Claude account is used.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_session_runtime::{Launch, Session, State, shell_integration};
use zmux_terminal_adapter::Size;

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn interactive_startup_keeps_user_configuration_and_notification_opt_out() {
    let path = std::env::temp_dir().join(format!(
        "zmux-shell-contract-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    let directory = Directory(path);
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
