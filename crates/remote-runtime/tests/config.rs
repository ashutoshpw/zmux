use serde_json::{Value, json};
use zmux_remote_runtime::{DescriptorError, JumpConfig, SshConnectionConfig};

#[test]
fn wire_config_maps_every_route_field_into_a_validated_descriptor() {
    let config: SshConnectionConfig = serde_json::from_value(json!({
        "destination": "deploy@production_blue",
        "config_file": "/tmp/fixture ssh/config",
        "port": 2222,
        "identity": "/Users/me/.ssh/team key",
        "forward_agent": false,
        "ssh_options": ["Compression=yes", "ServerAliveInterval=30"],
        "jump_hosts": [
            {"destination": "bastion", "port": 2200},
            {"destination": "jump@[2001:db8::7]"}
        ],
        "command": ["printf", "%s", "$(touch /tmp/not-local); literal value"]
    }))
    .unwrap();

    let descriptor = config.validate().unwrap();
    assert_eq!(
        descriptor.process_spec().args,
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
            "Compression=yes",
            "-o",
            "ServerAliveInterval=30",
            "-a",
            "--",
            "deploy@production_blue",
            "printf",
            "%s",
            "$(touch /tmp/not-local); literal value",
        ]
    );
}

#[test]
fn omitted_optional_fields_and_collections_use_explicit_defaults() {
    let config: SshConnectionConfig =
        serde_json::from_value(json!({"destination": "production"})).unwrap();

    assert_eq!(config.port, None);
    assert_eq!(config.config_file, None);
    assert_eq!(config.identity, None);
    assert_eq!(config.forward_agent, None);
    assert!(config.ssh_options.is_empty());
    assert!(config.jump_hosts.is_empty());
    assert_eq!(config.command, None);
    assert_eq!(
        config.validate().unwrap().process_spec().args,
        ["--", "production"]
    );
}

#[test]
fn restore_clears_only_the_remote_command() {
    let config = SshConnectionConfig {
        destination: "deploy@production".into(),
        config_file: Some("/tmp/fixture ssh/config".into()),
        port: Some(2222),
        identity: Some("/keys/production".into()),
        forward_agent: Some(true),
        ssh_options: vec!["Compression=yes".into()],
        jump_hosts: vec![JumpConfig {
            destination: "bastion".into(),
            port: Some(2200),
        }],
        command: Some(vec!["run-once".into(), "--job".into()]),
    };

    let restored = config.for_restore();
    assert_eq!(
        restored,
        SshConnectionConfig {
            command: None,
            ..config.clone()
        }
    );
    assert_eq!(
        config.command.as_deref(),
        Some(["run-once".into(), "--job".into()].as_slice())
    );
    assert_eq!(
        restored.validate().unwrap().process_spec().args,
        [
            "-F",
            "/tmp/fixture ssh/config",
            "-p",
            "2222",
            "-i",
            "/keys/production",
            "-J",
            "bastion:2200",
            "-o",
            "Compression=yes",
            "-A",
            "--",
            "deploy@production",
        ]
    );
}

#[test]
fn malformed_or_unknown_wire_structure_is_rejected() {
    for value in [
        json!({"destination": "host", "unknown": true}),
        json!({"destination": "host", "port": "22"}),
        json!({"destination": "host", "jump_hosts": [{"port": 22}]}),
        json!({
            "destination": "host",
            "jump_hosts": [{"destination": "jump", "unknown": true}]
        }),
    ] {
        assert!(serde_json::from_value::<SshConnectionConfig>(value).is_err());
    }
}

#[test]
fn invalid_values_remain_data_until_validation_and_never_change_routes() {
    let cases: [(Value, DescriptorError); 5] = [
        (
            json!({"destination": "host", "port": 0}),
            DescriptorError::InvalidPort,
        ),
        (
            json!({"destination": "host:2222"}),
            DescriptorError::InvalidHost("host:2222".into()),
        ),
        (
            json!({"destination": "host", "jump_hosts": [{"destination": "jump:2200"}]}),
            DescriptorError::InvalidHost("jump:2200".into()),
        ),
        (
            json!({"destination": "host", "ssh_options": ["ProxyCommand=local-shell"]}),
            DescriptorError::ConflictingOption("ProxyCommand".into()),
        ),
        (
            json!({"destination": "host", "config_file": ""}),
            DescriptorError::InvalidConfig("path is empty"),
        ),
    ];

    for (value, expected_error) in cases {
        let config: SshConnectionConfig = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&config).unwrap(), with_defaults(value));
        assert_eq!(config.validate(), Err(expected_error));
    }
}

fn with_defaults(mut value: Value) -> Value {
    let object = value.as_object_mut().unwrap();
    object.entry("config_file").or_insert(Value::Null);
    object.entry("port").or_insert(Value::Null);
    object.entry("identity").or_insert(Value::Null);
    object.entry("forward_agent").or_insert(Value::Null);
    object.entry("ssh_options").or_insert_with(|| json!([]));
    object.entry("jump_hosts").or_insert_with(|| json!([]));
    object.entry("command").or_insert(Value::Null);
    for jump in object["jump_hosts"].as_array_mut().unwrap() {
        jump.as_object_mut()
            .unwrap()
            .entry("port")
            .or_insert(Value::Null);
    }
    value
}
