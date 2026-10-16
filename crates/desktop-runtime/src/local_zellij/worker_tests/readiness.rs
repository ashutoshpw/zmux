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
    completion::{Capability, CommittedBinding},
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
fn child(role: &str, root: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE_TEST, "--ignored", "--nocapture"])
        .env("ZMUX_MONITOR_PROTOCOL_ROLE", role)
        .env("ZMUX_MONITOR_PROTOCOL_ROOT", root)
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
    if role != "cli" {
        fs::write(root.join(&role), b"held").unwrap();
        wait_file(&root.join("release"));
        return;
    }
    let descriptor: Value =
        serde_json::from_slice(&fs::read(root.join("observer.json")).unwrap()).unwrap();
    let owner: Uuid = serde_json::from_slice(&fs::read(root.join("owner.json")).unwrap()).unwrap();
    let mut workload = child("workload", &root);
    let mut frontend = child("frontend", &root);
    wait_file(&root.join("workload"));
    wait_file(&root.join("frontend"));
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
    frame(
        &mut stream,
        json!({"version":descriptor["version"],"context":descriptor["context"],"nonce":descriptor["nonce"],"sequence":2,"event":"transport_connected","owner_uuid":owner,"frontend":process_identity(frontend.id()).unwrap()}),
    );
    fs::write(root.join("transport-sent"), b"transport only").unwrap();
    wait_file(&root.join("release"));
    drop(stream);
    assert!(workload.wait().unwrap().success());
    assert!(frontend.wait().unwrap().success());
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
    let _completion = UnixListener::bind(root.join("completion.sock")).unwrap();
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
    )
    .unwrap();
    fs::copy(namespace.descriptor(), root.join("observer.json")).unwrap();
    fs::write(root.join("owner.json"), serde_json::to_vec(&owner).unwrap()).unwrap();
    let peer = child("cli", &root);
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
    let (events, _events_rx) = mpsc::sync_channel(4);
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
    let observed = ready_rx.recv_timeout(Duration::from_millis(50));
    // Cleanup runs even for the expected baseline failure, so no child is leaked.
    drop(fixture);
    stopped.store(true, Ordering::Release);
    let outcome = worker.join().unwrap();
    assert!(
        outcome.is_err(),
        "fixture EOF must finish monitor without final evidence"
    );
    assert!(
        matches!(observed, Err(mpsc::RecvTimeoutError::Timeout)),
        "transport progress returned an interactive admission without InputReady"
    );
}
