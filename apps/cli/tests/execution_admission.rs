//! Real original supervisor/helper processes; no Zellij or live-owner proof claim.
#![cfg(unix)]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zmux_remote_runtime::local_zellij::admission::{EXECUTION_DOMAIN, ExecutionBindingV2};
use zmux_session_runtime::child::process_identity;

fn send(stream: &mut UnixStream, value: &impl serde::Serialize) {
    let data = serde_json::to_vec(value).unwrap();
    stream
        .write_all(&(data.len() as u32).to_le_bytes())
        .unwrap();
    stream.write_all(&data).unwrap();
}
#[track_caller]
fn receive(stream: &mut UnixStream) -> Value {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let size = u32::from_le_bytes(size) as usize;
    assert!(size <= 8192);
    let mut data = vec![0; size];
    stream.read_exact(&mut data).unwrap();
    serde_json::from_slice(&data).unwrap()
}
fn inode(path: &std::path::Path) -> (u64, u64) {
    let m = fs::symlink_metadata(path).unwrap();
    (m.dev(), m.ino())
}
struct Fixture {
    root: PathBuf,
    child: Child,
    stream: UnixStream,
    birth: Value,
    hello: Value,
    spec: Value,
    counter: PathBuf,
    _listener: UnixListener,
}
impl Fixture {
    fn new() -> Self {
        Self::with_shell_copy(false)
    }
    fn with_shell_copy(copy_shell: bool) -> Self {
        let owner = Uuid::new_v4();
        let generation = Uuid::new_v4();
        let root = PathBuf::from("/tmp")
            .canonicalize()
            .unwrap()
            .join(format!("admit-{}", owner.simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let socket = root.join("bootstrap.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let profile = root.join("profile");
        fs::DirBuilder::new().mode(0o700).create(&profile).unwrap();
        let program = fs::canonicalize(env!("CARGO_BIN_EXE_zmux")).unwrap();
        let counter = root.join("user-code-count");
        let shell = if copy_shell {
            let shell = root.join("configured-shell");
            fs::copy(fs::canonicalize("/bin/sh").unwrap(), &shell).unwrap();
            shell
        } else {
            fs::canonicalize("/bin/sh").unwrap()
        };
        let spec = json!({"socket":socket,"starter":process_identity(std::process::id()).unwrap(),"program":program,"nonce":"a".repeat(64),"cwd":root,"shell":shell,"command":format!("printf x >> '{}'; exit 7",counter.display()),"owner_uuid":owner,"owner_generation":generation,"profile":profile,"admitted":true});
        let path = root.join("bootstrap.json");
        fs::write(&path, serde_json::to_vec(&spec).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let child = Command::new(&program)
            .arg("__local-zellij-supervisor")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let hello = receive(&mut stream);
        assert_eq!(hello["nonce"], spec["nonce"]);
        send(&mut stream, &"spawn_once");
        let birth = receive(&mut stream);
        let actual = process_identity(birth["pid"].as_u64().unwrap() as u32).unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), birth);
        Self {
            root,
            child,
            stream,
            birth,
            hello,
            spec,
            counter,
            _listener: listener,
        }
    }
    fn binding(&self) -> ExecutionBindingV2 {
        // Independent canonical invocation bytes, including JSON quoting of exact paths/script.
        let invocation = format!(
            "{{\"version\":1,\"shell\":{},\"argv\":[\"-lc\",{}],\"cwd\":{},\"environment_policy\":\"inherit_clean_v1\"}}",
            self.spec["shell"], self.spec["command"], self.spec["cwd"]
        );
        let mut hash = Sha256::new();
        hash.update(b"zmux.local-zellij.invocation.v1\0");
        hash.update(invocation.as_bytes());
        let supervisor = process_identity(self.child.id()).unwrap();
        let server = process_identity(std::process::id()).unwrap();
        serde_json::from_value(json!({"version":2,"domain":EXECUTION_DOMAIN,"owner_uuid":self.spec["owner_uuid"],"owner_generation":self.spec["owner_generation"],"namespace":{"root":inode(&self.root),"socket":inode(&self.root.join("bootstrap.sock"))},"completion_socket":self.hello["completion_socket"],"server":server,"supervisor":supervisor,"workload_birth":self.birth,"lifetime":{"pid":self.birth["pid"],"parent_pid":self.birth["parent_pid"],"start":self.birth["start"],"boot":self.birth["boot"]},"invocation":{"shell":self.spec["shell"],"invocation_digest":format!("{:x}",hash.finalize())},"admission_id":Uuid::new_v4()})).unwrap()
    }
    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "owned supervisor did not end");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn admit(&mut self, binding: &ExecutionBindingV2) {
        send(&mut self.stream, binding);
        assert_eq!(receive(&mut self.stream), binding.digest().unwrap());
    }
    fn terminal(&self, binding: &ExecutionBindingV2) -> Value {
        // The original supervisor retains its actual reaped Child result as a publisher.
        let mut control = UnixStream::connect(self.root.join("completion.sock")).unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(
            zmux_remote_runtime::local_zellij::observer_protocol::peer_identity(&control).unwrap(),
            process_identity(self.child.id()).unwrap()
        );
        let publication_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let challenge = Uuid::new_v4();
            send(
                &mut control,
                &json!({"version":2,"execution_contract":"admitted_lifetime_v2","owner_uuid":binding.owner_uuid,"owner_generation":binding.owner_generation,"binding_digest":binding.digest().unwrap(),"challenge":challenge,"operation":{"operation":"query"}}),
            );
            let reply = receive(&mut control);
            assert_eq!(reply["challenge"], challenge.to_string());
            assert_eq!(reply["execution_contract"], "admitted_lifetime_v2");
            if !reply["receipt"].is_null() {
                assert_eq!(
                    reply["receipt"]["execution_contract"],
                    "admitted_lifetime_v2"
                );
                return reply["receipt"]["receipt"].clone();
            }
            assert!(Instant::now() < publication_deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn actual_helper_never_executes_user_code_before_committed_run_and_executes_once_after() {
    let mut f = Fixture::new();
    thread::sleep(Duration::from_millis(30));
    assert!(
        !f.counter.exists(),
        "user code ran while actual helper awaited Run"
    );
    let binding = f.binding();
    f.admit(&binding);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !f.counter.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(fs::read(&f.counter).unwrap(), b"x");
    assert!(f.child.try_wait().unwrap().is_none());
    let terminal = f.terminal(&binding);
    assert_eq!(
        terminal["workload_exit"],
        json!({"kind":"exited","value":7})
    );
    assert_eq!(terminal["child_reaped"], true);
    assert_eq!(fs::read(&f.counter).unwrap(), b"x");
}
#[test]
fn actual_configured_shell_exec_failure_is_authenticated_and_retains_raw_child_exit() {
    let mut f = Fixture::with_shell_copy(true);
    let binding = f.binding();
    // The helper is already the measured blocked Child; fail only the subsequent exec.
    fs::remove_file(f.spec["shell"].as_str().unwrap()).unwrap();
    f.admit(&binding);
    let terminal = f.terminal(&binding);
    assert_eq!(
        terminal["workload_exit"],
        json!({"kind":"exited","value":1})
    );
    assert_eq!(terminal["child_reaped"], true);
    assert_eq!(
        terminal["execution_observation"],
        json!({"kind":"exec_failed","stage":"configured_shell_exec","errno":2})
    );
    assert!(!f.counter.exists());
    let receipt: zmux_remote_runtime::local_zellij::completion::TerminalReceiptV2 =
        serde_json::from_value(terminal).unwrap();
    let receipt = zmux_remote_runtime::local_zellij::completion::TerminalReceipt::V2(receipt);
    receipt.validate().unwrap();
    assert!(
        receipt
            .command_result()
            .unwrap_err()
            .to_string()
            .contains("exec failed")
    );
}
#[test]
fn actual_supervisor_eof_before_run_never_executes_user_code() {
    let mut f = Fixture::new();
    f.stream.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(!f.wait_exit().success());
    assert!(!f.counter.exists());
}
#[test]
fn actual_supervisor_rejects_changed_owner_generation_before_run() {
    let mut f = Fixture::new();
    let mut binding = f.binding();
    binding.owner_generation = Uuid::new_v4();
    send(&mut f.stream, &binding);
    assert!(!f.wait_exit().success());
    assert!(!f.counter.exists());
}
#[test]
fn actual_post_binding_bootstrap_disconnect_retains_the_once_only_child_result() {
    let mut f = Fixture::new();
    let binding = f.binding();
    send(&mut f.stream, &binding);
    f.stream.shutdown(std::net::Shutdown::Both).unwrap();
    let terminal = f.terminal(&binding);
    assert_eq!(
        terminal["workload_exit"],
        json!({"kind":"exited","value":7})
    );
    assert_eq!(terminal["child_reaped"], true);
    assert_eq!(fs::read(&f.counter).unwrap(), b"x");
    assert!(f.child.try_wait().unwrap().is_none());
}
