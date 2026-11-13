use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    ops::Deref,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_remote_runtime::{
    Forwarding, JumpHost, ListenEndpoint, MAX_COMMAND_BYTES, ProcessSpec, SSH_PROGRAM,
    SshDescriptor,
    connection::{ConnectionError, ManagedConnection},
};

const LIFECYCLE_FRAME_DEADLINE: Duration = Duration::from_secs(15);

fn receive_non_heartbeat_lifecycle_frame(
    receiver: &mpsc::Receiver<String>,
    expectation: &str,
) -> (serde_json::Value, Option<u64>) {
    let deadline = Instant::now() + LIFECYCLE_FRAME_DEADLINE;
    let mut last_heartbeat_sequence = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out after {LIFECYCLE_FRAME_DEADLINE:?} waiting for {expectation}; last heartbeat sequence: {last_heartbeat_sequence:?}"
        );
        let line = receiver.recv_timeout(remaining).unwrap_or_else(|error| {
            panic!(
                "timed out after {LIFECYCLE_FRAME_DEADLINE:?} waiting for {expectation}; last heartbeat sequence: {last_heartbeat_sequence:?}; receive error: {error}"
            )
        });
        let frame: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("decode {expectation} lifecycle frame: {error}"));
        if frame["kind"] == "heartbeat" {
            last_heartbeat_sequence = frame["sequence"].as_u64();
            continue;
        }
        return (frame, last_heartbeat_sequence);
    }
}

#[test]
fn private_control_paths_are_short_unique_and_mode_0700() {
    let first = ManagedConnection::create(descriptor("first-host")).unwrap();
    let second = ManagedConnection::create(descriptor("second-host")).unwrap();

    assert_ne!(first.control_path(), second.control_path());
    assert_eq!(
        first.control_path().parent().unwrap().parent().unwrap(),
        std::path::Path::new("/tmp")
    );
    assert!(first.control_path().as_os_str().as_encoded_bytes().len() <= 64);
    assert_eq!(
        fs::metadata(first.control_path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn terminal_owns_the_master_and_preserves_the_validated_descriptor() {
    let descriptor = SshDescriptor::builder("deploy@prod-alias")
        .unwrap()
        .config_file("/tmp/team ssh config")
        .port(2222)
        .identity("/tmp/team key")
        .jump_host(JumpHost::parse("jump@bastion", Some(2200)).unwrap())
        .forwarding(Forwarding::Agent { enabled: true })
        .forwarding(Forwarding::Dynamic {
            listen: ListenEndpoint::parse(Some("127.0.0.1"), 1080).unwrap(),
        })
        .command(["printf", "literal remote command"])
        .build()
        .unwrap();
    let original = descriptor.process_spec();
    let connection = Arc::new(ManagedConnection::create(descriptor).unwrap());
    assert!(!connection.shell_integration_enabled());
    let spec = connection
        .terminal_spec(&zmux_session_runtime::shell_integration::OpenRequestNonce::generate());

    assert_eq!(spec.program, "ssh");
    assert_eq!(
        &spec.args[..9],
        [
            "-S",
            connection.control_path().to_str().unwrap(),
            "-o",
            "ControlMaster=yes",
            "-o",
            "ControlPersist=no",
            "-o",
            "ForkAfterAuthentication=no",
            "-tt",
        ]
    );
    assert_eq!(&spec.args[9..], original.args);
}

#[test]
fn operations_have_a_closed_route_and_reject_unsafe_commands() {
    let descriptor = SshDescriptor::builder("deploy@prod-alias")
        .unwrap()
        .port(2222)
        .identity("/tmp/key")
        .jump_host(JumpHost::parse("bastion", None).unwrap())
        .build()
        .unwrap();
    let connection = Arc::new(ManagedConnection::create(descriptor).unwrap());
    let nonce = zmux_session_runtime::shell_integration::OpenRequestNonce::generate();
    let lifecycle = connection.agent_lifecycle_spec(&nonce).unwrap();
    assert_eq!(lifecycle.program, SSH_PROGRAM);
    assert_eq!(
        lifecycle.args[3],
        connection.control_path().to_str().unwrap()
    );
    assert_eq!(lifecycle.args[5], "ControlMaster=no");
    assert_eq!(lifecycle.args[11], "ProxyCommand=/usr/bin/false");
    assert!(
        lifecycle
            .args
            .last()
            .unwrap()
            .contains("claude-lifecycle-stream.py")
    );
    assert!(
        lifecycle
            .args
            .last()
            .unwrap()
            .contains(&nonce.remote_integration_directory())
    );

    assert_eq!(
        connection.operation_spec("printf '%s' value").unwrap(),
        ProcessSpec {
            program: SSH_PROGRAM.into(),
            args: vec![
                "-F".into(),
                "none".into(),
                "-S".into(),
                connection.control_path().to_str().unwrap().into(),
                "-o".into(),
                "ControlMaster=no".into(),
                "-o".into(),
                "ControlPersist=no".into(),
                "-o".into(),
                "BatchMode=yes".into(),
                "-o".into(),
                "ProxyCommand=/usr/bin/false".into(),
                "-o".into(),
                "PermitLocalCommand=no".into(),
                "-T".into(),
                "-p".into(),
                "2222".into(),
                "-l".into(),
                "deploy".into(),
                "--".into(),
                "prod-alias".into(),
                "printf '%s' value".into(),
            ],
        }
    );
    for command in ["", "  \t", "printf ok\nrm bad", "value\0suffix"] {
        assert!(
            connection.operation_spec(command).is_err(),
            "accepted {command:?}"
        );
    }
    assert!(matches!(
        connection.operation_spec(&"x".repeat(MAX_COMMAND_BYTES + 1)),
        Err(ConnectionError::OperationCommandTooLarge)
    ));
}

#[test]
fn openssh_config_applies_to_master_but_cannot_change_auxiliary_route() {
    let fixture = ManagedConnection::create(descriptor("fixture-owner")).unwrap();
    let config_path = fixture.control_path().parent().unwrap().join("config");
    fs::write(
        &config_path,
        "Host prod-alias\n  HostName configured.invalid\n  User configured-user\n  Port 2202\n  ProxyCommand /bin/echo configured-proxy\n  PermitLocalCommand yes\n  LocalCommand /bin/echo configured-local\n  RemoteCommand printf configured-remote\n",
    )
    .unwrap();
    let descriptor = SshDescriptor::builder("prod-alias")
        .unwrap()
        .config_file(config_path.to_str().unwrap())
        .build()
        .unwrap();
    let connection = ManagedConnection::create(descriptor).unwrap();
    assert!(!connection.shell_integration_enabled());
    assert!(matches!(
        connection.agent_lifecycle_spec(
            &zmux_session_runtime::shell_integration::OpenRequestNonce::generate()
        ),
        Err(ConnectionError::ShellIntegrationUnavailable)
    ));

    let terminal = effective_config(
        connection
            .terminal_spec(&zmux_session_runtime::shell_integration::OpenRequestNonce::generate()),
    );
    assert_eq!(
        config_value(&terminal, "hostname"),
        Some("configured.invalid")
    );
    assert_eq!(config_value(&terminal, "user"), Some("configured-user"));
    assert_eq!(config_value(&terminal, "port"), Some("2202"));
    assert_eq!(
        config_value(&terminal, "proxycommand"),
        Some("/bin/echo configured-proxy")
    );
    assert_eq!(config_value(&terminal, "permitlocalcommand"), Some("yes"));
    assert_eq!(
        config_value(&terminal, "remotecommand"),
        Some("printf configured-remote")
    );

    let operation = effective_config(connection.operation_spec("true").unwrap());
    assert_eq!(
        config_value(&operation, "hostname"),
        Some("configured.invalid")
    );
    assert_eq!(config_value(&operation, "user"), Some("configured-user"));
    assert_eq!(config_value(&operation, "port"), Some("2202"));
    assert_eq!(
        config_value(&operation, "proxycommand"),
        Some("/usr/bin/false")
    );
    assert_eq!(config_value(&operation, "permitlocalcommand"), Some("no"));
    assert_eq!(config_value(&operation, "localcommand"), None);
    assert_eq!(config_value(&operation, "batchmode"), Some("yes"));
    assert_eq!(config_value(&operation, "controlmaster"), Some("false"));
    assert_eq!(config_value(&operation, "controlpersist"), Some("no"));
    assert_eq!(config_value(&operation, "requesttty"), Some("false"));

    fs::remove_file(config_path).unwrap();
}

#[test]
fn cleanup_never_recursively_removes_unexpected_entries() {
    let connection = ManagedConnection::create(descriptor("host")).unwrap();
    let root = connection.control_path().parent().unwrap();
    fs::write(connection.control_path(), b"socket placeholder").unwrap();
    let unexpected = root.join("keep");
    fs::write(&unexpected, b"must survive").unwrap();

    assert!(matches!(
        connection.cleanup(),
        Err(ConnectionError::RemoveDirectory { .. })
    ));
    assert_eq!(fs::read(&unexpected).unwrap(), b"must survive");

    fs::remove_file(unexpected).unwrap();
    connection.cleanup().unwrap();
}

#[test]
fn managed_ssh_shell_routes_nonce_bound_opener_requests_from_loopback_sshd() {
    let fixture = SshdFixture::new();
    let destination = format!("{}@127.0.0.1", fixture.user);
    let descriptor = SshDescriptor::builder(&destination)
        .unwrap()
        .config_file(fixture.client_config.to_str().unwrap())
        .port(fixture.port)
        .identity(fixture.identity.to_str().unwrap())
        .build()
        .unwrap();
    let connection = Arc::new(ManagedConnection::create(descriptor).unwrap());
    assert!(connection.shell_integration_enabled());

    let nonce = zmux_session_runtime::shell_integration::OpenRequestNonce::generate();
    let registration = zmux_session_runtime::shell_integration::OpenRequestRegistration::new(
        nonce,
        zmux_session_runtime::shell_integration::OpenRequestSource::ManagedSsh,
    );
    let spec = connection.terminal_spec(&nonce);
    let mut session = zmux_session_runtime::Session::spawn_with_open_requests(
        zmux_session_runtime::Launch {
            program: spec.program,
            args: spec.args,
            cwd: None,
            size: zmux_session_runtime::default_size(),
        },
        &[],
        &[],
        zmux_session_runtime::ShutdownPolicy::Kill,
        None,
        Some(registration),
        Arc::clone(&connection),
    )
    .unwrap();
    let active_registration = session
        .open_request_registration()
        .expect("managed SSH opener registration");
    assert!(
        active_registration.nonce() == nonce,
        "the PTY and managed SSH bootstrap must share one nonce"
    );
    assert!(matches!(
        active_registration.source(),
        zmux_session_runtime::shell_integration::OpenRequestSource::ManagedSsh
    ));

    wait_for_control_socket(connection.control_path(), &mut session);
    wait_for_remote_lifecycle_files(&connection, &nonce, &mut session);
    let remote_hooks = format!(
        "{}/plugin/hooks/hooks.json",
        nonce.remote_integration_directory()
    );
    let validate_hooks = connection
        .operation_spec(&format!("python3 -m json.tool '{remote_hooks}' >/dev/null"))
        .unwrap();
    let output = Command::new(&validate_hooks.program)
        .args(&validate_hooks.args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "managed SSH bootstrap published invalid Claude hook JSON: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stream_spec = connection.agent_lifecycle_spec(&nonce).unwrap();
    let mut stream_child = Command::new(&stream_spec.program)
        .args(&stream_spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let (stream_tx, stream_rx) = mpsc::sync_channel(128);
    let stream_stdout = stream_child.stdout.take().unwrap();
    thread::spawn(move || {
        for line in BufReader::new(stream_stdout).lines() {
            let Ok(line) = line else { break };
            if stream_tx.send(line).is_err() {
                break;
            }
        }
    });
    let hello = stream_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("managed lifecycle stream handshake");
    let hello: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(hello["kind"], "hello");
    assert_eq!(
        hello["version"],
        zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION
    );
    assert_eq!(hello["nonce"], nonce.remote_lifecycle_token());
    let generation = uuid::Uuid::new_v4();
    let mut stream_stdin = stream_child.stdin.take().unwrap();
    writeln!(
        stream_stdin,
        "{{\"kind\":\"activate\",\"version\":{},\"nonce\":\"{}\",\"generation\":\"{}\"}}",
        zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION,
        nonce.remote_lifecycle_token(),
        generation
    )
    .unwrap();
    let active = stream_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("managed lifecycle stream activation");
    let active: serde_json::Value = serde_json::from_str(&active).unwrap();
    assert_eq!(active["kind"], "active");
    assert_eq!(
        active["version"],
        zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION
    );
    assert_eq!(active["nonce"], nonce.remote_lifecycle_token());
    assert_eq!(active["generation"], generation.to_string());
    let remote_notice = || {
        let root = nonce.remote_integration_directory();
        let script = r#"import json,os,subprocess,sys; root,nonce=sys.argv[1:]; fifo=os.path.join(root,"attention"); os.path.exists(fifo) and os.unlink(fifo); os.mkfifo(fifo,0o600); os.chmod(fifo,0o600); reader=os.open(fifo,os.O_RDONLY|os.O_NONBLOCK); environment=os.environ.copy(); environment["ZMUX_AGENT_LIFECYCLE_NONCE"]=nonce; environment["ZMUX_SHELL_INTEGRATION_DIRECTORY"]=root; payload=json.dumps({"hook_event_name":"Notification","session_id":"00000000-0000-4000-8000-000000000002","notification_type":"permission_prompt"}); result=subprocess.run(["python3",os.path.join(root,"claude-lifecycle-notice.py"),"attention"],input=payload.encode(),env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE); result.returncode and sys.stderr.write(result.stderr.decode()); result.returncode and sys.exit(2); data=os.read(reader,128); data!=b"attention\n" and sys.exit(4); sys.stdout.write(data.decode())"#;
        let command = format!(
            "python3 -c '{}' '{}' '{}'",
            script,
            root,
            nonce.remote_lifecycle_token()
        );
        let spec = connection.operation_spec(&command).unwrap();
        let output = Command::new(&spec.program)
            .args(&spec.args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "remote notice state check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    assert_eq!(
        remote_notice(),
        "attention\n",
        "a live lifecycle transport must not suppress a notice without event admission"
    );

    let provider_session = uuid::Uuid::new_v4();
    let prompt_id = uuid::Uuid::new_v4();
    let mut last_stream_sequence = 0_u64;
    let (started, prompted) = {
        let mut push_remote_hook = |event: serde_json::Value| {
            let record = serde_json::json!({
                "nonce": nonce.remote_lifecycle_token(),
                "event_id": uuid::Uuid::new_v4(),
                "receipt_required": false,
                "receipt_requested": false,
                "event": event,
            });
            let record = serde_json::to_string(&record).unwrap();
            let command = format!(
                "printf '%s\\n' '{}' > '{}/agent-events'\n",
                record,
                nonce.remote_integration_directory()
            );
            session.send_input(command.as_bytes()).unwrap();
            let (frame, heartbeat_sequence) =
                receive_non_heartbeat_lifecycle_frame(&stream_rx, "remote provider Hook frame");
            if let Some(heartbeat_sequence) = heartbeat_sequence {
                last_stream_sequence = last_stream_sequence.max(heartbeat_sequence);
            }
            assert_eq!(frame["kind"], "hook");
            let sequence = frame["sequence"].as_u64().unwrap();
            assert!(sequence > last_stream_sequence);
            last_stream_sequence = sequence;
            frame
        };
        let started = push_remote_hook(serde_json::json!({
            "name":"SessionStart",
            "session_id":provider_session,
            "main_session":true,
            "source":"startup"
        }));
        let prompted = push_remote_hook(serde_json::json!({
            "name":"UserPromptSubmit",
            "session_id":provider_session,
            "main_session":true,
            "prompt_id":prompt_id,
            "permission_mode":"default"
        }));
        (started, prompted)
    };
    assert_eq!(started["kind"], "hook");
    assert!(started["sequence"].as_u64().unwrap() > 0);
    assert!(prompted["sequence"].as_u64().unwrap() > started["sequence"].as_u64().unwrap());
    assert_eq!(prompted["event"]["prompt_id"], prompt_id.to_string());

    let hook_id = uuid::Uuid::new_v4();
    let root = nonce.remote_integration_directory();
    let receipt_path = Path::new(&root).join(format!("r-{}", hook_id.simple()));
    let receipt_endpoint = UnixDatagram::bind(&receipt_path).expect("bind per-hook receipt socket");
    fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o600))
        .expect("secure per-hook receipt socket");
    // Mirrors RECEIPT_WAIT_SECONDS in the production Claude hook.
    const RECEIPT_WAIT_BUDGET: Duration = Duration::from_millis(1_800);
    let event = serde_json::json!({
        "name":"Notification",
        "session_id":provider_session,
        "main_session":true,
        "notification_type":"permission_prompt"
    });
    let record = serde_json::json!({
        "nonce":nonce.remote_lifecycle_token(),
        "event_id":hook_id,
        "receipt_required":true,
        "receipt_requested":true,
        "event":event.clone()
    });
    let writer_script = r#"import json,os,sys,time; root,record,budget=sys.argv[1:]; frame=json.loads(record); frame["receipt_deadline"]=time.monotonic()+float(budget); data=json.dumps(frame,separators=(",",":" )).encode()+b"\n"; fd=os.open(os.path.join(root,"agent-events"),os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC); os.write(fd,data); os.close(fd)"#;
    let write_command = format!(
        "python3 -c {} {} {} {}",
        shell_quote(writer_script),
        shell_quote(&root),
        shell_quote(&record.to_string()),
        shell_quote(&RECEIPT_WAIT_BUDGET.as_secs_f64().to_string())
    );
    let writer = connection.operation_spec(&write_command).unwrap();
    // This local deadline starts before SSH writer execution, so its budget also
    // includes setup time and is conservative relative to the remote event deadline.
    let receipt_budget_started = Instant::now();
    let receipt_deadline = receipt_budget_started + RECEIPT_WAIT_BUDGET;
    let output = Command::new(&writer.program)
        .args(&writer.args)
        .output()
        .expect("write receipt-requested event to remote lifecycle FIFO");
    assert!(
        output.status.success(),
        "remote event writer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (hook_frame, heartbeat_sequence) = receive_non_heartbeat_lifecycle_frame(
        &stream_rx,
        "receipt-requested remote provider Hook frame",
    );
    if let Some(heartbeat_sequence) = heartbeat_sequence {
        last_stream_sequence = last_stream_sequence.max(heartbeat_sequence);
    }
    assert_eq!(hook_frame["kind"], "hook");
    let hook_sequence = hook_frame["sequence"].as_u64().unwrap();
    assert!(hook_sequence > last_stream_sequence);
    last_stream_sequence = hook_sequence;
    assert_eq!(hook_frame["event_id"], hook_id.to_string());
    assert_eq!(hook_frame["receipt_requested"], true);
    assert_eq!(hook_frame["generation"], generation.to_string());

    let stale_receipt = zmux_remote_runtime::agent_lifecycle::ClientFrame::Receipt {
        version: zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION,
        nonce: nonce.remote_lifecycle_token().parse().unwrap(),
        generation: uuid::Uuid::new_v4(),
        event_id: hook_id,
        disposition: zmux_remote_runtime::agent_lifecycle::HookDisposition::AlertCovered,
    };
    writeln!(
        stream_stdin,
        "{}",
        serde_json::to_string(&stale_receipt).unwrap()
    )
    .unwrap();
    let mut datagram = [0; 512];
    receipt_endpoint
        .set_read_timeout(Some(Duration::from_millis(250)))
        .expect("bound stale-generation negative-control wait");
    assert!(matches!(
        receipt_endpoint.recv(&mut datagram),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            )
    ));

    let remaining_before_send = receipt_deadline.saturating_duration_since(Instant::now());
    assert!(
        !remaining_before_send.is_zero(),
        "local receipt deadline expired before valid receipt send (elapsed={}ms; remaining={}ms)",
        receipt_budget_started.elapsed().as_millis(),
        remaining_before_send.as_millis()
    );
    let valid_receipt = zmux_remote_runtime::agent_lifecycle::ClientFrame::Receipt {
        version: zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION,
        nonce: nonce.remote_lifecycle_token().parse().unwrap(),
        generation,
        event_id: hook_id,
        disposition: zmux_remote_runtime::agent_lifecycle::HookDisposition::HandledNoAlert,
    };
    writeln!(
        stream_stdin,
        "{}",
        serde_json::to_string(&valid_receipt).unwrap()
    )
    .unwrap();
    let remaining_for_read = receipt_deadline.saturating_duration_since(Instant::now());
    assert!(
        !remaining_for_read.is_zero(),
        "local receipt deadline expired after valid receipt send (elapsed={}ms; remaining={}ms)",
        receipt_budget_started.elapsed().as_millis(),
        remaining_for_read.as_millis()
    );
    receipt_endpoint
        .set_read_timeout(Some(remaining_for_read))
        .expect("bound valid-receipt read to remaining absolute deadline");
    let length = receipt_endpoint.recv(&mut datagram).unwrap_or_else(|error| {
        let remaining = receipt_deadline.saturating_duration_since(Instant::now());
        panic!(
            "valid exact event receipt did not reach its per-hook socket before the local deadline (elapsed={}ms; remaining={}ms; receive error: {error})",
            receipt_budget_started.elapsed().as_millis(),
            remaining.as_millis()
        )
    });
    let receipt: serde_json::Value = serde_json::from_slice(&datagram[..length]).unwrap();
    assert_eq!(receipt["kind"], "receipt");
    assert_eq!(receipt["event_id"], hook_id.to_string());
    assert_eq!(receipt["generation"], generation.to_string());
    assert_eq!(receipt["disposition"], "handled_no_alert");
    assert_eq!(
        uuid::Uuid::parse_str(receipt["nonce"].as_str().unwrap()).unwrap(),
        nonce
            .remote_lifecycle_token()
            .parse::<uuid::Uuid>()
            .unwrap()
    );
    drop(receipt_endpoint);
    fs::remove_file(&receipt_path).expect("remove per-hook receipt socket");

    let endpoint_collision_id = uuid::Uuid::new_v4();
    let endpoint_collision_script = r#"import importlib.util,os,socket,sys; root,event_id=sys.argv[1:]; path=os.path.join(root,"r-"+event_id.replace("-","")); owner=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM); owner.bind(path); os.chmod(path,0o600); before=os.stat(path,follow_symlinks=False); spec=importlib.util.spec_from_file_location("lifecycle_hook",os.path.join(root,"claude-lifecycle-hook.py")); module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module); failed=[False]; exec("try: module.receipt_endpoint(root,event_id)\nexcept OSError: failed[0]=True"); after=os.stat(path,follow_symlinks=False); (before.st_dev,before.st_ino)!=(after.st_dev,after.st_ino) and sys.exit(4); (not failed[0]) and sys.exit(3); owner.close(); os.unlink(path)"#;
    let collision_command = format!(
        "python3 -c {} {} {}",
        shell_quote(endpoint_collision_script),
        shell_quote(&root),
        shell_quote(&endpoint_collision_id.to_string())
    );
    let collision = connection.operation_spec(&collision_command).unwrap();
    let output = Command::new(&collision.program)
        .args(&collision.args)
        .output()
        .expect("run receipt endpoint collision and admission-fence regression");
    assert!(
        output.status.success(),
        "receipt endpoint collision damaged an existing socket: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let queued_event_ids: Vec<_> = (0
        ..zmux_remote_runtime::agent_lifecycle::MAX_PENDING_HOOK_RECEIPTS)
        .map(|_| uuid::Uuid::new_v4())
        .collect();
    let mut queued_receipts = Vec::with_capacity(queued_event_ids.len());
    for event_id in &queued_event_ids {
        let path = Path::new(&root).join(format!("r-{}", event_id.simple()));
        let endpoint = UnixDatagram::bind(&path).expect("bind bounded queue receipt socket");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("secure bounded queue receipt socket");
        endpoint
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bound queue receipt wait");
        queued_receipts.push((*event_id, endpoint, path));
    }
    let queue_writer_script = r#"import json,os,select,sys,time
root,nonce,ids=sys.argv[1:]
deadline=time.monotonic()+5.0
fd=os.open(os.path.join(root,"agent-events"),os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
event={"name":"Notification","session_id":"00000000-0000-4000-8000-000000000002","main_session":True,"notification_type":"permission_prompt"}
try:
    for event_id in json.loads(ids):
        record={"nonce":nonce,"event_id":event_id,"receipt_required":True,"receipt_requested":True,"receipt_deadline":time.monotonic()+2.9,"event":event}
        data=(json.dumps(record,separators=(",",":"))+"\n").encode()
        while True:
            try:
                if os.write(fd,data)!=len(data):
                    raise OSError("short lifecycle FIFO write")
                break
            except BlockingIOError:
                remaining=deadline-time.monotonic()
                if remaining<=0 or not select.select([], [fd], [], remaining)[1]:
                    raise TimeoutError("bounded receipt fixture FIFO write expired")
finally:
    os.close(fd)
"#;
    let queued_event_ids_json = serde_json::to_string(&queued_event_ids).unwrap();
    let write_command = python_exec_command(
        queue_writer_script,
        &[
            &root,
            &nonce.remote_lifecycle_token(),
            &queued_event_ids_json,
        ],
    );
    let writer = connection.operation_spec(&write_command).unwrap();
    let output = Command::new(&writer.program)
        .args(&writer.args)
        .output()
        .expect("fill the remote stream's bounded receipt queue");
    assert!(
        output.status.success(),
        "receipt queue writer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for expected_id in queued_event_ids
        .iter()
        .take(zmux_remote_runtime::agent_lifecycle::MAX_PENDING_HOOK_RECEIPTS)
    {
        let (frame, heartbeat_sequence) =
            receive_non_heartbeat_lifecycle_frame(&stream_rx, "bounded receipt queue Hook frame");
        if let Some(heartbeat_sequence) = heartbeat_sequence {
            last_stream_sequence = last_stream_sequence.max(heartbeat_sequence);
        }
        assert_eq!(frame["kind"], "hook");
        let sequence = frame["sequence"].as_u64().unwrap();
        assert!(sequence > last_stream_sequence);
        last_stream_sequence = sequence;
        assert_eq!(frame["event_id"], expected_id.to_string());
        assert_eq!(frame["receipt_requested"], true);
    }
    for (_, endpoint, path) in queued_receipts {
        drop(endpoint);
        fs::remove_file(path).expect("remove bounded queue receipt socket");
    }

    let overflow_hook_script = r#"import json,os,shutil,subprocess,sys,uuid
source_root=sys.argv[1]
nonce=uuid.uuid4().hex
root=f"/tmp/zmux-shell-{nonce}"
descriptors=[]
created=False
try:
    os.mkdir(root,0o700)
    created=True
    hook_path=os.path.join(root,"claude-lifecycle-hook.py")
    shutil.copyfile(os.path.join(source_root,"claude-lifecycle-hook.py"),hook_path)
    for name in ("agent-events","agent-events-loss","attention"):
        path=os.path.join(root,name)
        os.mkfifo(path,0o600)
        os.chmod(path,0o600)
    event_path=os.path.join(root,"agent-events")
    loss_path=os.path.join(root,"agent-events-loss")
    attention_path=os.path.join(root,"attention")
    event_reader=os.open(event_path,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(event_reader)
    event_writer=os.open(event_path,os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(event_writer)
    loss_reader=os.open(loss_path,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(loss_reader)
    loss_writer=os.open(loss_path,os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(loss_writer)
    attention_reader=os.open(attention_path,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(attention_reader)
    attention_writer=os.open(attention_path,os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
    descriptors.append(attention_writer)
    total=0
    saturated=False
    while total<1048576:
        try:
            total+=os.write(event_writer,b"x"*4096)
        except BlockingIOError:
            saturated=True
            break
    if not saturated:
        sys.exit(2)
    environment=os.environ.copy()
    environment["ZMUX_AGENT_LIFECYCLE_NONCE"]=nonce
    environment["ZMUX_SHELL_INTEGRATION_DIRECTORY"]=root
    payload=json.dumps({"hook_event_name":"Notification","session_id":"00000000-0000-4000-8000-000000000002","notification_type":"permission_prompt"}).encode()
    result=subprocess.run(["python3",hook_path],input=payload,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
    if result.returncode!=0:
        sys.stderr.write(result.stderr.decode(errors="replace"))
        sys.exit(3)
    if os.read(loss_reader,1)!=b"!":
        sys.exit(4)
    if os.read(attention_reader,128)!=b"attention_silent\n":
        sys.exit(5)
    written=b""
    while True:
        try:
            chunk=os.read(event_reader,8192)
        except BlockingIOError:
            break
        if not chunk:
            break
        written+=chunk
    if b'"event_id"' in written:
        sys.exit(6)
finally:
    for descriptor in descriptors:
        try:
            os.close(descriptor)
        except OSError:
            pass
    if created:
        shutil.rmtree(root,ignore_errors=True)
"#;
    let overflow_command = python_exec_command(overflow_hook_script, &[&root]);
    let overflow = connection.operation_spec(&overflow_command).unwrap();
    let output = Command::new(&overflow.program)
        .args(&overflow.args)
        .output()
        .expect("run production hook FIFO-overflow fallback regression");
    assert!(
        output.status.success(),
        "nonblocking hook overflow did not signal evidence loss and render its fallback: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    session
        .send_input(b"open https://ssh.example.test/login\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let opened = loop {
        match session.take_open_urls() {
            Ok(urls) if !urls.is_empty() => break urls,
            Ok(_) | Err(zmux_session_runtime::SessionError::Busy) => {}
            Err(error) => panic!("managed SSH session stopped before opener request: {error}"),
        }
        let snapshot = session.snapshot().unwrap();
        assert_ne!(
            snapshot.state,
            zmux_session_runtime::State::Failed,
            "managed SSH shell failed: {:?}",
            snapshot.failure
        );
        assert!(
            Instant::now() < deadline,
            "managed SSH opener request was not relayed"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].as_str(), "https://ssh.example.test/login");

    // Make the primary FIFO unsafe while the stream reader and its dedicated
    // loss-signal FIFO remain live. The hook reports loss and the reader fences
    // this generation instead of continuing with stale provider evidence.
    let root = nonce.remote_integration_directory();
    let lifecycle_token = nonce.remote_lifecycle_token();
    let hook_input =
        format!("{{\"hook_event_name\":\"SessionStart\",\"session_id\":\"{provider_session}\"}}");
    let drop_hook_command = format!(
        "chmod 0666 '{root}/agent-events' && printf '%s\\n' '{hook_input}' | ZMUX_AGENT_LIFECYCLE_NONCE={lifecycle_token} ZMUX_SHELL_INTEGRATION_DIRECTORY='{root}' python3 '{root}/claude-lifecycle-hook.py'"
    );
    let drop_hook = connection.operation_spec(&drop_hook_command).unwrap();
    let output = Command::new(&drop_hook.program)
        .args(&drop_hook.args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "remote hook loss fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (gap, heartbeat_sequence) =
        receive_non_heartbeat_lifecycle_frame(&stream_rx, "remote hook loss Gap frame");
    if let Some(heartbeat_sequence) = heartbeat_sequence {
        last_stream_sequence = last_stream_sequence.max(heartbeat_sequence);
    }
    assert_eq!(gap["kind"], "gap");
    assert!(gap["sequence"].as_u64().unwrap() > last_stream_sequence);
    assert_eq!(gap["generation"], generation.to_string());
    drop(stream_stdin);
    assert_eq!(remote_notice(), "attention\n");

    let _ = stream_child.kill();
    let _ = stream_child.wait();
    session.shutdown(Duration::from_secs(5)).unwrap();
    zmux_remote_runtime::connection::request_cleanup_barrier()
        .unwrap()
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
}

#[test]
fn required_remote_receipt_failures_emit_a_generation_gap() {
    use zmux_remote_runtime::agent_lifecycle::MAX_PENDING_HOOK_RECEIPTS;

    for deadline_seconds in [2.5, -1.0] {
        let mut stream = LocalLifecycleStream::start();
        let event_id = uuid::Uuid::new_v4();
        let endpoint = if deadline_seconds < 0.0 {
            let path = stream.receipt_path(event_id);
            let socket = UnixDatagram::bind(&path).expect("bind expired receipt endpoint");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("secure expired receipt endpoint");
            socket
                .set_read_timeout(Some(Duration::from_millis(50)))
                .expect("bound expired receipt assertion");
            Some(socket)
        } else {
            None
        };
        stream.write_events(&[event_id], deadline_seconds);
        let gap = stream.read_until_gap();
        assert_eq!(gap["kind"], "gap");
        assert_eq!(
            gap["version"],
            zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION
        );
        assert_eq!(gap["nonce"], stream.nonce_token);
        assert_eq!(gap["generation"], stream.generation.to_string());
        assert!(gap["sequence"].as_u64().unwrap() > 0);
        assert_eq!(stream.wait_status().code(), Some(74));
        if let Some(endpoint) = endpoint {
            let mut receipt = [0; 512];
            assert!(matches!(
                endpoint.recv(&mut receipt),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    )
            ));
        }
    }

    let mut stream = LocalLifecycleStream::start();
    let event_ids: Vec<_> = (0..=MAX_PENDING_HOOK_RECEIPTS)
        .map(|_| uuid::Uuid::new_v4())
        .collect();
    let mut endpoints = Vec::with_capacity(event_ids.len());
    for event_id in &event_ids {
        let path = stream.receipt_path(*event_id);
        let endpoint = UnixDatagram::bind(&path).expect("bind queued receipt endpoint");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("secure queued receipt endpoint");
        endpoints.push(endpoint);
    }
    stream.write_events(&event_ids, 2.5);
    let mut last_sequence = 0;
    for event_id in event_ids.iter().take(MAX_PENDING_HOOK_RECEIPTS) {
        let frame = stream.read_until_hook_or_gap();
        assert_eq!(frame["kind"], "hook");
        assert_eq!(frame["event_id"], event_id.to_string());
        assert_eq!(frame["receipt_requested"], true);
        let sequence = frame["sequence"].as_u64().unwrap();
        assert!(sequence > last_sequence);
        last_sequence = sequence;
    }
    let gap = stream.read_until_gap();
    assert_eq!(gap["kind"], "gap");
    assert_eq!(gap["generation"], stream.generation.to_string());
    assert!(gap["sequence"].as_u64().unwrap() > last_sequence);
    assert_ne!(
        gap["event_id"],
        event_ids[MAX_PENDING_HOOK_RECEIPTS].to_string()
    );
    assert_eq!(stream.wait_status().code(), Some(74));
}

#[test]
fn configured_remote_hooks_fallback_for_unreadable_payloads_and_ignore_other_notifications() {
    for (hook_name, matcher, payload, fallback) in [
        ("Stop", None, b"{".to_vec(), "stop_silent\n"),
        (
            "Notification",
            Some("permission_prompt"),
            vec![b'x'; 65_537],
            "attention_silent\n",
        ),
    ] {
        let mut stream = LocalLifecycleStream::start();
        let command = LocalLifecycleStream::configured_hook_command(hook_name, matcher);
        assert_eq!(stream.run_configured_hook(&command, &payload), fallback);
        let gap = stream.read_until_gap();
        assert_eq!(gap["kind"], "gap");
        assert_eq!(gap["generation"], stream.generation.to_string());
        assert_eq!(stream.wait_status().code(), Some(74));
    }

    let mut stream = LocalLifecycleStream::start();
    let valid_other_notification = serde_json::json!({
        "hook_event_name":"Notification",
        "session_id":"00000000-0000-4000-8000-000000000002",
        "notification_type":"auth_success"
    })
    .to_string()
    .into_bytes();
    let generic_command = LocalLifecycleStream::configured_hook_command("Notification", None);
    assert_eq!(
        stream.run_configured_hook(&generic_command, &valid_other_notification),
        ""
    );
    let frame = stream.read_until_hook_or_gap();
    assert_eq!(frame["kind"], "hook");
    assert_eq!(frame["event"]["notification_type"], "auth_success");

    let matcher_command =
        LocalLifecycleStream::configured_hook_command("Notification", Some("permission_prompt"));
    assert_eq!(
        stream.run_configured_hook(&matcher_command, &valid_other_notification),
        ""
    );
    assert!(matches!(
        stream.stdout.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let mut stream = LocalLifecycleStream::start();
    let valid_attention = serde_json::json!({
        "hook_event_name":"Notification",
        "session_id":"00000000-0000-4000-8000-000000000002",
        "notification_type":"permission_prompt"
    })
    .to_string()
    .into_bytes();
    assert_eq!(
        stream.run_configured_hook(&generic_command, &valid_attention),
        "attention_silent\n"
    );
    let frame = stream.read_until_hook_or_gap();
    assert_eq!(frame["kind"], "hook");
    assert_eq!(frame["event"]["notification_type"], "permission_prompt");
    assert_eq!(frame["receipt_requested"], true);
    assert_eq!(
        stream.run_configured_hook(&matcher_command, &valid_attention),
        ""
    );
    assert!(matches!(
        stream.stdout.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

struct LocalLifecycleStream {
    _root: FixtureRoot,
    root_path: PathBuf,
    nonce_token: String,
    generation: uuid::Uuid,
    child: Child,
    stdout: mpsc::Receiver<String>,
    _stdin: ChildStdin,
}

impl LocalLifecycleStream {
    fn start() -> Self {
        let nonce = zmux_session_runtime::shell_integration::OpenRequestNonce::generate();
        let nonce_token = nonce.remote_lifecycle_token();
        let root_path = PathBuf::from(nonce.remote_integration_directory());
        let root = FixtureRoot::new(root_path.clone());
        for name in ["agent-events", "agent-events-loss", "attention"] {
            let path = root_path.join(name);
            let status = Command::new("mkfifo")
                .arg(&path)
                .status()
                .expect("mkfifo is available for the local lifecycle stream fixture");
            assert!(status.success(), "create lifecycle FIFO {path:?}");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("secure lifecycle FIFO");
        }
        let hook_source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../session-runtime/src/shell/claude-lifecycle-hook.py");
        let hook_destination = root_path.join("claude-lifecycle-hook.py");
        fs::copy(hook_source, &hook_destination).expect("install production lifecycle hook helper");
        fs::set_permissions(&hook_destination, fs::Permissions::from_mode(0o700))
            .expect("secure production lifecycle hook helper");

        let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../session-runtime/src/shell/claude-lifecycle-stream.py");
        let mut child = Command::new("python3")
            .arg(script_path)
            .arg(&nonce_token)
            .env("ZMUX_SHELL_INTEGRATION_DIRECTORY", &root_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start local production lifecycle stream helper");
        let stdout = child.stdout.take().unwrap();
        let (stdout_sender, stdout_receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if stdout_sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut stream = Self {
            _root: root,
            root_path,
            nonce_token,
            generation: uuid::Uuid::new_v4(),
            _stdin: child.stdin.take().unwrap(),
            stdout: stdout_receiver,
            child,
        };
        let hello = stream.read_frame();
        assert_eq!(hello["kind"], "hello");
        assert_eq!(hello["nonce"], stream.nonce_token);
        writeln!(
            stream._stdin,
            "{}",
            serde_json::to_string(
                &zmux_remote_runtime::agent_lifecycle::ClientFrame::Activate {
                    version: zmux_remote_runtime::agent_lifecycle::AGENT_LIFECYCLE_PROTOCOL_VERSION,
                    nonce: stream.nonce_token.parse().unwrap(),
                    generation: stream.generation,
                }
            )
            .unwrap()
        )
        .unwrap();
        let active = stream.read_frame();
        assert_eq!(active["kind"], "active");
        assert_eq!(active["generation"], stream.generation.to_string());
        stream
    }

    fn receipt_path(&self, event_id: uuid::Uuid) -> PathBuf {
        self.root_path.join(format!("r-{}", event_id.simple()))
    }

    fn configured_hook_command(hook_name: &str, matcher: Option<&str>) -> String {
        let config: serde_json::Value = serde_json::from_str(include_str!(
            "../../session-runtime/src/shell/claude-hooks-remote.json"
        ))
        .expect("parse production remote Claude hook configuration");
        let groups = config["hooks"][hook_name]
            .as_array()
            .expect("configured remote hook event");
        let group = groups
            .iter()
            .find(|group| match matcher {
                Some(matcher) => group["matcher"] == matcher,
                None => group.get("matcher").is_none(),
            })
            .expect("configured remote hook matcher");
        group["hooks"][0]["command"]
            .as_str()
            .expect("configured remote hook command")
            .to_owned()
    }

    fn run_configured_hook(&self, command: &str, payload: &[u8]) -> String {
        let runner = r#"import os,select,subprocess,sys
root,nonce,command=sys.argv[1:]
attention=os.path.join(root,"attention")
reader=os.open(attention,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
hold=os.open(attention,os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
environment=os.environ.copy()
environment["ZMUX_AGENT_LIFECYCLE_NONCE"]=nonce
environment["ZMUX_SHELL_INTEGRATION_DIRECTORY"]=root
payload=sys.stdin.buffer.read()
result=subprocess.run(["/bin/sh","-c",command],input=payload,env=environment,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
if result.returncode:
    sys.stderr.buffer.write(result.stderr)
    sys.exit(result.returncode)
data=os.read(reader,128) if select.select([reader],[],[],0.1)[0] else b""
sys.stdout.buffer.write(data)
os.close(reader)
os.close(hold)
"#;
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(runner)
            .arg(&self.root_path)
            .arg(&self.nonce_token)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run production configured remote hook command");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload)
            .expect("write payload to configured remote hook");
        let output = child
            .wait_with_output()
            .expect("wait for configured remote hook command");
        assert!(
            output.status.success(),
            "configured remote hook failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("decode silent fallback token")
    }

    fn write_events(&self, event_ids: &[uuid::Uuid], deadline_seconds: f64) {
        let ids = serde_json::to_string(event_ids).unwrap();
        let script = r#"import json,os,select,sys,time
root,nonce,ids,deadline=sys.argv[1:]
event={"name":"Notification","session_id":"00000000-0000-4000-8000-000000000002","main_session":True,"notification_type":"permission_prompt"}
fd=os.open(os.path.join(root,"agent-events"),os.O_WRONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC)
try:
    for event_id in json.loads(ids):
        record={"nonce":nonce,"event_id":event_id,"receipt_required":True,"receipt_requested":True,"receipt_deadline":time.monotonic()+float(deadline),"event":event}
        data=(json.dumps(record,separators=(",",":"))+"\n").encode()
        while True:
            try:
                if os.write(fd,data)!=len(data):
                    raise OSError("short lifecycle FIFO write")
                break
            except BlockingIOError:
                if not select.select([], [fd], [], 5.0)[1]:
                    raise TimeoutError("lifecycle FIFO write expired")
finally:
    os.close(fd)
"#;
        let output = Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(&self.root_path)
            .arg(&self.nonce_token)
            .arg(ids)
            .arg(deadline_seconds.to_string())
            .output()
            .expect("write lifecycle test records");
        assert!(
            output.status.success(),
            "lifecycle FIFO writer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn read_frame(&mut self) -> serde_json::Value {
        let line = self
            .stdout
            .recv_timeout(Duration::from_secs(5))
            .expect("read lifecycle stream frame");
        serde_json::from_str(&line).expect("decode lifecycle stream frame")
    }

    fn read_until_hook_or_gap(&mut self) -> serde_json::Value {
        let (frame, _) = receive_non_heartbeat_lifecycle_frame(
            &self.stdout,
            "local lifecycle Hook or Gap frame",
        );
        frame
    }

    fn read_until_gap(&mut self) -> serde_json::Value {
        let (frame, _) =
            receive_non_heartbeat_lifecycle_frame(&self.stdout, "local lifecycle Gap frame");
        assert_ne!(frame["kind"], "hook", "dropped hook must not be admitted");
        assert_eq!(frame["kind"], "gap", "expected a local lifecycle Gap frame");
        frame
    }

    fn wait_status(&mut self) -> std::process::ExitStatus {
        self.child.wait().expect("wait for fenced lifecycle stream")
    }
}

impl Drop for LocalLifecycleStream {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct SshdFixture {
    _root: FixtureRoot,
    child: Child,
    user: String,
    port: u16,
    client_config: PathBuf,
    identity: PathBuf,
}

impl SshdFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let root = FixtureRoot::new(std::env::temp_dir().join(format!(
            "zmux-sshd-fixture-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        let remote_home = root.join("remote-home");
        fs::create_dir(&remote_home).unwrap();
        fs::set_permissions(&remote_home, fs::Permissions::from_mode(0o700)).unwrap();

        let host_key = root.join("host_key");
        let identity = root.join("client_key");
        for path in [&host_key, &identity] {
            let status = Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", ""])
                .arg("-f")
                .arg(path)
                .status()
                .expect("ssh-keygen is required for the isolated SSH fixture");
            assert!(status.success(), "ssh-keygen failed for fixture key");
        }
        let user = Command::new("id")
            .arg("-un")
            .output()
            .expect("id is available for the SSH fixture")
            .stdout;
        let user = String::from_utf8(user).unwrap().trim().to_owned();
        let identity_public = fs::read_to_string(identity.with_extension("pub")).unwrap();
        let public_key = identity_public
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let authorized_keys = root.join("authorized_keys");
        fs::write(
            &authorized_keys,
            format!(
                "environment=\"HOME={}\",environment=\"SHELL=/bin/sh\" {public_key} zmux fixture\n",
                remote_home.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&authorized_keys, fs::Permissions::from_mode(0o600)).unwrap();

        let client_config = root.join("ssh_config");
        let port = (0..32)
            .find_map(|_| reserve_local_port())
            .expect("an unused loopback port is available");
        let host_public = fs::read_to_string(host_key.with_extension("pub")).unwrap();
        let host_key_fields = host_public
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let known_hosts = root.join("known_hosts");
        fs::write(
            &known_hosts,
            format!("[127.0.0.1]:{port} {host_key_fields}\n"),
        )
        .unwrap();
        fs::write(
            &client_config,
            format!(
                "Host *\n  BatchMode yes\n  IdentitiesOnly yes\n  PreferredAuthentications publickey\n  PasswordAuthentication no\n  KbdInteractiveAuthentication no\n  StrictHostKeyChecking yes\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n  ConnectTimeout 5\n  LogLevel ERROR\n",
                known_hosts.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&client_config, fs::Permissions::from_mode(0o600)).unwrap();

        let server_config = root.join("sshd_config");
        fs::write(
            &server_config,
            format!(
                "Port {port}\nAddressFamily inet\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\nAuthorizedKeysFile {}\nStrictModes no\nPubkeyAuthentication yes\nAuthenticationMethods publickey\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin yes\nPermitUserEnvironment yes\nPermitTTY yes\nUsePAM no\nAllowTcpForwarding no\nPrintMotd no\nPrintLastLog no\nLogLevel ERROR\nSubsystem sftp internal-sftp\n",
                host_key.display(),
                root.join("sshd.pid").display(),
                authorized_keys.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&server_config, fs::Permissions::from_mode(0o600)).unwrap();
        let log = fs::File::create(root.join("sshd.log")).unwrap();
        let mut child = Command::new("/usr/sbin/sshd")
            .arg("-D")
            .arg("-e")
            .arg("-f")
            .arg(&server_config)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .expect("/usr/sbin/sshd is required for the isolated SSH fixture");
        if let Err(error) = wait_for_sshd(port, &mut child) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{error}");
        }

        Self {
            _root: root,
            child,
            user,
            port,
            client_config,
            identity,
        }
    }
}

impl Drop for SshdFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new(path: PathBuf) -> Self {
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}

impl Deref for FixtureRoot {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for FixtureRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reserve_local_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|listener| listener.local_addr().ok().map(|address| address.port()))
}

fn wait_for_sshd(port: u16, child: &mut Child) -> Result<(), &'static str> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child
            .try_wait()
            .map_err(|_| "failed to observe isolated sshd")?
            .is_some()
        {
            return Err("isolated sshd exited before listening");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("isolated sshd did not listen");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn python_exec_command(script: &str, arguments: &[&str]) -> String {
    let launcher = "import json,sys; original=sys.argv; sys.argv=['zmux-test']+original[2:]; exec(json.loads(original[1]))";
    let script_argument = serde_json::to_string(script).unwrap();
    let mut command = format!(
        "python3 -c {} {}",
        shell_quote(launcher),
        shell_quote(&script_argument)
    );
    for argument in arguments {
        command.push(' ');
        command.push_str(&shell_quote(argument));
    }
    command
}

fn wait_for_control_socket(path: &Path, session: &mut zmux_session_runtime::Session) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if path.exists() {
            return;
        }
        let snapshot = session.snapshot().unwrap();
        assert_ne!(
            snapshot.state,
            zmux_session_runtime::State::Failed,
            "managed SSH session failed before opening its control socket: {:?}",
            snapshot.failure
        );
        assert!(
            Instant::now() < deadline,
            "managed SSH control socket was not ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_remote_lifecycle_files(
    connection: &ManagedConnection,
    nonce: &zmux_session_runtime::shell_integration::OpenRequestNonce,
    session: &mut zmux_session_runtime::Session,
) {
    let root = nonce.remote_integration_directory();
    let command = format!(
        "test -f {root}/claude-lifecycle-stream.py && test -p {root}/agent-events && test -p {root}/agent-events-loss"
    );
    let spec = connection.operation_spec(&command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ready = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if ready {
            return;
        }
        let snapshot = session.snapshot().unwrap();
        assert_ne!(
            snapshot.state,
            zmux_session_runtime::State::Failed,
            "managed SSH session failed before lifecycle files were ready: {:?}",
            snapshot.failure
        );
        assert!(
            Instant::now() < deadline,
            "managed SSH lifecycle files were not ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn descriptor(destination: &str) -> SshDescriptor {
    SshDescriptor::builder(destination)
        .unwrap()
        .build()
        .unwrap()
}

fn effective_config(spec: ProcessSpec) -> String {
    let output = Command::new(&spec.program)
        .arg("-G")
        .args(spec.args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "ssh -G failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn config_value<'a>(config: &'a str, key: &str) -> Option<&'a str> {
    config
        .lines()
        .find_map(|line| line.split_once(' ').filter(|(name, _)| *name == key))
        .map(|(_, value)| value)
}

#[test]
fn asynchronous_cleanup_barrier_retains_failed_path_removal_diagnostics() {
    let connection = ManagedConnection::create(descriptor("host")).unwrap();
    let root = connection.control_path().parent().unwrap().to_owned();
    let unexpected = root.join("keep");
    fs::write(&unexpected, b"must survive failed cleanup").unwrap();
    drop(connection);
    let report = zmux_remote_runtime::connection::request_cleanup_barrier()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains(root.to_str().unwrap())),
        "barrier hid failed cleanup: {report:?}"
    );
    assert_eq!(
        fs::read(&unexpected).unwrap(),
        b"must survive failed cleanup"
    );
    // Later queue drainage cannot erase an unresolved cleanup failure.
    let repeated = zmux_remote_runtime::connection::request_cleanup_barrier()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(
        repeated
            .issues
            .iter()
            .any(|issue| issue.contains(root.to_str().unwrap()))
    );
    fs::remove_file(unexpected).unwrap();
    fs::remove_dir(root).unwrap();
}
