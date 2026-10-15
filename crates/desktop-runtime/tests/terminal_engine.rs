use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{Desktop, DesktopOptions, Shell, terminal_engine::TerminalEngineConfig};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "zmux-desktop-engine-{}-{nonce:x}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
    fn shell(&self) -> Shell {
        let path = self.0.join("child.py");
        fs::write(
            &path,
            r#"import os,pathlib,select,sys,time,tty
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
os.write(1,b'\x1b[6n')
reply=b''
deadline=time.monotonic()+1
while time.monotonic()<deadline and not reply.endswith(b'R'):
    if select.select([0],[],[],max(0,deadline-time.monotonic()))[0]:reply+=os.read(0,64)
(root/'reply.tmp').write_text(reply.hex());(root/'reply.tmp').replace(root/'reply')
time.sleep(3)
"#,
        )
        .unwrap();
        Shell {
            program: "python3".into(),
            args: vec![
                "-u".into(),
                path.to_string_lossy().into_owned(),
                self.0.to_string_lossy().into_owned(),
            ],
            cwd: None,
        }
    }
    fn reply(&self, desktop: &Desktop) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(reply) = fs::read_to_string(self.0.join("reply")) {
                return reply;
            }
            if let Ok(snapshot) = desktop.snapshot() {
                assert!(snapshot.last_error.is_none(), "{:?}", snapshot.last_error);
            }
            assert!(
                Instant::now() < deadline,
                "configured desktop child did not report actual reply"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn candidate() -> TerminalEngineConfig {
    TerminalEngineConfig::Ghostty {
        engine_bytes: 16 * 1024 * 1024,
        general_bytes: 16 * 1024 * 1024,
        page_bytes: 16 * 1024 * 1024,
        reply_timeout_ms: 1000,
    }
}

#[test]
fn configured_desktop_child_observes_explicit_engine_and_omission_keeps_vt100() {
    let mut cases = vec![(TerminalEngineConfig::default(), "")];
    #[cfg(feature = "ghostty")]
    cases.push((candidate(), "1b5b313b3152")); // Literal child-observed ESC[1;1R.
    for (terminal_engine, expected) in cases.drain(..) {
        let fixture = Fixture::new();
        let desktop = Desktop::with_options(
            fixture.shell(),
            DesktopOptions {
                terminal_engine,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fixture.reply(&desktop), expected);
        desktop.shutdown(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn invalid_or_unavailable_explicit_engine_fails_before_any_child_launch() {
    let fixture = Fixture::new();
    let mut cases = vec![TerminalEngineConfig::Ghostty {
        engine_bytes: 0,
        general_bytes: 0,
        page_bytes: 0,
        reply_timeout_ms: 1000,
    }];
    #[cfg(not(feature = "ghostty"))]
    cases.push(candidate());
    for terminal_engine in cases.drain(..) {
        let error = Desktop::with_options(
            fixture.shell(),
            DesktopOptions {
                terminal_engine,
                ..Default::default()
            },
        )
        .err()
        .expect("invalid engine configuration launched a desktop");
        assert!(error.to_string().contains("Ghostty"), "{error}");
        assert!(!fixture.0.join("reply").exists());
    }
}
