use std::{fs, os::unix::fs::PermissionsExt, process::Command};
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
    let spec = connection.terminal_spec();

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
        "Host prod-alias\n  HostName configured.invalid\n  User configured-user\n  Port 2202\n  ProxyCommand /bin/echo configured-proxy\n  PermitLocalCommand yes\n  LocalCommand /bin/echo configured-local\n",
    )
    .unwrap();
    let descriptor = SshDescriptor::builder("prod-alias")
        .unwrap()
        .config_file(config_path.to_str().unwrap())
        .build()
        .unwrap();
    let connection = ManagedConnection::create(descriptor).unwrap();

    let terminal = effective_config(connection.terminal_spec());
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
