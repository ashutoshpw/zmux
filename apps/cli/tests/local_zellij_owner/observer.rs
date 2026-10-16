//! Real CLI protocol admission; synthetic revision acknowledgments here do not
//! establish a native durable persistence checkpoint or GUI integration.
use super::*;
use std::{
    ffi::OsString,
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
};

struct Host {
    root: PathBuf,
    descriptor: PathBuf,
    listener: UnixListener,
    context: Value,
    nonce: String,
}
impl Host {
    fn new() -> Self {
        let root = PathBuf::from("/tmp")
            .canonicalize()
            .unwrap()
            .join(format!("zo{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let socket = root.join("observer.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let context = serde_json::json!({
            "installation_epoch": uuid::Uuid::new_v4(),
            "profile_id": uuid::Uuid::new_v4(),
            "operation_id": 1,
            "identity_namespace": uuid::Uuid::new_v4(),
            "session_id": uuid::Uuid::new_v4(),
            "surface_id": uuid::Uuid::new_v4(),
            "launch_generation": 1
        });
        let nonce = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let descriptor = root.join("launch.json");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&descriptor)
            .unwrap();
        file.write_all(
            &serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "socket": socket,
                "launcher": process_identity(std::process::id()).unwrap(),
                "context": context,
                "nonce": nonce
            }))
            .unwrap(),
        )
        .unwrap();
        file.sync_all().unwrap();
        Self {
            root,
            descriptor,
            listener,
            context,
            nonce,
        }
    }
    fn environment(&self) -> [(&str, OsString); 1] {
        [(
            "ZMUX_LOCAL_ZELLIJ_OBSERVER",
            self.descriptor.clone().into_os_string(),
        )]
    }
    fn accept(&self, client: &mut Client, must_not_start: &std::path::Path) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(
                !must_not_start.exists(),
                "workload was authorized before observer protocol acknowledgment"
            );
            match self.listener.accept() {
                Ok((stream, _)) => {
                    #[cfg(target_os = "linux")]
                    let pid = nix::sys::socket::getsockopt(
                        &stream,
                        nix::sys::socket::sockopt::PeerCredentials,
                    )
                    .unwrap()
                    .pid();
                    #[cfg(target_os = "macos")]
                    let pid = nix::sys::socket::getsockopt(
                        &stream,
                        nix::sys::socket::sockopt::LocalPeerPid,
                    )
                    .unwrap();
                    assert_eq!(pid as u32, client.pid());
                    // The retained spawned CLI fixes this kernel PID even when
                    // an intentionally rejected handshake exits before /proc or
                    // proc_pidpath can be sampled. Positive admission below also
                    // samples its full live identity before acknowledging.
                    stream.set_nonblocking(true).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => panic!("observer listener: {error}"),
            }
            client.pump(true);
            assert!(
                observe_child(client.pid(), false).unwrap().is_none(),
                "CLI ended before observer connection: {}",
                String::from_utf8_lossy(&client.output)
            );
            assert!(Instant::now() < deadline, "observer connection deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn receive(&self, stream: &mut UnixStream, client: &mut Client) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut exact = |bytes: &mut [u8]| {
            let mut offset = 0;
            while offset < bytes.len() {
                match stream.read(&mut bytes[offset..]) {
                    Ok(0) => panic!("observer EOF: {}", String::from_utf8_lossy(&client.output)),
                    Ok(count) => offset += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        client.pump(true);
                        assert!(
                            Instant::now() < deadline,
                            "observer frame deadline: {}",
                            String::from_utf8_lossy(&client.output)
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("observer frame: {error}"),
                }
            }
        };
        let mut length = [0; 4];
        exact(&mut length);
        let length = u32::from_le_bytes(length) as usize;
        assert!((1..=32768).contains(&length));
        let mut bytes = vec![0; length];
        exact(&mut bytes);
        let frame: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(frame["version"], 1);
        assert_eq!(frame["context"], self.context);
        assert_eq!(frame["nonce"], self.nonce);
        frame
    }
    fn acknowledge(&self, stream: &mut UnixStream, reserved: &Value) {
        self.send(stream, &self.acknowledgment(reserved));
    }
    fn acknowledgment(&self, reserved: &Value) -> Value {
        serde_json::json!({
            "version": 1, "context": self.context, "nonce": self.nonce,
            "sequence": 2, "event": "reservation_committed",
            "owner_uuid": reserved["owner_uuid"], "document_revision": 42
        })
    }
    fn send(&self, stream: &mut UnixStream, frame: &Value) {
        let bytes = serde_json::to_vec(frame).unwrap();
        let mut wire = (bytes.len() as u32).to_le_bytes().to_vec();
        wire.extend_from_slice(&bytes);
        // Exercise actual fragmented stream framing, including its length prefix.
        for fragment in wire.chunks(3) {
            stream.write_all(fragment).unwrap();
            thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn assert_descriptor_not_inherited(pid: u32) {
    // Inspect only the key's presence. Never retain or print process environment
    // contents, which can include unrelated runner/application credentials.
    #[cfg(target_os = "linux")]
    let bytes = {
        let file = fs::File::open(format!("/proc/{pid}/environ")).unwrap();
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut bytes).unwrap();
        bytes
    };
    #[cfg(target_os = "macos")]
    let bytes = {
        let output = Command::new("/bin/ps")
            .args(["eww", "-p", &pid.to_string(), "-o", "command="])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    };
    assert!(bytes.len() <= 1024 * 1024);
    assert!(
        !bytes
            .windows(b"ZMUX_LOCAL_ZELLIJ_OBSERVER=".len())
            .any(|part| part == b"ZMUX_LOCAL_ZELLIJ_OBSERVER="),
        "observer descriptor inherited by an owned downstream process"
    );
}

fn assert_owner_alive(record: &Value) {
    for field in ["server", "supervisor", "workload"] {
        let identity: zmux_session_runtime::child::ProcessIdentity =
            serde_json::from_value(record[field].clone()).unwrap();
        assert_eq!(process_identity(identity.pid).unwrap(), identity);
    }
}

#[test]
fn observed_start_waits_for_correlated_protocol_ack_before_the_real_workload() {
    let f = Fixture::new();
    let host = Host::new();
    let count = f.root.join("observed-startup-count");
    let proof = f.root.join("observed-input-proof");
    let leaked = f.root.join("observer-context-leaked");
    let script = format!(
        "printf x >> '{}'; if [ \"${{ZMUX_LOCAL_ZELLIJ_OBSERVER+x}}\" = x ]; then printf leak > '{}'; fi; while IFS= read -r -t 20 line; do case \"$line\" in proof-*) printf '%s' \"$line\" > '{}' ;; esac; done",
        count.display(),
        leaked.display(),
        proof.display()
    );
    let mut client = Client::with_environment(
        &f,
        &[
            "local-zellij",
            "start",
            "observed",
            "--headless",
            "--command",
            &script,
        ],
        &host.environment(),
    );
    let mut stream = host.accept(&mut client, &count);
    let reserved = host.receive(&mut stream, &mut client);
    assert_eq!(
        process_identity(client.pid()).unwrap().executable,
        fs::canonicalize(env!("CARGO_BIN_EXE_zmux")).unwrap()
    );
    assert_eq!(reserved["event"], "owner_reserved");
    assert_eq!(reserved["sequence"], 1);
    assert_eq!(f.records().unwrap()[0]["state"], "reserved");
    let before_ack = Instant::now() + Duration::from_millis(200);
    while Instant::now() < before_ack {
        client.pump(true);
        assert!(
            !count.exists(),
            "workload started before protocol acknowledgment"
        );
        thread::sleep(Duration::from_millis(5));
    }
    // This host declares a revision to exercise protocol admission only. It is
    // deliberately not presented as a native storage checkpoint/save receipt.
    host.acknowledge(&mut stream, &reserved);
    let bound = host.receive(&mut stream, &mut client);
    assert_eq!(bound["event"], "owner_bound");
    assert_eq!(bound["sequence"], 3);
    assert_eq!(bound["record"]["id"], reserved["owner_uuid"]);
    let connected = host.receive(&mut stream, &mut client);
    assert_eq!(connected["event"], "transport_connected");
    assert_eq!(connected["sequence"], 4);
    assert_eq!(connected["owner_uuid"], reserved["owner_uuid"]);
    assert_eq!(connected["frontend"]["parent_pid"], client.pid());
    client.prove_input(&proof, || true);
    for field in ["server", "supervisor", "workload"] {
        assert_descriptor_not_inherited(bound["record"][field]["pid"].as_u64().unwrap() as u32);
    }
    assert_descriptor_not_inherited(connected["frontend"]["pid"].as_u64().unwrap() as u32);
    assert_eq!(fs::read(&count).unwrap(), b"x");
    assert!(!leaked.exists());
    kill(Pid::from_raw(client.pid() as i32), Signal::SIGTERM).unwrap();
    assert_eq!(client.finish(), 0);
    let finished = host.receive(&mut stream, &mut client);
    assert_eq!(finished["event"], "frontend_finished");
    assert_eq!(finished["sequence"], 5);
    assert_eq!(finished["owner_uuid"], reserved["owner_uuid"]);
    assert_eq!(
        finished["owned_client_exit"],
        serde_json::json!({"kind":"exited","value":0})
    );
    assert_eq!(finished["original_owner_preserved"], true);
    assert_eq!(finished["workers_joined"], true);
    assert_eq!(finished["requested_cancel"], true);
    assert_eq!(finished["transport"]["pending_to_server_on_cleanup"], 0);

    let saved = fs::read_to_string(f.root.join("profile/registry.json")).unwrap();
    assert!(!saved.contains(&host.nonce));
    assert!(!saved.contains("ZMUX_LOCAL_ZELLIJ_OBSERVER"));
    let id = reserved["owner_uuid"].as_str().unwrap();
    let second = Host::new();
    fs::remove_file(&proof).unwrap();
    let mut attached = Client::with_environment(
        &f,
        &["local-zellij", "attach", "--headless", "--id", id],
        &second.environment(),
    );
    let mut stream = second.accept(&mut attached, &f.root.join("must-not-create-on-attach"));
    let rebound = second.receive(&mut stream, &mut attached);
    assert_eq!(rebound["event"], "owner_bound");
    assert_eq!(rebound["sequence"], 1);
    assert_eq!(rebound["record"], bound["record"]);
    let connected = second.receive(&mut stream, &mut attached);
    assert_eq!(connected["event"], "transport_connected");
    assert_eq!(connected["sequence"], 2);
    attached.prove_input(&proof, || true);
    kill(Pid::from_raw(attached.pid() as i32), Signal::SIGHUP).unwrap();
    assert_eq!(attached.finish(), 0);
    let finished = second.receive(&mut stream, &mut attached);
    assert_eq!(finished["event"], "frontend_finished");
    assert_eq!(finished["sequence"], 3);
    assert_eq!(finished["original_owner_preserved"], true);
    assert_eq!(fs::read(&count).unwrap(), b"x");

    let third = Host::new();
    let mut rejected = Client::with_environment(
        &f,
        &["local-zellij", "attach", "--headless", "--id", id],
        &third.environment(),
    );
    let mut stream = third.accept(&mut rejected, &f.root.join("must-not-create-on-attach"));
    let rebound = third.receive(&mut stream, &mut rejected);
    assert_eq!(rebound["event"], "owner_bound");
    stream.write_all(b"unexpected second instruction").unwrap();
    assert_eq!(rejected.finish(), 1);
    assert!(String::from_utf8_lossy(&rejected.output).contains("unexpected observer frame"));
    let current = f.records().unwrap()[0].clone();
    for field in ["id", "server", "supervisor", "workload"] {
        assert_eq!(current[field], bound["record"][field]);
    }
    assert_eq!(fs::read(&count).unwrap(), b"x");
    assert_owner_alive(&bound["record"]);

    let disconnected = Host::new();
    fs::remove_file(&proof).unwrap();
    let mut client = Client::with_environment(
        &f,
        &["local-zellij", "attach", "--headless", "--id", id],
        &disconnected.environment(),
    );
    let mut stream = disconnected.accept(&mut client, &f.root.join("must-not-create-on-attach"));
    assert_eq!(
        disconnected.receive(&mut stream, &mut client)["event"],
        "owner_bound"
    );
    assert_eq!(
        disconnected.receive(&mut stream, &mut client)["event"],
        "transport_connected"
    );
    client.prove_input(&proof, || true);
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    assert_eq!(client.finish(), 1);
    assert!(String::from_utf8_lossy(&client.output).contains("observer EOF"));
    assert_owner_alive(&bound["record"]);
    assert_eq!(fs::read(&count).unwrap(), b"x");

    // A peer's SHUT_RD alone does not portably make the writer fail: Darwin
    // can still accept a small final frame. Actual EOF above is the CLI failure
    // control; deterministic partial-write refusal belongs to the real socket
    // backpressure test in the shared framing owner, without a CLI-only hook.
    eprintln!(
        "R03 observer protocol receipt: {}",
        serde_json::json!({
            "owner_uuid": reserved["owner_uuid"],
            "server": bound["record"]["server"],
            "supervisor": bound["record"]["supervisor"],
            "workload": bound["record"]["workload"],
            "declared_document_revision": 42,
            "checkpoint_origin": "synthetic_protocol_host",
            "native_durable_checkpoint_proven": false,
            "start_then_attach_input": true,
            "startup_count": fs::read(&count).unwrap().len(),
            "clean_terminate_and_hup_final_receipts": true,
            "unsolicited_input_rejected": true,
            "actual_observer_eof_rejected": true,
            "original_owner_alive_after_eof": true,
            "backend_frontend_supervisor_workload_context_scrub": true
        })
    );
}

#[test]
fn observer_descriptor_is_removed_from_the_actual_version_probe_environment() {
    let mut f = Fixture::new();
    let host = Host::new();
    let leaked = f.root.join("version-probe-context-leaked");
    let binary = f.root.join("version-probe");
    fs::write(&binary, format!(
        "#!/bin/sh\nif [ \"${{ZMUX_LOCAL_ZELLIJ_OBSERVER+x}}\" = x ]; then printf leak > '{}'; fi\nprintf 'zellij 0.43.0\\n'\n",
        leaked.display())).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    f.binary = binary;
    let output = Command::new(env!("CARGO_BIN_EXE_zmux"))
        .args(["local-zellij", "list"])
        .envs(f.environment())
        .envs(host.environment())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    assert!(!leaked.exists());
}

#[test]
fn invalid_or_missing_observer_ack_retains_reservation_without_authorizing_a_workload() {
    for case in [
        "nonce",
        "generation",
        "owner",
        "revision",
        "sequence",
        "oversize",
        "eof",
        "cancel",
        "deadline",
    ] {
        let f = Fixture::new();
        let host = Host::new();
        let count = f.root.join("must-not-start");
        let script = format!("printf started > '{}'", count.display());
        let mut client = Client::with_environment(
            &f,
            &[
                "local-zellij",
                "start",
                "denied",
                "--headless",
                "--command",
                &script,
            ],
            &host.environment(),
        );
        let mut stream = host.accept(&mut client, &count);
        let reserved = host.receive(&mut stream, &mut client);
        assert_eq!(reserved["event"], "owner_reserved");
        let mut frame = host.acknowledgment(&reserved);
        match case {
            "nonce" => frame["nonce"] = "0".repeat(64).into(),
            "generation" => frame["context"]["launch_generation"] = 2.into(),
            "owner" => frame["owner_uuid"] = uuid::Uuid::new_v4().to_string().into(),
            "revision" => frame["document_revision"] = 0.into(),
            "sequence" => frame["sequence"] = 1.into(),
            "oversize" => stream.write_all(&32769_u32.to_le_bytes()).unwrap(),
            "eof" => stream.shutdown(std::net::Shutdown::Both).unwrap(),
            "cancel" => kill(Pid::from_raw(client.pid() as i32), Signal::SIGTERM).unwrap(),
            "deadline" => {
                let deadline = Instant::now() + Duration::from_secs(12);
                while observe_child(client.pid(), false).unwrap().is_none() {
                    client.pump(true);
                    assert!(!count.exists());
                    assert!(
                        Instant::now() < deadline,
                        "observer renewed its startup deadline"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
            }
            _ => unreachable!(),
        }
        if ["nonce", "generation", "owner", "revision", "sequence"].contains(&case) {
            host.send(&mut stream, &frame);
        }
        assert_eq!(client.finish(), 1, "case {case}");
        assert!(!count.exists(), "case {case} authorized workload");
        let records = f.records().unwrap();
        assert_eq!(records[0]["id"], reserved["owner_uuid"]);
        assert_eq!(records[0]["state"], "uncertain");
        assert!(records[0]["server"].is_null());
        assert!(records[0]["supervisor"].is_null());
        assert!(records[0]["workload"].is_null());
        let diagnostic = String::from_utf8_lossy(&client.output);
        assert!(diagnostic.contains("reservation retained, do not replay"));
        assert!(
            diagnostic.contains(match case {
                "oversize" => "acknowledgment frame exceeded bound",
                "eof" => "observer EOF",
                "cancel" => "foreground startup canceled by signal",
                "deadline" => "observer frame deadline",
                _ => "acknowledgment correlation rejected",
            }),
            "case {case}: {diagnostic}"
        );
    }
}

#[test]
fn changed_actual_launcher_identity_rejects_observation_before_reserving_an_owner() {
    let f = Fixture::new();
    let host = Host::new();
    let mut descriptor: Value =
        serde_json::from_slice(&fs::read(&host.descriptor).unwrap()).unwrap();
    descriptor["launcher"]["start"][0] =
        (descriptor["launcher"]["start"][0].as_u64().unwrap() + 1).into();
    fs::write(&host.descriptor, serde_json::to_vec(&descriptor).unwrap()).unwrap();
    let count = f.root.join("must-not-start");
    let script = format!("printf started > '{}'", count.display());
    let mut client = Client::with_environment(
        &f,
        &[
            "local-zellij",
            "start",
            "bad-peer",
            "--headless",
            "--command",
            &script,
        ],
        &host.environment(),
    );
    // No authorization is sent for this negative control. Darwin can stop
    // exposing LOCAL_PEERPID once the rejected CLI disconnects, so do not race
    // an unnecessary server-side lookup after the product has already refused.
    assert_eq!(client.finish(), 1);
    assert!(
        String::from_utf8_lossy(&client.output)
            .contains("observer connected peer or namespace identity rejected")
    );
    assert!(!count.exists());
    assert!(f.records().is_none());
}

#[test]
fn actual_registry_capacity_refuses_sixty_fifth_reservation_before_any_workload() {
    // Actual failed one-use observer admissions reserve all slots. No registry
    // fixture, fake backend or executable workload supplies the asserted bound.
    let fixture = Fixture::new();
    let marker = fixture.root.join("capacity-must-not-start");
    let script = format!("printf started > '{}'", marker.display());
    for index in 0..64 {
        let host = Host::new();
        let name = format!("capacity-{index}");
        let mut client = Client::with_environment(
            &fixture,
            &[
                "local-zellij",
                "start",
                &name,
                "--headless",
                "--command",
                &script,
            ],
            &host.environment(),
        );
        let mut stream = host.accept(&mut client, &marker);
        let reserved = host.receive(&mut stream, &mut client);
        assert_eq!(reserved["event"], "owner_reserved");
        let mut denied = host.acknowledgment(&reserved);
        denied["document_revision"] = 0.into();
        host.send(&mut stream, &denied);
        assert_ne!(client.finish(), 0);
        assert!(!marker.exists());
    }
    let before = fixture.records().unwrap();
    assert_eq!(before.as_array().unwrap().len(), 64);
    assert!(
        before
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["server"].is_null() && r["supervisor"].is_null() && r["workload"].is_null())
    );
    let host = Host::new();
    let mut denied = Client::with_environment(
        &fixture,
        &[
            "local-zellij",
            "start",
            "capacity-65",
            "--headless",
            "--command",
            &script,
        ],
        &host.environment(),
    );
    assert_ne!(denied.finish(), 0);
    assert!(String::from_utf8_lossy(&denied.output).contains("registry is full"));
    // The optional observer connects before registry admission. Capacity refusal
    // may connect, but must emit no OwnerReserved frame or workload authority.
    let (mut refused_observer, _) = host.listener.accept().unwrap();
    refused_observer.set_nonblocking(false).unwrap();
    refused_observer
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(refused_observer.read(&mut byte).unwrap(), 0);
    assert_eq!(fixture.records().unwrap(), before);
    assert!(!marker.exists());
    let list = fixture.output(&["local-zellij", "list"]);
    assert!(list.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&list.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        64
    );
    let status = fixture.output(&[
        "local-zellij",
        "status",
        "--id",
        before[0]["id"].as_str().unwrap(),
    ]);
    assert!(status.status.success());
    eprintln!(
        "R03 real registry cap: {}",
        serde_json::json!({"actual_reserved_slots":64,"sixty_fifth_reservation":false,"workloads_spawned":0,"status_available":true,"list_available":true,"checkpoint_ack_origin":"synthetic_invalid_admission","native_durability_proven":false})
    );
}
