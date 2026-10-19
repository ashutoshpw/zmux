use std::{
    fs,
    net::{TcpListener, TcpStream},
    ops::Deref,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_remote_runtime::{
    Forwarding, JumpHost, ListenEndpoint, MAX_COMMAND_BYTES, ProcessSpec, SSH_PROGRAM,
    SshDescriptor,
    connection::{ConnectionError, ManagedConnection},
};

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
    let connection = ManagedConnection::create(descriptor).unwrap();
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
        .config_file("/tmp/config")
        .port(2222)
        .identity("/tmp/key")
        .jump_host(JumpHost::parse("bastion", None).unwrap())
        .build()
        .unwrap();
    let connection = ManagedConnection::create(descriptor).unwrap();

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
                "--".into(),
                "deploy@prod-alias".into(),
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
    assert_eq!(config_value(&operation, "hostname"), Some("prod-alias"));
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
    let connection = ManagedConnection::create(descriptor).unwrap();
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
        connection,
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
    session.shutdown(Duration::from_secs(5)).unwrap();
    zmux_remote_runtime::connection::request_cleanup_barrier()
        .unwrap()
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
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
