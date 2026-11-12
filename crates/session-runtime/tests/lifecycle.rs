use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::UnixDatagram,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use zmux_session_runtime::{
    INPUT_CHUNK_BYTES, InputReceipt, InputWriteFailure, InputWriteResult, InputWriteStatus, Launch,
    Session, SessionError, Snapshot, State,
};
use zmux_terminal_adapter::Size;

fn launch(script: &str) -> Session {
    Session::spawn(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        size: Size { rows: 8, cols: 60 },
    })
    .unwrap()
}

fn wait_for(session: &Session, predicate: impl Fn(&Snapshot) -> bool) -> std::sync::Arc<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.snapshot() {
            Ok(snapshot) if predicate(&snapshot) => return snapshot,
            Ok(snapshot) if snapshot.state == State::Failed => {
                panic!("session failed: {:?}", snapshot.failure)
            }
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => panic!("snapshot failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "session did not reach expected state"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn write_result(receipt: &mut InputReceipt) -> InputWriteResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match receipt.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(error) => panic!("write completion disappeared: {error}"),
        }
        assert!(Instant::now() < deadline, "PTY write did not complete");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn managed_provider_fifo_emits_v4_routes_and_exact_admission_receipts() {
    assert!(
        Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success()),
        "managed provider lifecycle producer tests require Python 3"
    );

    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let generation = uuid::Uuid::new_v4();
    let next_generation = uuid::Uuid::new_v4();
    let root = PathBuf::from(format!("/tmp/zmux-shell-{nonce}"));
    fs::create_dir(&root).expect("create private managed shell root");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .expect("secure managed shell root");
    let fifo = root.join("agent-events");
    let loss_fifo = root.join("agent-events-loss");
    for path in [&fifo, &loss_fifo] {
        assert!(
            Command::new("mkfifo")
                .arg(path)
                .status()
                .is_ok_and(|status| status.success()),
            "create managed lifecycle FIFO"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("secure managed lifecycle FIFO");
    }

    let mut stream = Command::new("python3")
        .arg("-c")
        .arg(include_str!("../src/shell/claude-lifecycle-stream.py"))
        .arg(&nonce)
        .env("ZMUX_SHELL_INTEGRATION_DIRECTORY", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the shipped managed lifecycle stream producer");
    let mut output = BufReader::new(stream.stdout.take().unwrap());
    let mut input = stream.stdin.take().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).expect("read lifecycle hello");
    let hello: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(hello["kind"], "hello");
    assert_eq!(hello["nonce"], nonce);

    serde_json::to_writer(
        &mut input,
        &serde_json::json!({
            "kind":"activate",
            "version":4,
            "nonce":nonce,
            "generation":generation,
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read lifecycle active frame");
    let active: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(active["kind"], "active");
    assert_eq!(active["generation"], generation.to_string());
    let marker = root.join("agent-lifecycle-generation");
    let metadata = fs::symlink_metadata(&marker).expect("active generation marker");
    assert!(metadata.is_file());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::read_to_string(&marker).unwrap().trim(),
        generation.to_string()
    );

    let mut writer = fs::OpenOptions::new()
        .write(true)
        .open(&fifo)
        .expect("open provider event FIFO after stream activation");
    let stale = serde_json::json!({
        "nonce":nonce,
        "generation":uuid::Uuid::new_v4(),
        "provider_observation": {
            "provider":"codex","scope":"subagent","session_id":"foreign/thread",
            "turn_id":"foreign/turn","state":"waiting","transition":"state_changed","marker":"live"
        }
    });
    serde_json::to_writer(&mut writer, &stale).unwrap();
    writer.write_all(b"\n").unwrap();

    let current = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_observation": {
            "provider":"codex","scope":"root","session_id":"thread:opaque/root",
            "turn_id":"turn/opaque:8","state":"working","transition":"state_changed","marker":"live"
        }
    });
    serde_json::to_writer(&mut writer, &current).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();

    line.clear();
    output
        .read_line(&mut line)
        .expect("read provider lifecycle frame");
    let provider: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(provider["kind"], "provider_state");
    assert_eq!(provider["version"], 4);
    assert_eq!(provider["nonce"], nonce);
    assert_eq!(provider["generation"], generation.to_string());
    assert_eq!(provider["sequence"], 1);
    assert_eq!(provider["observation"]["session_id"], "thread:opaque/root");
    assert_eq!(provider["observation"]["turn_id"], "turn/opaque:8");
    assert_eq!(provider["observation"].as_object().unwrap().len(), 7);
    assert!(!line.contains("foreign"));
    assert!(!line.contains("prompt"));

    let route_id = uuid::Uuid::new_v4();
    let provider_generation = uuid::Uuid::new_v4();
    let receipt_path = root.join(format!("p-{}", route_id.simple()));
    let receipt_endpoint = UnixDatagram::bind(&receipt_path)
        .expect("bind private remote-local provider-route receipt socket");
    fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o600))
        .expect("secure private provider-route receipt socket");
    receipt_endpoint
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let starting_route = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_route": {
            "provider":"codex",
            "route_id":route_id,
            "provider_generation":provider_generation,
            "phase":"starting"
        }
    });
    serde_json::to_writer(&mut writer, &starting_route).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read v4 provider-route admission frame");
    let route: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(route["kind"], "provider_route");
    assert_eq!(route["version"], 4);
    assert_eq!(route["nonce"], nonce);
    assert_eq!(route["generation"], generation.to_string());
    assert_eq!(route["sequence"], 2);
    assert_eq!(route["provider"], "codex");
    assert_eq!(route["route_id"], route_id.to_string());
    assert_eq!(
        route["provider_generation"],
        provider_generation.to_string()
    );
    assert_eq!(route["phase"], "starting");
    assert_eq!(route.as_object().unwrap().len(), 9);
    assert!(!line.contains(receipt_path.to_str().unwrap()));

    serde_json::to_writer(
        &mut input,
        &serde_json::json!({
            "kind":"provider_route_receipt",
            "version":4,
            "nonce":nonce,
            "generation":generation,
            "route_id":route_id,
            "accepted":true
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut datagram = [0; 512];
    let length = receipt_endpoint
        .recv(&mut datagram)
        .expect("forward exact Desktop route admission to producer socket");
    let receipt: serde_json::Value = serde_json::from_slice(&datagram[..length]).unwrap();
    assert_eq!(receipt["kind"], "provider_route_receipt");
    assert_eq!(receipt["version"], 4);
    assert_eq!(
        receipt["nonce"],
        uuid::Uuid::parse_str(&nonce).unwrap().to_string()
    );
    assert_eq!(receipt["generation"], generation.to_string());
    assert_eq!(receipt["route_id"], route_id.to_string());
    assert_eq!(receipt["accepted"], true);

    let ended_route = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_route": {
            "provider":"codex",
            "route_id":route_id,
            "provider_generation":provider_generation,
            "phase":"ended"
        }
    });
    serde_json::to_writer(&mut writer, &ended_route).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read v4 route retirement frame");
    let ended: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ended["kind"], "provider_route");
    assert_eq!(ended["sequence"], 3);
    assert_eq!(ended["route_id"], route_id.to_string());
    assert_eq!(ended["phase"], "ended");
    drop(receipt_endpoint);
    fs::remove_file(&receipt_path).expect("remove test-owned provider receipt socket");

    let cancelled_route_id = uuid::Uuid::new_v4();
    let cancelled_provider_generation = uuid::Uuid::new_v4();
    let cancelled_receipt_path = root.join(format!("p-{}", cancelled_route_id.simple()));
    let cancelled_receipt_endpoint = UnixDatagram::bind(&cancelled_receipt_path)
        .expect("bind private receipt socket for a rejected provider route");
    fs::set_permissions(&cancelled_receipt_path, fs::Permissions::from_mode(0o600))
        .expect("secure rejected provider route receipt socket");
    let starting_cancelled_route = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_route": {
            "provider":"opencode",
            "route_id":cancelled_route_id,
            "provider_generation":cancelled_provider_generation,
            "phase":"starting"
        }
    });
    serde_json::to_writer(&mut writer, &starting_cancelled_route).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read second v4 provider-route admission frame");
    let starting: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(starting["kind"], "provider_route");
    assert_eq!(starting["sequence"], 4);
    assert_eq!(starting["route_id"], cancelled_route_id.to_string());
    assert_eq!(starting["phase"], "starting");

    serde_json::to_writer(
        &mut input,
        &serde_json::json!({
            "kind":"provider_route_receipt",
            "version":4,
            "nonce":nonce,
            "generation":generation,
            "route_id":cancelled_route_id,
            "accepted":false
        }),
    )
    .unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
    let mut cancelled_datagram = [0; 512];
    let cancelled_length = cancelled_receipt_endpoint
        .recv(&mut cancelled_datagram)
        .expect("forward exact Desktop rejection to remote provider launcher");
    let rejected: serde_json::Value =
        serde_json::from_slice(&cancelled_datagram[..cancelled_length]).unwrap();
    assert_eq!(rejected["route_id"], cancelled_route_id.to_string());
    assert_eq!(rejected["accepted"], false);

    let cancelled_route = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_route": {
            "provider":"opencode",
            "route_id":cancelled_route_id,
            "provider_generation":cancelled_provider_generation,
            "phase":"cancelled"
        }
    });
    serde_json::to_writer(&mut writer, &cancelled_route).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read v4 provider-route reservation cancellation");
    let cancelled: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(cancelled["kind"], "provider_route");
    assert_eq!(cancelled["sequence"], 5);
    assert_eq!(cancelled["route_id"], cancelled_route_id.to_string());
    assert_eq!(
        cancelled["provider_generation"],
        cancelled_provider_generation.to_string()
    );
    assert_eq!(cancelled["phase"], "cancelled");
    drop(cancelled_receipt_endpoint);
    fs::remove_file(&cancelled_receipt_path)
        .expect("remove test-owned rejected provider receipt socket");

    // Replace the marker while the old stream is still active. Its cleanup
    // must leave the newer attachment's marker untouched.
    fs::write(&marker, format!("{next_generation}\n")).unwrap();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
    let malformed_current = serde_json::json!({
        "nonce":nonce,
        "generation":generation,
        "provider_observation": {
            "provider":"codex","scope":"root","session_id":"thread:opaque/root",
            "turn_id":"turn/opaque:8","state":"working","transition":"state_changed",
            "marker":"live","prompt":"private prompt must not be forwarded"
        }
    });
    serde_json::to_writer(&mut writer, &malformed_current).unwrap();
    writer.write_all(b"\n").unwrap();
    writer.flush().unwrap();
    line.clear();
    output
        .read_line(&mut line)
        .expect("read provider evidence gap");
    let gap: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(gap["kind"], "gap");
    assert_eq!(gap["sequence"], 6);
    assert!(!line.contains("private prompt"));

    drop(writer);
    drop(input);
    let status = stream.wait().expect("reap managed lifecycle stream");
    assert_eq!(status.code(), Some(73));
    assert_eq!(
        fs::read_to_string(&marker).unwrap().trim(),
        next_generation.to_string()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_provider_launch_waits_for_exact_ack_and_falls_back_once_on_reject_or_timeout() {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let generation = uuid::Uuid::new_v4();
    let root = PathBuf::from(format!("/tmp/zmux-shell-{nonce}"));
    fs::create_dir(&root).expect("create private managed shell root");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .expect("secure managed shell root");
    for name in ["agent-events", "agent-events-loss"] {
        let path = root.join(name);
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("secure managed lifecycle FIFO");
    }
    let generation_path = root.join("agent-lifecycle-generation");
    fs::write(&generation_path, format!("{generation}\n")).unwrap();
    fs::set_permissions(&generation_path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(
        root.join("provider-opencode-plugin.js"),
        b"export default {};\n",
    )
    .unwrap();

    let producer_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/shell/provider-launch.py");
    let driver = r#"
import importlib.util,json,os,select,socket,sys,threading,time,types,uuid
spec=importlib.util.spec_from_file_location("provider_launch",sys.argv[1])
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
root=os.environ["ZMUX_SHELL_INTEGRATION_DIRECTORY"]
nonce=os.environ["ZMUX_AGENT_LIFECYCLE_NONCE"]
generation=open(os.path.join(root,"agent-lifecycle-generation"),encoding="ascii").read().strip()
reader=os.open(os.path.join(root,"agent-events"),os.O_RDONLY|os.O_NONBLOCK)
writer_hold=os.open(os.path.join(root,"agent-events"),os.O_WRONLY|os.O_NONBLOCK)
pending=bytearray()
def next_frame(timeout=3.0):
    deadline=time.monotonic()+timeout
    while time.monotonic()<deadline:
        if b"\n" in pending:
            line,_,rest=pending.partition(b"\n");pending[:]=rest
            return json.loads(line)
        ready,_,_=select.select([reader],[],[],max(0,deadline-time.monotonic()))
        if ready: pending.extend(os.read(reader,4096))
    raise AssertionError("provider route FIFO event timed out")
class Child:
    returncode=0
    def poll(self): return 0
    def wait(self): return 0
class FakeProcess:
    def __init__(self): self.spawned=[];self.commands=[];self.fail_spawn=False
    def Popen(self,argv,env=None):
        if self.fail_spawn: raise OSError("synthetic pre-spawn failure")
        self.spawned.append(list(argv));return Child()
    def run(self,argv):
        self.commands.append(list(argv))
        return types.SimpleNamespace(returncode=23)
class FakeServer:
    def __init__(self,executable): pass
    def rpc(self,method,params): return {"config":{"hooks":{}}}
    def close(self): pass
m.shutil.which=lambda name:"/fake/"+name
m.AppServer=FakeServer
m.PROVIDER_ROUTE_RECEIPT_TIMEOUT=0.20
m.interactive_opencode=lambda args: True
m.expected_opencode_session=lambda args: None
m.bounded_command_output=lambda *args,**kwargs:b"--standalone"
m.advertises_option=lambda output,option:True
def run_case(provider,disposition):
    fake=FakeProcess();fake.fail_spawn=disposition=="spawn-fail";m.subprocess=fake
    result=[]
    def launch():
        result.append(m.run_codex([],root) if provider=="codex" else m.run_opencode([],root))
    worker=threading.Thread(target=launch);worker.start()
    start=next_frame()
    assert start["provider_route"]["phase"]=="starting",start
    route=start["provider_route"]
    assert route["provider"]==provider
    assert route["provider_generation"] and route["route_id"]
    assert not fake.spawned and not fake.commands, "provider spawned before Desktop admission"
    receipt_path=os.path.join(root,"p-"+uuid.UUID(route["route_id"]).hex)
    if disposition in ("accept","spawn-fail"):
        endpoint=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM)
        endpoint.sendto(json.dumps({"kind":"provider_route_receipt","version":4,
            "nonce":str(uuid.UUID(nonce)),"generation":generation,
            "route_id":route["route_id"],"accepted":True},separators=(",",":")).encode(),receipt_path)
        endpoint.close()
    elif disposition=="reject":
        endpoint=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM)
        endpoint.sendto(json.dumps({"kind":"provider_route_receipt","version":4,
            "nonce":str(uuid.UUID(nonce)),"generation":generation,
            "route_id":route["route_id"],"accepted":False},separators=(",",":")).encode(),receipt_path)
        endpoint.close()
    worker.join(3)
    assert not worker.is_alive(), "provider launch did not leave bounded admission wait"
    retired=next_frame()
    expected_phase="ended" if disposition=="accept" else "cancelled"
    assert retired["provider_route"]["phase"]==expected_phase,retired
    assert retired["provider_route"]["route_id"]==route["route_id"]
    assert retired["provider_route"]["provider_generation"]==route["provider_generation"]
    if disposition=="accept":
        assert result==[0],result
        assert len(fake.spawned)==1 and not fake.commands,(fake.spawned,fake.commands)
        if provider=="opencode": assert "--standalone" in fake.spawned[0]
        else: assert "hooks.SessionStart=" in " ".join(fake.spawned[0])
    else:
        assert result==[23],result
        assert not fake.spawned and fake.commands==[["/fake/"+provider]],(fake.spawned,fake.commands)
for provider in ("codex","opencode"):
    run_case(provider,"accept")
    run_case(provider,"reject")
    run_case(provider,"timeout")
    run_case(provider,"spawn-fail")
os.close(reader);os.close(writer_hold)
"#;
    let status = Command::new("python3")
        .arg("-c")
        .arg(driver)
        .arg(&producer_path)
        .env("ZMUX_SHELL_INTEGRATION_DIRECTORY", &root)
        .env("ZMUX_AGENT_LIFECYCLE_NONCE", &nonce)
        .output()
        .expect("run the production managed provider launcher with bounded fakes");
    assert!(
        status.status.success(),
        "managed provider launcher contract failed: stdout={} stderr={}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_worker_encodes_semantic_keys_and_whole_paste_from_live_modes() {
    use zmux_session_runtime::{Key, KeyAction, KeyEvent};
    let script = r#"import os,tty
tty.setraw(0)
def read(expected):
    actual=b''
    while len(actual)<len(expected):actual+=os.read(0,len(expected)-len(actual))
    assert actual==expected,repr((actual,expected))
os.write(1,b'\x1b[?1h\x1b[?2004hREADY')
read(b'\x1bOA\x1b[1;6D\x1f\t\x1b[200~one\r\ntwo\x1b[201~'+b'\x1b[200~'+b'L'*16384+b'\x1b[201~END')
os.write(1,b'\x1b[?1l\x1b[?2004lNEXT')
read(b'\x1b[Aone\rtwoEND')
os.write(1,b'\r\nSEMANTIC_OK')
"#;
    let mut session = Session::spawn(Launch {
        program: "python3".into(),
        args: vec!["-u".into(), "-c".into(), script.into()],
        cwd: None,
        size: Size { rows: 8, cols: 60 },
    })
    .unwrap();
    wait_for(&session, |s| {
        s.viewport.application_cursor
            && s.viewport.bracketed_paste
            && s.viewport.contents.contains("READY")
    });
    session
        .send_key(KeyEvent {
            key: Key::ArrowUp,
            action: KeyAction::Press,
            modifiers: 0,
            legacy_control: None,
        })
        .unwrap();
    session
        .send_key(KeyEvent {
            key: Key::ArrowLeft,
            action: KeyAction::Repeat,
            modifiers: 5,
            legacy_control: None,
        })
        .unwrap();
    session
        .send_key(KeyEvent {
            key: Key::ArrowUp,
            action: KeyAction::Release,
            modifiers: 0,
            legacy_control: None,
        })
        .unwrap();
    for (key, modifiers, byte) in [(Key::Backspace, 4, 31), (Key::Tab, 5, 9)] {
        session
            .send_key(KeyEvent {
                key,
                action: KeyAction::Press,
                modifiers,
                legacy_control: Some(byte),
            })
            .unwrap();
    }
    session.send_paste("one\r\ntwo").unwrap();
    session
        .send_paste(&"L".repeat(zmux_session_runtime::PASTE_BYTES))
        .unwrap();
    session.send_input(b"END").unwrap();
    wait_for(&session, |s| {
        !s.viewport.application_cursor
            && !s.viewport.bracketed_paste
            && s.viewport.contents.contains("NEXT")
    });
    session
        .send_key(KeyEvent {
            key: Key::ArrowUp,
            action: KeyAction::Press,
            modifiers: 0,
            legacy_control: None,
        })
        .unwrap();
    session.send_paste("one\r\ntwo").unwrap();
    session.send_input(b"END").unwrap();
    let done = wait_for(&session, |s| s.state == State::Exited);
    assert!(
        done.viewport.contents.contains("SEMANTIC_OK"),
        "{}",
        done.viewport.contents
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn failed_start_releases_its_owned_resource_on_the_session_worker() {
    struct Resource(std::sync::mpsc::SyncSender<thread::ThreadId>);
    impl Drop for Resource {
        fn drop(&mut self) {
            let _ = self.0.try_send(thread::current().id());
        }
    }
    let caller = thread::current().id();
    let (released, observation) = std::sync::mpsc::sync_channel(1);
    let result = Session::spawn_with_resource(
        Launch {
            program: "/dev/null/zmux-missing-command".into(),
            args: Vec::new(),
            cwd: None,
            size: Size { rows: 8, cols: 60 },
        },
        &[],
        Resource(released),
    );
    assert!(
        matches!(result, Err(SessionError::Failure(message)) if message.contains("launch PTY command"))
    );
    let worker = observation
        .try_recv()
        .expect("failed startup returned before resource cleanup");
    assert_ne!(
        worker, caller,
        "startup cleanup ran on the launching caller"
    );
}

#[test]
fn write_receipt_confirms_full_pty_acceptance_in_the_existing_input_order() {
    let mut session = launch("stty raw -echo; printf READY; cat");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    session.send_input(b"before-").unwrap();
    let mut receipt = session
        .send_input_with_receipt(b"reply-", Instant::now() + Duration::from_secs(2))
        .unwrap();
    session.send_input(b"after").unwrap();
    let result = write_result(&mut receipt);
    assert_eq!(result.status, InputWriteStatus::Complete);
    assert_eq!(result.bytes_written, 6);
    assert_eq!(result.total_bytes, 6);
    assert_eq!(result.reason, None);
    wait_for(&session, |s| {
        s.viewport.contents.contains("before-reply-after")
    });
    assert!(matches!(
        receipt.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn queued_write_deadline_cancels_before_a_blocked_predecessor_drains_without_replay() {
    let mut session = launch("stty raw -echo; printf READY; kill -STOP $$; cat");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    for _ in 0..8 {
        session.send_input(&[b'x'; INPUT_CHUNK_BYTES]).unwrap();
    }
    let mut receipt = session
        .send_input_with_receipt(b"EXPIRED_TOKEN", Instant::now() + Duration::from_millis(40))
        .unwrap();
    let result = write_result(&mut receipt);
    assert_eq!(result.status, InputWriteStatus::NotWritten);
    assert_eq!(result.bytes_written, 0);
    assert_eq!(result.reason, Some(InputWriteFailure::Deadline));
    // The child cannot drain its input until this explicit release, so expiry
    // must complete while an earlier input is still blocked in the PTY.
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(session.process_id() as i32),
        nix::sys::signal::Signal::SIGCONT,
    )
    .unwrap();
    session.send_input(b"\r\nAFTER_DEADLINE\r\n").unwrap();
    let after = wait_for(&session, |s| s.viewport.contents.contains("AFTER_DEADLINE"));
    assert!(!after.viewport.contents.contains("EXPIRED_TOKEN"));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn bounded_write_receipts_finish_once_under_backpressure_deadline_and_shutdown() {
    for shutdown in [false, true] {
        let mut session = launch("stty raw -echo; printf READY; exec sleep 60");
        wait_for(&session, |s| s.viewport.contents.contains("READY"));
        let deadline = Instant::now()
            + if shutdown {
                Duration::from_secs(30)
            } else {
                Duration::from_millis(80)
            };
        let mut receipts = Vec::new();
        for _ in 0..zmux_session_runtime::INPUT_QUEUE_CHUNKS {
            receipts.push(
                session
                    .send_input_with_receipt(&[b'x'; INPUT_CHUNK_BYTES], deadline)
                    .unwrap(),
            );
        }
        if shutdown {
            session.shutdown(Duration::from_secs(3)).unwrap();
        }
        let results: Vec<_> = receipts.iter_mut().map(write_result).collect();
        assert!(
            results
                .iter()
                .any(|result| result.status != InputWriteStatus::Complete),
            "a non-reading PTY must apply backpressure"
        );
        for (receipt, result) in receipts.iter_mut().zip(results) {
            assert_eq!(result.total_bytes, INPUT_CHUNK_BYTES);
            match result.status {
                InputWriteStatus::Complete => {
                    assert_eq!(result.bytes_written, INPUT_CHUNK_BYTES);
                    assert_eq!(result.reason, None);
                }
                InputWriteStatus::NotWritten => assert_eq!(result.bytes_written, 0),
                InputWriteStatus::Partial => {
                    assert!(result.bytes_written > 0 && result.bytes_written < INPUT_CHUNK_BYTES)
                }
                InputWriteStatus::Unknown => {
                    panic!("normal deadline/shutdown lost worker ownership")
                }
            }
            if result.status != InputWriteStatus::Complete {
                assert_eq!(
                    result.reason,
                    Some(if shutdown {
                        InputWriteFailure::SessionClosed
                    } else {
                        InputWriteFailure::Deadline
                    })
                );
            }
            // The worker may still own a cancelled queued input, but cannot
            // issue another result or write it after the receipt was returned.
            assert!(receipt.try_recv().is_err());
        }
        if !shutdown {
            session.shutdown(Duration::from_secs(3)).unwrap();
        }
        assert!(receipts.iter_mut().all(|receipt| matches!(
            receipt.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        )));
    }
}

#[test]
fn real_pty_output_is_parsed_and_process_exit_is_reported() {
    let mut session = launch("printf 'old\\r\\033[2Khéllo 世界\\n'; exit 7");
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert_eq!(snapshot.exit_code, Some(7));
    assert_eq!(snapshot.viewport.contents, "héllo 世界");
    assert!(snapshot.bytes_read > 0);
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn socket_password_scrubbing_wins_over_application_environment_overrides() {
    let mut session = Session::spawn_with_environment(Launch {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "printf 'credential=%s context=%s' \"${ZMUX_SOCKET_PASSWORD-unset}\" \"$ZMUX_SURFACE_ID\"".into()],
        cwd: None,
        size: Size { rows: 8, cols: 60 },
    }, &[("ZMUX_SOCKET_PASSWORD".into(), "synthetic-secret".into()), ("ZMUX_SURFACE_ID".into(), "owned-surface".into())]).unwrap();
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert_eq!(
        snapshot.viewport.contents,
        "credential=unset context=owned-surface"
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn input_and_resize_reach_the_real_terminal() {
    let mut session = launch("read line; stty size; printf '\\naccepted:%s\\n' \"$line\"");
    session.resize(Size { rows: 12, cols: 45 }).unwrap();
    wait_for(&session, |s| s.viewport.size == Size { rows: 12, cols: 45 });
    session.send_input(b"test-input\n").unwrap();
    let snapshot = wait_for(&session, |s| s.state == State::Exited);
    assert!(snapshot.viewport.contents.contains("12 45"));
    assert!(snapshot.viewport.contents.contains("accepted:test-input"));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn interactive_shell_preserves_exact_multiline_input_across_native_chunk_boundaries() {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("multiline input fixture cleanup: {error}");
            }
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("zmux-multiline-{}-{nonce:x}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = Fixture(root);
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let payload: String = (0..24)
        .map(|row| format!("row {row:02}: finally keeps its exact bytes.\n"))
        .collect();
    for (name, program, args) in [
        ("bash", "/bin/bash", vec!["--noprofile", "--norc", "-i"]),
        ("zsh", "/bin/zsh", vec!["-f", "-i"]),
    ] {
        // Both are stock macOS shells. Linux hosts without zsh still exercise
        // bash's interactive line editor; no user startup files are loaded.
        if !std::path::Path::new(program).is_file() {
            assert_eq!(name, "zsh", "the bash fixture dependency is missing");
            continue;
        }
        let output = fixture.0.join(format!("{name}.txt"));
        let command = format!(
            "python3 -c {} {} {}\r",
            quote(
                "import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(sys.argv[2].encode()); print('MULTILINE_'+'CAPTURED', flush=True)"
            ),
            quote(output.to_str().unwrap()),
            quote(&payload)
        );
        assert!(command.len() > 1024 && command.len() < 2048);
        let mut session = Session::spawn_with_environment(
            Launch {
                program: program.into(),
                args: args.into_iter().map(String::from).collect(),
                cwd: Some(fixture.0.clone()),
                size: Size { rows: 31, cols: 63 },
            },
            &[("HOME".into(), fixture.0.to_string_lossy().into_owned())],
        )
        .unwrap();
        // Match the native bridge's two 1024-byte admissions without bypassing
        // the real session writer, shell line discipline, or shell argument parser.
        for chunk in command.as_bytes().chunks(1024) {
            session.send_input(chunk).unwrap();
        }
        wait_for(&session, |value| {
            value.viewport.contents.contains("MULTILINE_CAPTURED")
        });
        assert_eq!(fs::read(output).unwrap(), payload.as_bytes(), "{name}");
        session.shutdown(Duration::from_secs(3)).unwrap();
    }
}

#[test]
fn output_flood_does_not_prevent_cancel_and_leader_reaping() {
    let mut session = launch("while :; do printf 'busy-output\\n'; done");
    let pid = session.process_id();
    wait_for(&session, |s| s.bytes_read > 20_000);
    session.shutdown(Duration::from_secs(3)).unwrap();
    assert_eq!(session.snapshot().unwrap().state, State::Closed);
    assert_eq!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    );
    assert!(matches!(
        session.send_input(b"no\n"),
        Err(SessionError::Closed)
    ));
}

#[test]
fn malformed_requests_do_not_change_session_geometry_or_reach_input() {
    let mut session = launch("exec sleep 60");
    let before = session.snapshot().unwrap().viewport.size;
    assert!(session.resize(Size { rows: 0, cols: 60 }).is_err());
    assert!(matches!(
        session.send_input(&vec![b'x'; INPUT_CHUNK_BYTES + 1]),
        Err(SessionError::InputSize)
    ));
    assert!(matches!(
        session.send_input(b""),
        Err(SessionError::InputSize)
    ));
    assert_eq!(session.snapshot().unwrap().viewport.size, before);
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn dropping_an_active_client_requests_cleanup_without_joining() {
    let session = launch("exec sleep 60");
    let pid = session.process_id();
    drop(session);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None)
            == Err(nix::errno::Errno::ESRCH)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "dropped session left its leader running"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn saturated_input_returns_backpressure_without_blocking_cancellation() {
    let mut session = launch("stty raw -echo; printf 'READY'; exec sleep 60");
    wait_for(&session, |s| s.viewport.contents.contains("READY"));
    let input = [b'x'; INPUT_CHUNK_BYTES];
    let mut full = false;
    for _ in 0..1_000 {
        match session.send_input(&input) {
            Ok(()) => {}
            Err(SessionError::QueueFull) => {
                full = true;
                break;
            }
            Err(error) => panic!("unexpected input failure: {error}"),
        }
    }
    assert!(
        full,
        "a non-reading process must encounter bounded input backpressure"
    );
    session.shutdown(Duration::from_secs(3)).unwrap();
    assert_eq!(session.snapshot().unwrap().state, State::Closed);
}

#[test]
fn exited_session_rejects_new_input_and_resize_requests() {
    let mut session = launch("exit 0");
    wait_for(&session, |s| s.state == State::Exited);
    assert!(matches!(
        session.resize(Size { rows: 10, cols: 40 }),
        Err(SessionError::Closed)
    ));
    assert!(matches!(
        session.send_input(b"unused\n"),
        Err(SessionError::Closed)
    ));
    session.shutdown(Duration::from_secs(3)).unwrap();
}

#[test]
fn exited_process_history_remains_navigable_without_accepting_new_input() {
    let mut session =
        launch("i=0; while [ $i -lt 30 ]; do printf 'DONE_%02d\\n' $i; i=$((i+1)); done; exit 7");
    let done = wait_for(&session, |s| s.state == State::Exited);
    let observation = zmux_session_runtime::child::observe_child(session.process_id(), false);
    assert!(
        matches!(observation, Err(ref error) if error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32)),
        "completed history retained an unreaped child: {observation:?}"
    );
    let bytes = done.bytes_read;
    assert!(!done.viewport.contents.contains("DONE_00"));
    session.scroll(1000).unwrap();
    let history = wait_for(&session, |s| s.viewport.contents.contains("DONE_00"));
    assert_eq!(history.state, State::Exited);
    assert_eq!(history.exit_code, Some(7));
    assert_eq!(history.bytes_read, bytes);
    assert!(matches!(
        session.send_input(b"must not execute"),
        Err(SessionError::Closed)
    ));
    session.scroll(0).unwrap();
    wait_for(&session, |s| {
        s.viewport.scrollback_offset == 0 && s.viewport.contents.contains("DONE_29")
    });
    session.shutdown(Duration::from_secs(3)).unwrap();
}
