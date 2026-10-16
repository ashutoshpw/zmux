//! Shipped CLI processes own persistence; actual Desktop/PTYS own paste bytes.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use zmux_desktop_runtime::{Desktop, Shell, control::ControlServer};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "zb-{}-{:x}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_zmux"));
        c.args(args).env("HOME", &self.0);
        for key in [
            "ZMUX_WINDOW_ID",
            "ZMUX_WORKSPACE_ID",
            "ZMUX_PANE_ID",
            "ZMUX_SURFACE_ID",
            "ZMUX_TAB_ID",
            "ZMUX_SOCKET_PASSWORD",
            "ZMUX_SOCKET_PASSWORD_FILE",
        ] {
            c.env_remove(key);
        }
        c
    }
    fn run(&self, args: &[&str]) -> Value {
        success(self.command(args).output().unwrap())
    }
    fn input(&self, args: &[&str], bytes: &[u8]) -> Output {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        child.wait_with_output().unwrap()
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn exact_offline_buffers_survive_processes_concurrent_writers_and_bad_inputs() {
    let h = Home::new();
    let text = "  α\n\tZ \n";
    success(h.input(&["set-buffer"], text.as_bytes()));
    h.run(&["set-buffer", "--name", "flags", "--", "--help", "--name"]);
    let mut children = Vec::new();
    for n in 0..12 {
        children.push(
            h.command(&["set-buffer", "--name", &format!("n{n:02}"), "value"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        success(child.wait_with_output().unwrap());
    }
    let listed = h.run(&["list-buffers"]);
    let rows = listed["buffers"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    assert_eq!(rows[0]["name"], "default");
    assert_eq!(rows[0]["size"], text.len());
    let path = h.0.join(".config/zmux/buffers/store.json");
    let original = fs::read(&path).unwrap();
    let saved: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(
        saved["buffers"]["default"].as_str().unwrap().as_bytes(),
        text.as_bytes()
    );
    assert_eq!(saved["buffers"]["flags"], "--help --name");
    for bytes in [vec![], vec![0xff], vec![0], vec![b'x'; 4097]] {
        assert!(!h.input(&["set-buffer"], &bytes).status.success());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::write(&path, b"broken").unwrap();
    assert!(
        !h.command(&["set-buffer", "safe"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&path).unwrap(), b"broken");
    fs::remove_file(&path).unwrap();
    let sentinel = h.0.join("sentinel");
    fs::write(&sentinel, b"unchanged").unwrap();
    symlink(&sentinel, &path).unwrap();
    assert!(
        !h.command(&["set-buffer", "safe"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"unchanged");
    fs::remove_file(&path).unwrap();
    fs::hard_link(&sentinel, &path).unwrap();
    assert!(
        !h.command(&["set-buffer", "safe"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"unchanged");
}
#[test]
fn buffer_paste_reaches_actual_pty_once_with_mode_aware_brackets() {
    for bracketed in [false, true] {
        let h = Home::new();
        let text = " α\n\tZ ";
        let expected = if bracketed {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text.into()
        };
        let script = format!(
            "import os,tty,time\ntty.setraw(0)\nos.write(1,b'\\x1b[?2004hREADY')\nb=b''\nwhile len(b)<{}: b+=os.read(0,4096)\nos.write(1,b'\\r\\nBYTES:'+b.hex().encode()+b':END')\ntime.sleep(10)",
            expected.len()
        );
        let desktop = Arc::new(
            Desktop::new(Shell {
                program: "/usr/bin/env".into(),
                args: vec!["python3".into(), "-c".into(), script],
                cwd: None,
            })
            .unwrap(),
        );
        let socket = h.0.join("c");
        let server = ControlServer::start(&socket, Arc::clone(&desktop)).unwrap();
        // The production publication and screen expose readiness; no fixed startup sleep.
        let mut prefix = vec!["--socket", socket.to_str().unwrap()];
        let deadline = Instant::now() + Duration::from_secs(10);
        let pane = loop {
            let mut args = prefix.clone();
            args.push("list-panes");
            let value = h.run(&args);
            if let Some(pane) = value["panes"]
                .as_array()
                .and_then(|p| p.first())
                .and_then(|p| p["pane_id"].as_u64())
            {
                break pane.to_string();
            }
            assert!(Instant::now() < deadline, "terminal not published: {value}");
            thread::sleep(Duration::from_millis(10));
        };
        let screen = |needle: &str| {
            loop {
                let mut args = prefix.clone();
                args.extend(["read-screen", &pane]);
                let value = h.run(&args);
                if value.to_string().contains(needle) {
                    break;
                }
                assert!(Instant::now() < deadline, "missing {needle}: {value}");
                thread::sleep(Duration::from_millis(10));
            }
        };
        screen("READY");
        success(h.input(&["set-buffer"], text.as_bytes()));
        let mut bad = prefix.clone();
        bad.extend(["paste-buffer", "--pane", "999999"]);
        assert!(!h.command(&bad).output().unwrap().status.success());
        let mut args = prefix.clone();
        args.extend(["paste-buffer", "--pane", &pane]);
        if bracketed {
            args.push("--bracketed");
        }
        use zmux_desktop_runtime::control_policy::{ControlAccess, ControlPolicy};
        server
            .set_policy(ControlPolicy::same_user(ControlAccess::ReadOnly))
            .unwrap();
        let denied = h.command(&args).output().unwrap();
        assert!(!denied.status.success());
        assert!(String::from_utf8_lossy(&denied.stderr).contains("read"));
        server
            .set_policy(ControlPolicy::same_user(ControlAccess::Full))
            .unwrap();
        h.run(&args);
        let hex = expected
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        screen(&format!("BYTES:{hex}:END"));
        prefix.clear();
        server.shutdown(Duration::from_secs(3)).unwrap();
        desktop.shutdown(Duration::from_secs(3)).unwrap();
    }
}
