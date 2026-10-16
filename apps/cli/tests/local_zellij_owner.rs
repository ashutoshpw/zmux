#![cfg(feature = "local-zellij-conformance")]
//! Explicit host gate: actual installed/pinned Zellij through the production CLI.
use filedescriptor::FileDescriptor;
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::DirBuilderExt},
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::child::{ExitObservation, observe_child, process_identity};

#[path = "local_zellij_owner/admitted.rs"]
mod admitted;
#[path = "local_zellij_owner/native.rs"]
mod native;
#[path = "local_zellij_owner/observer.rs"]
mod observer;
#[path = "local_zellij_owner/prebound.rs"]
mod prebound;

// Compare fresh tcgetattr snapshots. Darwin keeps PENDIN as kernel input state
// after noncanonical -> canonical TCSANOW, even when the saved flags omit it.
// Mask only that state in local copies, including nix's private raw snapshot;
// all configurable flags, control characters and speeds still compare exactly.
fn terminal_configuration(settings: nix::sys::termios::Termios) -> nix::sys::termios::Termios {
    #[cfg(target_os = "macos")]
    {
        let mut raw: nix::libc::termios = settings.into();
        raw.c_lflag &= !nix::libc::PENDIN;
        raw.into()
    }
    #[cfg(not(target_os = "macos"))]
    {
        settings
    }
}

#[test]
fn canonical_restore_preserves_queued_private_pty_input_exactly_once() {
    use nix::{
        fcntl::{FcntlArg, OFlag, fcntl},
        poll::{PollFd, PollFlags, poll},
        pty::openpty,
        sys::termios::{LocalFlags, SetArg, cfmakeraw, tcgetattr, tcsetattr},
        unistd::{read, write},
    };
    use std::os::fd::AsFd;

    // No subprocess, user terminal or adopted session participates in this
    // platform probe. Do not poll/read before recording the immediate snapshot:
    // Darwin can process pending input during a readiness query.
    let pair = openpty(None, None).unwrap();
    let before = tcgetattr(&pair.slave).unwrap();
    assert!(before.local_flags.contains(LocalFlags::ICANON));
    #[cfg(target_os = "macos")]
    assert!(!before.local_flags.contains(LocalFlags::PENDIN));
    let mut raw = before.clone();
    cfmakeraw(&mut raw);
    tcsetattr(&pair.slave, SetArg::TCSANOW, &raw).unwrap();
    let queued = b"queued-before-restore";
    assert_eq!(write(&pair.master, queued).unwrap(), queued.len());
    tcsetattr(&pair.slave, SetArg::TCSANOW, &before).unwrap();
    let observed = tcgetattr(&pair.slave).unwrap();
    #[cfg(target_os = "macos")]
    assert_eq!(
        observed.local_flags.bits() ^ before.local_flags.bits(),
        nix::libc::PENDIN,
        "Darwin canonical restore did not exhibit the investigated PENDIN state"
    );
    assert_eq!(
        terminal_configuration(observed.clone()),
        terminal_configuration(before.clone()),
        "TCSANOW did not restore every configurable terminal setting"
    );

    let flags =
        OFlag::from_bits_truncate(fcntl(pair.slave.as_raw_fd(), FcntlArg::F_GETFL).unwrap());
    fcntl(
        pair.slave.as_raw_fd(),
        FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK),
    )
    .unwrap();
    assert_eq!(write(&pair.master, b"\n").unwrap(), 1);
    let mut pending = [PollFd::new(pair.slave.as_fd(), PollFlags::POLLIN)];
    assert_eq!(poll(&mut pending, 2000_u16).unwrap(), 1);
    assert_eq!(pending[0].revents(), Some(PollFlags::POLLIN));
    let mut bytes = [0; 128];
    let count = read(pair.slave.as_raw_fd(), &mut bytes).unwrap();
    assert_eq!(&bytes[..count], b"queued-before-restore\n");
    assert_eq!(
        poll(&mut pending, 200_u16).unwrap(),
        0,
        "canonical input was replayed after its one complete delivery"
    );
    let after_read = tcgetattr(&pair.slave).unwrap();
    assert_eq!(
        terminal_configuration(after_read.clone()),
        terminal_configuration(before.clone())
    );
    #[cfg(target_os = "macos")]
    assert!(!after_read.local_flags.contains(LocalFlags::PENDIN));
    let host = Command::new("uname").arg("-a").output().unwrap();
    assert!(host.status.success());
    #[cfg(target_os = "macos")]
    let kernel = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.version"])
        .output()
        .unwrap();
    #[cfg(not(target_os = "macos"))]
    let kernel = Command::new("uname").arg("-v").output().unwrap();
    assert!(kernel.status.success());
    println!(
        "R03 private PTY restore receipt: {}",
        serde_json::json!({
            "host": String::from_utf8(host.stdout).unwrap().trim(),
            "kernel_version": String::from_utf8(kernel.stdout).unwrap().trim(),
            "before": format!("{before:?}"),
            "immediate_restore": format!("{observed:?}"),
            "after_read": format!("{after_read:?}"),
            "local_flags_delta": observed.local_flags.bits() ^ before.local_flags.bits(),
            "restore": "TCSANOW",
            "queued_input": "queued-before-restore",
            "received_input": String::from_utf8(bytes[..count].to_vec()).unwrap(),
            "received_bytes": count,
            "no_additional_input_ms": 200,
            "exactly_once": true
        })
    );
}

struct MasterFd<'a>(&'a dyn MasterPty);
impl AsRawFd for MasterFd<'_> {
    fn as_raw_fd(&self) -> i32 {
        self.0.as_raw_fd().expect("native PTY descriptor")
    }
}
struct Client {
    pair: portable_pty::PtyPair,
    io: FileDescriptor,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    output: Vec<u8>,
    before: nix::sys::termios::Termios,
    reaped: bool,
}
impl Client {
    fn new(fixture: &Fixture, args: &[&str]) -> Self {
        Self::with_environment(fixture, args, &[])
    }
    fn with_environment(
        fixture: &Fixture,
        args: &[&str],
        environment: &[(&str, std::ffi::OsString)],
    ) -> Self {
        let pair = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let before = pair.master.get_termios().expect("actual PTY termios");
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_zmux"));
        command.args(args);
        for (key, value) in fixture.environment() {
            command.env(key, value);
        }
        for (key, value) in environment {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        let mut io = FileDescriptor::dup(&MasterFd(&*pair.master)).unwrap();
        io.set_non_blocking(true).unwrap();
        Self {
            pair,
            io,
            child,
            output: Vec::new(),
            before,
            reaped: false,
        }
    }
    fn pid(&self) -> u32 {
        self.child.process_id().unwrap()
    }
    fn capture(&mut self, bytes: &[u8]) {
        // Keep the bounded tail so final diagnostics survive initial rendering.
        let excess = (self.output.len() + bytes.len()).saturating_sub(65536);
        self.output.drain(..excess);
        self.output.extend_from_slice(bytes);
    }
    fn drain_final_output(&mut self) -> Option<std::io::Error> {
        let mut bytes = [0; 16384];
        for _ in 0..32 {
            match self.io.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => self.capture(&bytes[..count]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.raw_os_error() == Some(nix::errno::Errno::EIO as i32) =>
                {
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                Err(error) => return Some(error),
            }
        }
        None
    }
    fn pump(&mut self, replies: bool) {
        let mut bytes = [0; 16384];
        match self.io.read(&mut bytes) {
            Ok(count) if count > 0 => {
                self.capture(&bytes[..count]);
                if replies {
                    for (query, answer) in [
                        (b"\x1b[c".as_slice(), b"\x1b[?1;2c".as_slice()),
                        (b"\x1b[6n".as_slice(), b"\x1b[1;1R".as_slice()),
                    ] {
                        if bytes[..count]
                            .windows(query.len())
                            .any(|chunk| chunk == query)
                        {
                            self.send(answer);
                        }
                    }
                }
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(error) => panic!("actual PTY read failed: {error}"),
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        self.io.write_all(bytes).unwrap();
    }
    #[track_caller]
    fn probe(&mut self, bytes: &[u8], mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut next = Instant::now();
        while !predicate() {
            self.pump(true);
            let status = observe_child(self.pid(), false).unwrap();
            if let Some(status) = status {
                // Retain the same bounded PTY transcript used by until/finish.
                // Drain queued final diagnostics without answering further queries.
                let read_error = self.drain_final_output();
                panic!(
                    "CLI ended {status:?} while awaiting input proof; final_pty_read_error={read_error:?}: {}",
                    String::from_utf8_lossy(&self.output)
                );
            }
            assert!(
                Instant::now() < deadline,
                "actual workload input proof deadline: {}",
                String::from_utf8_lossy(&self.output)
            );
            if Instant::now() >= next {
                self.send(bytes);
                next = Instant::now() + Duration::from_millis(200);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[track_caller]
    fn prove_input(&mut self, path: &std::path::Path, mut ready: impl FnMut() -> bool) {
        let token = format!("proof-{}", uuid::Uuid::new_v4().simple());
        let request = format!("{token}\r");
        self.probe(request.as_bytes(), || {
            fs::read_to_string(path).is_ok_and(|text| text == token) && ready()
        });
    }
    fn until(&mut self, mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !predicate() {
            self.pump(true);
            assert!(
                observe_child(self.pid(), false).unwrap().is_none(),
                "CLI ended: {}",
                String::from_utf8_lossy(&self.output)
            );
            assert!(
                Instant::now() < deadline,
                "CLI deadline: {}",
                String::from_utf8_lossy(&self.output)
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn rendered(&mut self, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !String::from_utf8_lossy(&self.output).contains(marker) {
            self.pump(true);
            assert!(
                observe_child(self.pid(), false).unwrap().is_none(),
                "CLI ended before rendered marker: {}",
                String::from_utf8_lossy(&self.output)
            );
            assert!(
                Instant::now() < deadline,
                "actual render deadline: {}",
                String::from_utf8_lossy(&self.output)
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn finish(&mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(8);
        let outcome = loop {
            self.pump(false);
            if let Some(status) = observe_child(self.pid(), false).unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "CLI exit deadline");
            thread::sleep(Duration::from_millis(5));
        };
        for _ in 0..32 {
            self.pump(false);
        }
        assert_eq!(
            terminal_configuration(self.pair.master.get_termios().unwrap()),
            terminal_configuration(self.before.clone()),
            "foreground CLI did not restore actual terminal settings"
        );
        self.child.wait().unwrap();
        self.reaped = true;
        match outcome {
            ExitObservation::Exited(code) => code,
            other => panic!(
                "unclean CLI exit {other:?}: {}",
                String::from_utf8_lossy(&self.output)
            ),
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    temporary: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("r03-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        // Only spawned public CLI processes receive this deliberately long
        // TMPDIR. Their Unix socket namespace must use the short runtime base.
        let temporary =
            root.join("long-private-temporary-directory-for-unix-domain-socket-regression");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&temporary)
            .unwrap();
        Self {
            root: root.canonicalize().unwrap(),
            temporary,
            binary: PathBuf::from(
                std::env::var_os("ZMUX_TEST_ZELLIJ_BIN")
                    .expect("host gate requires checksum-verified ZMUX_TEST_ZELLIJ_BIN"),
            ),
        }
    }
    fn environment(&self) -> Vec<(&str, std::ffi::OsString)> {
        vec![
            ("TMPDIR", self.temporary.clone().into_os_string()),
            (
                "ZMUX_LOCAL_ZELLIJ_ROOT",
                self.root.join("profile").into_os_string(),
            ),
            (
                "ZMUX_LOCAL_ZELLIJ_BIN",
                self.binary.clone().into_os_string(),
            ),
            ("HOME", self.root.clone().into_os_string()),
            ("SHELL", "/bin/bash".into()),
            ("TERM", "xterm-256color".into()),
        ]
    }
    fn records(&self) -> Option<Value> {
        let text = fs::read(self.root.join("profile/registry.json")).ok()?;
        serde_json::from_slice(&text).ok()
    }
    fn output(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_zmux"))
            .args(args)
            .envs(self.environment())
            .output()
            .unwrap()
    }
}

#[test]
fn real_foreground_bootstrap_detach_and_reattach_keep_the_original_owned_workload() {
    let f = Fixture::new();
    let count = f.root.join("startup-count");
    let proof = f.root.join("input-proof");
    let size = f.root.join("size-proof");
    let script = format!(
        "printf x >> '{}'; while IFS= read -r -t 20 line; do case \"$line\" in proof-*) stty size > '{}.pending' && mv '{}.pending' '{}' && printf '%s' \"$line\" > '{}' ;; esac; done",
        count.display(),
        size.display(),
        size.display(),
        size.display(),
        proof.display()
    );
    let mut first = Client::new(
        &f,
        &[
            "local-zellij",
            "start",
            "work",
            "--headless",
            "--command",
            &script,
        ],
    );
    first.until(|| f.records().is_some_and(|r| r[0]["state"] == "bound"));
    let record = f.records().unwrap()[0].clone();
    assert_eq!(
        record["completion"]["execution_contract"],
        "admitted_lifetime_v2"
    );
    assert_eq!(
        record["workload"]["executable"],
        record["supervisor"]["executable"]
    );
    let id = record["id"].as_str().unwrap().to_owned();
    for (field, parent) in [("supervisor", "server"), ("workload", "supervisor")] {
        assert_eq!(record[field]["parent_pid"], record[parent]["pid"]);
        let identity: zmux_session_runtime::child::ProcessIdentity =
            serde_json::from_value(record[field].clone()).unwrap();
        if field == "workload" {
            assert_eq!(
                zmux_session_runtime::child::process_lifetime(identity.pid).unwrap(),
                (&identity).into()
            );
        } else {
            assert_eq!(process_identity(identity.pid).unwrap(), identity);
        }
    }
    first.until(|| count.exists());
    // Actual terminal input must reach the real original workload. This marker
    // is test data; production admission never consults it or shell output.
    for _ in 0..8 {
        first.pump(true);
        thread::sleep(Duration::from_millis(10));
    }
    first.prove_input(&proof, || true);
    let initial_size = fs::read_to_string(&size).unwrap();
    fs::remove_file(&proof).unwrap();
    first
        .pair
        .master
        .resize(PtySize {
            rows: 43,
            cols: 141,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let dimensions = |text: &str| -> Option<(u16, u16)> {
        let mut fields = text.split_whitespace();
        let (rows, cols) = (fields.next()?.parse().ok()?, fields.next()?.parse().ok()?);
        if fields.next().is_some() || rows == 0 || cols == 0 {
            None
        } else {
            Some((rows, cols))
        }
    };
    let initial_dimensions = dimensions(&initial_size).expect("actual initial stty dimensions");
    first.prove_input(&proof, || {
        fs::read_to_string(&size)
            .ok()
            .and_then(|size| dimensions(&size))
            .is_some_and(|(rows, cols)| {
                rows > initial_dimensions.0
                    && cols > initial_dimensions.1
                    && rows <= 43
                    && cols <= 141
            })
    });
    kill(Pid::from_raw(first.pid() as i32), Signal::SIGTSTP).unwrap();
    let stopped_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        first.pump(true);
        if matches!(
            observe_child(first.pid(), true).unwrap(),
            Some(ExitObservation::Stopped(_))
        ) {
            break;
        }
        assert!(
            Instant::now() < stopped_deadline,
            "foreground owner did not stop"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        terminal_configuration(first.pair.master.get_termios().unwrap()),
        terminal_configuration(first.before.clone()),
        "stopped CLI did not return restored terminal"
    );
    kill(Pid::from_raw(first.pid() as i32), Signal::SIGCONT).unwrap();
    fs::remove_file(&proof).unwrap();
    first.prove_input(&proof, || true);
    kill(Pid::from_raw(first.pid() as i32), Signal::SIGTERM).unwrap();
    assert_eq!(
        first.finish(),
        0,
        "{}",
        String::from_utf8_lossy(&first.output)
    );
    let status = f.output(&["local-zellij", "status", "--id", &id]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let current: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(current["state"], "identity_present");
    for field in ["server", "supervisor", "workload"] {
        assert_eq!(current[field], record[field]);
    }
    assert_eq!(fs::read(&count).unwrap(), b"x");
    // These are real namespace changes at the public attach boundary. Each must
    // fail before another client is admitted, preserving the original workload.
    let runtime = std::path::Path::new("/tmp")
        .canonicalize()
        .unwrap()
        .join(format!("z{}", id.replace('-', "")));
    let reject = |expected: &str| {
        let mut denied = Client::new(&f, &["local-zellij", "attach", "--headless", "--id", &id]);
        assert_eq!(denied.finish(), 1);
        assert!(
            String::from_utf8_lossy(&denied.output).contains(expected),
            "wrong namespace rejection: {}",
            String::from_utf8_lossy(&denied.output)
        );
        let identity: zmux_session_runtime::child::ProcessIdentity =
            serde_json::from_value(record["workload"].clone()).unwrap();
        assert_eq!(
            zmux_session_runtime::child::process_lifetime(identity.pid).unwrap(),
            (&identity).into()
        );
    };
    let config = runtime.join("config/config.kdl");
    let original = fs::read(&config).unwrap();
    fs::write(&config, b"session_serialization true\n").unwrap();
    reject("owned non-resurrecting configuration changed");
    fs::write(config, original).unwrap();
    let cache = runtime.join("cache/session-layout.kdl");
    fs::write(&cache, b"resurrection must never be adopted").unwrap();
    reject("unsafe resurrection cache entry");
    fs::remove_file(cache).unwrap();
    // /tmp canonicalizes to /private/tmp on Darwin. Keep the real prefix
    // shadow inside the same portable socket budget as production endpoints.
    let shadow = runtime.join("s/0.43.0").join(format!("zmux-{id}x"));
    assert!(
        shadow.as_os_str().len() < 100,
        "prefix-shadow fixture exceeds portable socket budget"
    );
    let shadow_listener = std::os::unix::net::UnixListener::bind(&shadow).unwrap();
    reject("unexpected backend socket or prefix shadow");
    drop(shadow_listener);
    fs::remove_file(shadow).unwrap();
    let backend = runtime.join("s/0.43.0").join(format!("zmux-{id}"));
    let saved = runtime.join("saved-backend");
    fs::rename(&backend, &saved).unwrap();
    let replacement = std::os::unix::net::UnixListener::bind(&backend).unwrap();
    reject("reserved namespace/socket was replaced");
    drop(replacement);
    fs::remove_file(&backend).unwrap();
    fs::rename(saved, backend).unwrap();
    let moved = runtime.with_extension("held");
    fs::rename(&runtime, &moved).unwrap();
    reject("No such file");
    assert!(!runtime.exists(), "attach recreated a missing runtime");
    fs::rename(moved, runtime).unwrap();
    fs::remove_file(&proof).unwrap();
    let mut second = Client::new(&f, &["local-zellij", "attach", "--headless", "--id", &id]);
    for _ in 0..20 {
        second.pump(true);
        thread::sleep(Duration::from_millis(10));
    }
    second.prove_input(&proof, || true);
    assert_eq!(fs::read(&count).unwrap(), b"x");
    kill(Pid::from_raw(second.pid() as i32), Signal::SIGHUP).unwrap();
    assert_eq!(
        second.finish(),
        0,
        "{}",
        String::from_utf8_lossy(&second.output)
    );
    let current: Value =
        serde_json::from_slice(&f.output(&["local-zellij", "status", "--id", &id]).stdout).unwrap();
    assert_eq!(current["state"], "identity_present");
    for field in ["server", "supervisor", "workload"] {
        assert_eq!(current[field], record[field]);
    }
    fs::remove_file(&proof).unwrap();
    let mut third = Client::new(&f, &["local-zellij", "attach", "--headless", "--id", &id]);
    third.prove_input(&proof, || true);
    kill(Pid::from_raw(third.pid() as i32), Signal::SIGINT).unwrap();
    assert_eq!(
        third.finish(),
        0,
        "{}",
        String::from_utf8_lossy(&third.output)
    );
    let third_output = String::from_utf8_lossy(&third.output);
    let third_receipt: Value = serde_json::from_str(
        third_output
            .split("zmux local-zellij attachment receipt: ")
            .last()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim(),
    )
    .unwrap();
    assert_eq!(
        third_receipt["requested_cancel"], true,
        "SIGINT must establish cancellation before client signal and rejected sends"
    );
    assert_eq!(third_receipt["original_owner_preserved"], true);
    fs::remove_file(&proof).unwrap();
    let mut fourth = Client::new(&f, &["local-zellij", "attach", "--headless", "--id", &id]);
    fourth.prove_input(&proof, || true);
    // End the explicitly finite test workload through its input EOF. No product
    // close path or global/adopted daemon signal is used for cleanup.
    fourth.send(b"\x04");
    let code = fourth.finish();
    if code != 0 {
        assert_eq!(code, 1);
        let output = String::from_utf8_lossy(&fourth.output);
        let receipt = output
            .split("zmux local-zellij attachment receipt: ")
            .last()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .trim();
        let receipt: Value = serde_json::from_str(receipt).expect("typed final transport receipt");
        assert_eq!(
            receipt["owned_client_exit"],
            serde_json::json!({"kind":"exited","value":0})
        );
        assert_eq!(receipt["workers_joined"], true);
        assert_eq!(receipt["requested_cancel"], false);
        assert_eq!(receipt["transport"]["errors"], serde_json::json!([]));
        assert_eq!(receipt["transport"]["pending_to_server_on_cleanup"], 0);
        assert!(
            receipt["transport"]["authenticated_connections"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(receipt["transport"]["rejected_to_server"].as_u64().unwrap() > 0);
        assert!(output.contains("unconfirmed opaque input loss:"));
    }
    eprintln!(
        "R03 real owner receipt: {}",
        serde_json::json!({"profile":f.root,"id":id,"server":record["server"],"supervisor":record["supervisor"],"workload":record["workload"],"startup_count":1,"actual_input":true,"resize":true,"stop_resume":true,"term_detach":true,"hup_detach":true,"reattach":true,"termios_restored":true})
    );
}

#[test]
fn queued_cancellation_during_registry_wait_cannot_authorize_a_workload() {
    use nix::fcntl::{Flock, FlockArg};
    use std::os::unix::fs::OpenOptionsExt;
    let f = Fixture::new();
    let profile = f.root.join("profile");
    fs::DirBuilder::new().mode(0o700).create(&profile).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(profile.join("registry.lock"))
        .unwrap();
    let lock = Flock::lock(lock, FlockArg::LockExclusive).unwrap();
    let proof = f.root.join("must-not-start");
    let script = format!("printf started > '{}'", proof.display());
    let mut client = Client::new(
        &f,
        &[
            "local-zellij",
            "start",
            "cancel",
            "--headless",
            "--command",
            &script,
        ],
    );
    // The held OS lock prevents creation. The required cancellation diagnostic
    // below also proves this signal reached the installed production handler.
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        client.pump(true);
        assert!(observe_child(client.pid(), false).unwrap().is_none());
        thread::sleep(Duration::from_millis(5));
    }
    kill(Pid::from_raw(client.pid() as i32), Signal::SIGTERM).unwrap();
    drop(lock);
    assert_eq!(client.finish(), 1);
    assert!(
        String::from_utf8_lossy(&client.output).contains("foreground startup canceled by signal")
    );
    assert!(!proof.exists());
    let records = f.records().unwrap();
    assert_eq!(records[0]["state"], "uncertain");
    assert!(records[0]["workload"].is_null());
}

#[test]
fn two_actual_frontends_consume_one_retained_completion_and_cleanup_only_its_generation() {
    let fixture = Fixture::new();
    let count = fixture.root.join("finite-multiclient-count");
    let script = format!(
        r#"printf x >> '{}'; printf 'R03_MULTI_READY\n'; IFS= read -r finish; [ "$finish" = 'finish-once' ] || exit 99; exit 7"#,
        count.display()
    );
    let mut first = Client::new(
        &fixture,
        &[
            "local-zellij",
            "start",
            "multi-finite",
            "--headless",
            "--command",
            &script,
        ],
    );
    first.until(|| fixture.records().is_some_and(|r| r[0]["state"] == "bound"));
    first.rendered("R03_MULTI_READY");
    let record: zmux_remote_runtime::local_zellij::Record =
        serde_json::from_value(fixture.records().unwrap()[0].clone()).unwrap();
    let owner = record.id.to_string();
    let mut second = Client::new(
        &fixture,
        &["local-zellij", "attach", "--headless", "--id", &owner],
    );
    second.rendered("R03_MULTI_READY");
    record.verify_live().unwrap();
    assert!(
        zmux_remote_runtime::local_zellij::completion::authenticate(
            &record,
            Instant::now() + Duration::from_secs(1)
        )
        .unwrap()
        .is_none()
    );
    first.send(b"finish-once\r");
    assert_eq!(first.finish(), 7);
    assert_eq!(second.finish(), 7);
    let terminal = zmux_remote_runtime::local_zellij::completion::authenticate(
        &record,
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.workload_exit(),
        zmux_remote_runtime::local_zellij::completion::WorkloadExit::Exited(7)
    );
    assert_eq!(
        process_identity(record.supervisor.as_ref().unwrap().pid).unwrap(),
        *record.supervisor.as_ref().unwrap()
    );
    assert_eq!(
        process_identity(record.server.as_ref().unwrap().pid).unwrap(),
        *record.server.as_ref().unwrap()
    );
    assert_eq!(fs::read(&count).unwrap(), b"x");
    // A third consumer sees the same immutable terminal fact, with no workload
    // restart and identical no-replace local durable consumption bytes.
    let mut third = Client::new(
        &fixture,
        &["local-zellij", "attach", "--headless", "--id", &owner],
    );
    assert_eq!(third.finish(), 7);
    assert_eq!(fs::read(&count).unwrap(), b"x");
    let status = fixture.output(&["local-zellij", "status", "--id", &owner]);
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["state"], "completed_retained");
    assert_eq!(status["completion_authenticated"], true);
    assert_eq!(
        status["terminal_receipt"]["receipt"]["workload_exit"],
        serde_json::json!({"kind":"exited","value":7})
    );
    let generation = terminal.owner_generation().to_string();
    let cleaned = fixture.output(&[
        "local-zellij",
        "cleanup",
        "--id",
        &owner,
        "--generation",
        &generation,
    ]);
    assert!(
        cleaned.status.success(),
        "{}",
        String::from_utf8_lossy(&cleaned.stderr)
    );
    assert!(fixture.records().unwrap().as_array().unwrap().is_empty());
    eprintln!(
        "R03 actual multi-client completion: {}",
        serde_json::json!({"owner_uuid":owner,"owner_generation":generation,"workload_exit":7,"startup_count":1,"concurrent_frontends":2,"duplicate_consumption":true,"live_original_publisher_retained_until_explicit_cleanup":true,"registry_reclaimed":true})
    );
}
