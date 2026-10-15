use std::process::Command;

#[test]
fn foreground_zellij_grammar_refuses_unsupported_lifecycle_before_any_owner() {
    let help = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args(["local-zellij", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--headless"));
    for args in [
        vec!["start", "work", "--detached"],
        vec!["attach", "--id", "bad"],
        vec!["attach", "--headless", "--id", "bad"],
        vec!["status", "--id", "bad"],
        vec!["close"],
        vec!["cleanup", "--prune"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("local-zellij")
            .args(args)
            .env("ZMUX_LOCAL_ZELLIJ_ROOT", "/invalid/must-not-be-created")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("No such file"),
            "reached owner setup before grammar refusal"
        );
    }
}

#[test]
fn production_supervisor_rejects_changed_starter_and_unauthorized_spawn() {
    use std::{
        fs,
        io::{Read, Write},
        os::unix::{
            fs::{DirBuilderExt, OpenOptionsExt},
            net::UnixListener,
        },
        time::{Duration, Instant},
    };
    use zmux_session_runtime::child::process_identity;
    // Darwin's per-user TMPDIR can exceed sockaddr_un before this UUID and
    // socket basename are appended. Retain the private fixture under short /tmp.
    let root =
        std::path::Path::new("/tmp").join(format!("r03-auth-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let root = root.canonicalize().unwrap();
    for changed_starter in [true, false] {
        let socket = root.join(if changed_starter {
            "changed.sock"
        } else {
            "denied.sock"
        });
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let proof = root.join("must-not-spawn");
        let mut starter = process_identity(std::process::id()).unwrap();
        if changed_starter {
            starter.start[0] += 1;
        }
        let spec = serde_json::json!({"socket":socket,"starter":starter,"program":std::fs::canonicalize(env!("CARGO_BIN_EXE_zmux")).unwrap(),"nonce":"a".repeat(64),"cwd":root,"shell":"/bin/sh","command":format!("printf started > '{}'", proof.display())});
        let path = root.join(if changed_starter {
            "changed.json"
        } else {
            "denied.json"
        });
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap()
            .write_all(&serde_json::to_vec(&spec).unwrap())
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_zmux"))
            .arg("__local-zellij-supervisor")
            .arg(&path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut peer = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "production supervisor did not reach bootstrap boundary"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("bootstrap listener: {error}"),
            }
        };
        if !changed_starter {
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut length = [0; 4];
            peer.read_exact(&mut length).unwrap();
            assert_eq!(u32::from_le_bytes(length), 66);
            let mut nonce = [0; 66];
            peer.read_exact(&mut nonce).unwrap();
            assert_eq!(
                serde_json::from_slice::<String>(&nonce).unwrap(),
                "a".repeat(64)
            );
            let denied = b"\"not-authorized\"";
            peer.write_all(&(denied.len() as u32).to_le_bytes())
                .unwrap();
            peer.write_all(denied).unwrap();
        }
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("production supervisor rejection exceeded bound");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(!proof.exists());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if changed_starter {
                "bootstrap starter identity changed"
            } else {
                "supervisor was not authorized to start workload"
            }),
            "wrong rejection: {error}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_supervisor_refuses_fifo_descriptor_without_waiting_for_a_writer() {
    use nix::{sys::stat::Mode, unistd::mkfifo};
    use std::{
        fs,
        os::unix::fs::DirBuilderExt,
        time::{Duration, Instant},
    };
    let root = std::env::temp_dir().join(format!("r03-fifo-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let descriptor = root.join("bootstrap.json");
    mkfifo(&descriptor, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .arg("__local-zellij-supervisor")
        .arg(descriptor)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            fs::remove_dir_all(root).unwrap();
            panic!("FIFO descriptor blocked before its type could be rejected");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsafe private file"));
    fs::remove_dir_all(root).unwrap();
}
