use zmux_remote_runtime::{
    ConnectEndpoint, DescriptorError, Forwarding, HostKeyPolicy, JumpHost, ListenEndpoint,
    MAX_ARGUMENT_BYTES, MAX_COMMAND_ARGS, MAX_FORWARDINGS, MAX_JUMP_HOSTS, MAX_SSH_OPTIONS,
    SshDescriptor, SshDestination, SshHost, SshOption,
};

#[test]
fn destinations_cover_config_aliases_users_and_ip_literals() {
    let alias = SshDestination::parse("deploy@production_blue").unwrap();
    assert_eq!(alias.user(), Some("deploy"));
    assert_eq!(alias.host().as_str(), "production_blue");
    assert_eq!(alias.to_string(), "deploy@production_blue");

    assert_eq!(
        SshDestination::parse("192.0.2.12").unwrap().to_string(),
        "192.0.2.12"
    );
    assert_eq!(
        SshDestination::parse("ops@[2001:0db8::5]")
            .unwrap()
            .to_string(),
        "ops@2001:db8::5"
    );
}

#[test]
fn malformed_or_option_shaped_destinations_fail_before_process_creation() {
    for destination in [
        "",
        "-oProxyCommand=evil",
        "user@@host",
        "user name@host",
        "user@host name",
        "host:2222",
        "[example.com]",
        "host\n-oProxyCommand=evil",
    ] {
        assert!(
            SshDescriptor::builder(destination).is_err(),
            "accepted {destination:?}"
        );
    }
}

#[test]
fn terminal_process_and_future_operations_share_one_endpoint_route() {
    let descriptor = SshDescriptor::builder("deploy@prod-alias")
        .unwrap()
        .config_file("/tmp/fixture ssh/config")
        .port(2222)
        .identity("/Users/me/.ssh/team key")
        .jump_host(JumpHost::parse("bastion", Some(2200)).unwrap())
        .jump_host(JumpHost::parse("jump@[2001:db8::7]", None).unwrap())
        .host_key_policy(HostKeyPolicy::AcceptNew)
        .option(SshOption::parse("Compression=yes").unwrap())
        .option(SshOption::parse("ServerAliveInterval=30").unwrap())
        .forwarding(Forwarding::Agent { enabled: true })
        .forwarding(Forwarding::Local {
            listen: ListenEndpoint::parse(Some("127.0.0.1"), 8080).unwrap(),
            target: ConnectEndpoint::parse("service.internal", 80).unwrap(),
        })
        .forwarding(Forwarding::Remote {
            listen: ListenEndpoint::parse(None, 9000).unwrap(),
            target: ConnectEndpoint::parse("2001:db8::10", 9001).unwrap(),
        })
        .forwarding(Forwarding::Dynamic {
            listen: ListenEndpoint::parse(Some("::1"), 1080).unwrap(),
        })
        .command(["printf", "%s", "$(touch /tmp/not-local); literal value"])
        .build()
        .unwrap();

    let transport = descriptor.endpoint().transport_args();
    assert_eq!(
        transport,
        [
            "-F",
            "/tmp/fixture ssh/config",
            "-p",
            "2222",
            "-i",
            "/Users/me/.ssh/team key",
            "-J",
            "bastion:2200,jump@[2001:db8::7]",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "Compression=yes",
            "-o",
            "ServerAliveInterval=30",
        ]
    );
    assert_eq!(
        descriptor.endpoint().ssh_args(),
        transport
            .iter()
            .cloned()
            .chain(["--".into(), "deploy@prod-alias".into()])
            .collect::<Vec<_>>()
    );

    let process = descriptor.process_spec();
    assert_eq!(process.program, "ssh");
    assert_eq!(&process.args[..transport.len()], transport);
    assert_eq!(
        &process.args[transport.len()..],
        [
            "-A",
            "-L",
            "127.0.0.1:8080:service.internal:80",
            "-R",
            "9000:[2001:db8::10]:9001",
            "-D",
            "[::1]:1080",
            "--",
            "deploy@prod-alias",
            "printf",
            "%s",
            "$(touch /tmp/not-local); literal value",
        ]
    );
}

#[test]
fn options_are_allowlisted_normalized_ordered_and_non_conflicting() {
    let descriptor = SshDescriptor::builder("host")
        .unwrap()
        .option(SshOption::parse("loglevel=debug2").unwrap())
        .option(SshOption::parse("preferredauthentications=PUBLICKEY,password").unwrap())
        .option(SshOption::parse("batchmode=NO").unwrap())
        .build()
        .unwrap();
    assert_eq!(
        descriptor.endpoint().transport_args(),
        [
            "-o",
            "LogLevel=DEBUG2",
            "-o",
            "PreferredAuthentications=publickey,password",
            "-o",
            "BatchMode=no",
        ]
    );

    assert!(matches!(
        SshOption::parse("MadeUpOption=yes"),
        Err(DescriptorError::UnknownOption(_))
    ));
    for option in [
        "Port=22",
        "IdentityFile=key",
        "ProxyJump=jump",
        "ProxyCommand=sh -c evil",
        "StrictHostKeyChecking=no",
        "UserKnownHostsFile=/dev/null",
        "ControlPath=/tmp/shared",
        "LocalForward=8080:host:80",
        "RemoteCommand=uname",
        "PermitLocalCommand=yes",
    ] {
        assert!(
            matches!(
                SshOption::parse(option),
                Err(DescriptorError::ConflictingOption(_))
            ),
            "did not reject {option}"
        );
    }
    for option in [
        "BatchMode=maybe",
        "ConnectionAttempts=0",
        "ServerAliveInterval=-1",
        "PreferredAuthentications=publickey,publickey",
        "LogLevel=TRACE",
        "Compression=yes\nProxyCommand=evil",
        "Compression",
    ] {
        assert!(SshOption::parse(option).is_err(), "accepted {option:?}");
    }
}

#[test]
fn builder_rejects_invalid_ports_duplicate_fields_and_command_controls() {
    assert_eq!(
        SshDescriptor::builder("host").unwrap().port(0).build(),
        Err(DescriptorError::InvalidPort)
    );
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .config_file("config\n-oProxyCommand=evil")
            .build(),
        Err(DescriptorError::InvalidConfig(_))
    ));
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .identity("key\n-oProxyCommand=evil")
            .build(),
        Err(DescriptorError::InvalidIdentity(_))
    ));
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .option(SshOption::parse("Compression=yes").unwrap())
            .option(SshOption::parse("compression=no").unwrap())
            .build(),
        Err(DescriptorError::DuplicateOption(_))
    ));
    assert_eq!(
        SshDescriptor::builder("host")
            .unwrap()
            .forwarding(Forwarding::Agent { enabled: true })
            .forwarding(Forwarding::Agent { enabled: false })
            .build(),
        Err(DescriptorError::DuplicateAgentForwarding)
    );
    assert_eq!(
        SshDescriptor::builder("host")
            .unwrap()
            .command(Vec::<String>::new())
            .build(),
        Err(DescriptorError::EmptyCommand)
    );
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .command(["printf", "safe\nrm -rf /"])
            .build(),
        Err(DescriptorError::InvalidCommandArgument { index: 1, .. })
    ));
}

#[test]
fn forwarding_uses_typed_hosts_ports_and_ipv6_brackets() {
    assert!(ListenEndpoint::parse(None, 0).is_err());
    assert!(ConnectEndpoint::parse("host:80", 80).is_err());
    assert!(SshHost::parse("*").is_err());

    let jump = JumpHost::parse("user@[2001:db8::1]", Some(22)).unwrap();
    let descriptor = SshDescriptor::builder("target")
        .unwrap()
        .jump_host(jump)
        .build()
        .unwrap();
    assert_eq!(
        descriptor.endpoint().transport_args(),
        ["-J", "user@[2001:db8::1]:22"]
    );
}

#[test]
fn config_defaults_are_preserved_and_explicit_security_or_agent_policy_is_rendered() {
    let config_default = SshDescriptor::builder("host").unwrap().build().unwrap();
    assert!(config_default.endpoint().transport_args().is_empty());

    let explicit = SshDescriptor::builder("host")
        .unwrap()
        .host_key_policy(HostKeyPolicy::Ask)
        .forwarding(Forwarding::Agent { enabled: false })
        .build()
        .unwrap()
        .process_spec();
    assert_eq!(
        explicit.args,
        ["-o", "StrictHostKeyChecking=ask", "-a", "--", "host"]
    );
}

#[test]
fn descriptor_collections_and_arguments_have_hard_bounds() {
    let mut jumps = SshDescriptor::builder("host").unwrap();
    for _ in 0..=MAX_JUMP_HOSTS {
        jumps = jumps.jump_host(JumpHost::parse("jump", None).unwrap());
    }
    assert!(matches!(
        jumps.build(),
        Err(DescriptorError::TooMany {
            kind: "jump hosts",
            ..
        })
    ));

    let mut options = SshDescriptor::builder("host").unwrap();
    for _ in 0..=MAX_SSH_OPTIONS {
        options = options.option(SshOption::parse("Compression=yes").unwrap());
    }
    assert!(matches!(
        options.build(),
        Err(DescriptorError::TooMany {
            kind: "SSH options",
            ..
        })
    ));

    let mut forwarding = SshDescriptor::builder("host").unwrap();
    for port in 1..=(MAX_FORWARDINGS as u16 + 1) {
        forwarding = forwarding.forwarding(Forwarding::Dynamic {
            listen: ListenEndpoint::new(None, port).unwrap(),
        });
    }
    assert!(matches!(
        forwarding.build(),
        Err(DescriptorError::TooMany {
            kind: "forwardings",
            ..
        })
    ));

    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .command(vec!["arg"; MAX_COMMAND_ARGS + 1])
            .build(),
        Err(DescriptorError::TooMany {
            kind: "remote command arguments",
            ..
        })
    ));
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .identity("x".repeat(MAX_ARGUMENT_BYTES + 1))
            .build(),
        Err(DescriptorError::InvalidIdentity("path is too long"))
    ));
    assert!(matches!(
        SshDescriptor::builder("host")
            .unwrap()
            .config_file("x".repeat(MAX_ARGUMENT_BYTES + 1))
            .build(),
        Err(DescriptorError::InvalidConfig("path is too long"))
    ));
}
