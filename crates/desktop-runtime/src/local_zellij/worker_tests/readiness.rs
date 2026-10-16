//! The production monitor's admission boundary, with an explicitly synthetic
//! protocol producer and real retained peers. This is not Zellij/Saved proof.
use super::*;
use crate::local_zellij::{Correlation, Installation, Profile};
use serde_json::{Value, json};
use std::{
    os::unix::fs::DirBuilderExt,
    process::{Command, Stdio},
};
use uuid::Uuid;
use zmux_remote_runtime::local_zellij::{
    NamespaceIdentity,
    completion::{Capability, CommittedBinding, TerminalReceiptV1, WorkloadExit},
};

const FIXTURE_TEST: &str = "local_zellij::worker::tests::readiness::monitor_protocol_peer";
fn inode(path: &Path) -> (u64, u64) {
    let m = fs::symlink_metadata(path).unwrap();
    (m.dev(), m.ino())
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "fixture barrier {}",
            path.display()
        );
        thread::sleep(POLL);
    }
}
fn frame(stream: &mut UnixStream, value: Value) {
    let body = serde_json::to_vec(&value).unwrap();
    stream
        .write_all(&(body.len() as u32).to_le_bytes())
        .unwrap();
    stream.write_all(&body).unwrap();
}
fn child(role: &str, root: &Path, mode: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE_TEST, "--ignored", "--nocapture"])
        .env("ZMUX_MONITOR_PROTOCOL_ROLE", role)
        .env("ZMUX_MONITOR_PROTOCOL_ROOT", root)
        .env("ZMUX_MONITOR_PROTOCOL_MODE", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}
#[test]
#[ignore = "owned subprocess protocol fixture; invoked only by its parent test"]
fn monitor_protocol_peer() {
    let root = PathBuf::from(std::env::var_os("ZMUX_MONITOR_PROTOCOL_ROOT").unwrap());
    let role = std::env::var("ZMUX_MONITOR_PROTOCOL_ROLE").unwrap();
    let mode = std::env::var("ZMUX_MONITOR_PROTOCOL_MODE").unwrap();
    if role != "cli" {
        fs::write(root.join(&role), b"held").unwrap();
        if role == "finite-workload" {
            wait_file(&root.join("finish-workload"));
            std::process::exit(7);
        }
        if role == "finite-frontend" {
            wait_file(&root.join("finish-frontend"));
            std::process::exit(0);
        }
        wait_file(&root.join("release"));
        return;
    }
    let descriptor: Value =
        serde_json::from_slice(&fs::read(root.join("observer.json")).unwrap()).unwrap();
    let owner: Uuid = serde_json::from_slice(&fs::read(root.join("owner.json")).unwrap()).unwrap();
    let completed = mode != "transport";
    let workload_role = if completed {
        "finite-workload"
    } else {
        "workload"
    };
    let mut workload = child(workload_role, &root, &mode);
    let frontend_role = if mode == "departed_frontend" {
        "finite-frontend"
    } else {
        "frontend"
    };
    let mut frontend = child(frontend_role, &root, &mode);
    wait_file(&root.join(workload_role));
    wait_file(&root.join(frontend_role));
    let mut frontend_identity = process_identity(frontend.id()).unwrap();
    let completion_listener = completed.then(|| {
        let listener = UnixListener::bind(root.join("completion.sock")).unwrap();
        fs::set_permissions(
            root.join("completion.sock"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    });
    let own = process_identity(std::process::id()).unwrap();
    let binding = CommittedBinding {
        version: 1,
        owner_uuid: owner,
        owner_generation: Uuid::new_v4(),
        namespace: NamespaceIdentity {
            root: inode(&root),
            socket: inode(&root.join("s/0.43.0").join(format!("zmux-{owner}"))),
        },
        completion_socket: inode(&root.join("completion.sock")),
        server: process_identity(own.parent_pid).unwrap(),
        supervisor: own,
        workload: process_identity(workload.id()).unwrap(),
    };
    let record = Record {
        id: owner,
        name: "monitor-contract".into(),
        state: "bound".into(),
        server: Some(binding.server.clone()),
        supervisor: Some(binding.supervisor.clone()),
        workload: Some(binding.workload.clone()),
        namespace: Some(binding.namespace.clone()),
        completion: Some(Capability {
            execution_contract: None,
            admission: None,
            owner_generation: binding.owner_generation,
            completion_socket: binding.completion_socket,
            binding_digest: binding.digest().unwrap(),
        }),
    };
    record.verify_live().unwrap();
    let mut stream = UnixStream::connect(descriptor["socket"].as_str().unwrap()).unwrap();
    frame(
        &mut stream,
        json!({"version":descriptor["version"],"context":descriptor["context"],"nonce":descriptor["nonce"],"sequence":1,"event":"owner_bound","record":record}),
    );
    let publisher = completion_listener.map(|listener| {
        fs::write(root.join("finish-workload"), b"actual finite Child exit").unwrap();
        let status = workload.wait().unwrap();
        assert_eq!(status.code(), Some(7));
        let receipt = TerminalReceiptV1 {
            version: 1,
            domain: zmux_remote_runtime::local_zellij::completion::DOMAIN.into(),
            binding: binding.clone(),
            binding_digest: binding.digest().unwrap(),
            terminal_sequence: 1,
            workload_exit: WorkloadExit::Exited(status.code().unwrap()),
            child_reaped: true,
        };
        receipt.validate().unwrap();
        fs::write(root.join("terminal.json"), serde_json::to_vec(&receipt).unwrap()).unwrap();
        let root = root.clone();
        thread::spawn(move || {
            // Synthetic control framing, using the actual retained supervisor peer
            // and its independently waited Child status. This is not Saved proof.
            while !root.join("release").exists() {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(POLL);
                        continue;
                    }
                    Err(error) => panic!("completion fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut header = [0; 4];
                stream.read_exact(&mut header).unwrap();
                let size = u32::from_le_bytes(header) as usize;
                assert!((1..=16384).contains(&size));
                let mut bytes = vec![0; size];
                stream.read_exact(&mut bytes).unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(request["version"], 1);
                assert_eq!(request["owner_uuid"], json!(receipt.binding.owner_uuid));
                assert_eq!(request["owner_generation"], json!(receipt.binding.owner_generation));
                assert_eq!(request["binding_digest"], receipt.binding_digest);
                assert_eq!(request["operation"], json!({"operation":"query"}));
                frame(&mut stream, json!({"version":1,"challenge":request["challenge"],"binding_digest":receipt.binding_digest,"receipt":receipt,"consumed":false,"error":null,"retiring":false}));
            }
        })
    });
    if mode == "departed_frontend" {
        fs::write(root.join("finish-frontend"), b"actual frontend exit").unwrap();
        assert!(frontend.wait().unwrap().success());
    }
    if mode == "completed_bad_frontend" {
        frontend_identity.parent_pid += 1;
    } else if mode == "completed_changed_frontend" {
        frontend_identity.start[0] += 1;
    }
    frame(
        &mut stream,
        json!({"version":descriptor["version"],"context":descriptor["context"],"nonce":descriptor["nonce"],"sequence":2,"event":"transport_connected","owner_uuid":owner,"frontend":frontend_identity,"attachment_id":Uuid::new_v4()}),
    );
    if completed {
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(root.join("terminal.json")).unwrap()).unwrap();
        if mode == "completed_wrong_terminal" {
            receipt["workload_exit"]["value"] = json!(0);
        }
        frame(
            &mut stream,
            json!({"version":descriptor["version"],"context":descriptor["context"],"nonce":descriptor["nonce"],"sequence":3,"event":"workload_finished","owner_uuid":owner,"receipt":receipt}),
        );
    }
    fs::write(root.join("transport-sent"), b"transport only").unwrap();
    wait_file(&root.join("release"));
    drop(stream);
    if !completed {
        assert!(workload.wait().unwrap().success());
    }
    assert!(frontend.wait().unwrap().success());
    if let Some(publisher) = publisher {
        publisher.join().unwrap();
    }
}
struct Fixture {
    root: PathBuf,
    peer: Child,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::write(self.root.join("release"), b"cleanup owned fixture").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if self.peer.try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(POLL);
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn transport_connected_without_input_ready_cannot_return_interactive_admission() {
    monitor_case("transport");
}
#[test]
fn terminal_after_queued_transport_retains_only_authenticated_completion() {
    for mode in [
        "completed",
        "completed_bad_frontend",
        "completed_changed_frontend",
        "completed_wrong_terminal",
        "departed_frontend",
    ] {
        monitor_case(mode);
    }
}
fn monitor_case(mode: &str) {
    let owner = Uuid::new_v4();
    let root = wire::runtime_base()
        .unwrap()
        .join(format!("z{}", owner.simple()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    for name in ["s", "s/0.43.0", "cache", "config"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(name))
            .unwrap();
    }
    fs::write(root.join("config/config.kdl"),b"session_serialization false\non_force_close \"detach\"\nshow_release_notes false\nshow_startup_tips false\n").unwrap();
    fs::set_permissions(
        root.join("config/config.kdl"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let _backend = UnixListener::bind(root.join("s/0.43.0").join(format!("zmux-{owner}"))).unwrap();
    fs::set_permissions(
        root.join("s/0.43.0").join(format!("zmux-{owner}")),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let _completion =
        (mode == "transport").then(|| UnixListener::bind(root.join("completion.sock")).unwrap());
    let context = Correlation {
        installation_epoch: Uuid::new_v4(),
        profile_id: Uuid::new_v4(),
        operation_id: 1,
        identity_namespace: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        surface_id: Uuid::new_v4(),
        launch_generation: 1,
    };
    let (namespace, listener) = Namespace::create(
        context.clone(),
        process_identity(std::process::id()).unwrap(),
        Instant::now() + Duration::from_secs(3),
    )
    .unwrap();
    fs::copy(namespace.descriptor(), root.join("observer.json")).unwrap();
    fs::write(root.join("owner.json"), serde_json::to_vec(&owner).unwrap()).unwrap();
    let peer = child("cli", &root, mode);
    let fixture = Fixture {
        root: root.clone(),
        peer,
    };
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let request = LaunchRequest {
        correlation: context.clone(),
        installation: Installation {
            epoch: context.installation_epoch,
            cli: executable.clone(),
            zellij: executable.clone(),
        },
        profile: Profile {
            id: context.profile_id,
            root: root.clone(),
        },
        operation: LaunchOperation::Attach {
            identity: zmux_workspace_core::LocalZellijIdentity {
                profile_id: context.profile_id,
                owner_uuid: owner,
            },
        },
        startup_deadline: Instant::now() + Duration::from_secs(3),
        cancellation: Arc::new(AtomicBool::new(false)),
        engine: None,
        environment: Vec::new(),
    };
    let (identity_tx, identities) = mpsc::sync_channel(1);
    identity_tx
        .send(process_identity(fixture.peer.id()).unwrap())
        .unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (events, events_rx) = mpsc::sync_channel(4);
    let (_auth_tx, authorization) = mpsc::sync_channel(1);
    let stopped = Arc::new(AtomicBool::new(false));
    let monitor = Monitor {
        request,
        validated: Validated {
            cli: executable.clone(),
            zellij: executable,
            profile: root.clone(),
        },
        listener,
        view: namespace.view.clone(),
        nonce: namespace.nonce.clone(),
        stopped: stopped.clone(),
        identities,
        events,
        authorization,
        ready: ready_tx,
        state: Arc::new(Mutex::new(Observation::default())),
    };
    let worker = thread::spawn(move || monitor.run());
    wait_file(&root.join("transport-sent"));
    let observed = ready_rx.recv_timeout(if mode == "transport" {
        Duration::from_millis(50)
    } else {
        Duration::from_secs(1)
    });
    let progress = events_rx.recv_timeout(Duration::from_secs(1));
    // Cleanup runs even for the expected baseline failure, so no child is leaked.
    drop(fixture);
    stopped.store(true, Ordering::Release);
    let outcome = worker.join().unwrap();
    assert!(
        outcome.is_err(),
        "fixture EOF must finish monitor without final evidence"
    );
    if mode == "completed" || mode == "departed_frontend" {
        let ready = observed
            .unwrap_or_else(|error| {
                panic!(
                    "authenticated terminal admission unavailable: {error:?}; monitor={outcome:?}"
                )
            })
            .expect("terminal admission");
        let ClientAdmission::TerminalObserved { receipt, .. } = ready.admission else {
            panic!("historical transport granted interactive admission");
        };
        assert_eq!(receipt.workload_exit(), WorkloadExit::Exited(7));
        assert!(
            ready.frontend.is_none(),
            "historical transport seeded readiness"
        );
        let Ok(LaunchEvent::WorkloadFinished { receipt: event, .. }) = progress else {
            panic!("correlated terminal event not retained");
        };
        assert_eq!(*receipt, *event);
        return;
    }
    if mode.starts_with("completed_") {
        assert!(observed.is_err(), "invalid frontend granted admission");
        assert!(
            progress.is_err(),
            "invalid frontend published terminal progress"
        );
        let expected = if mode == "completed_wrong_terminal" {
            "differs from historical transport proof"
        } else {
            "frontend"
        };
        assert!(outcome.unwrap_err().to_string().contains(expected));
        return;
    }
    assert!(
        matches!(progress, Ok(LaunchEvent::TransportConnected { .. })),
        "monitor must have validated actual transport progress before negative admission assertion"
    );
    assert!(
        matches!(observed, Err(mpsc::RecvTimeoutError::Timeout)),
        "transport progress returned an interactive admission without InputReady"
    );
}
