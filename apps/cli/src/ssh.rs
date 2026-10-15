use super::*;
use zmux_remote_runtime::{JumpConfig, SshConnectionConfig};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("ssh".into()))));
    }
    let args = utf8_args(tail)?;
    let mut connection = SshConnectionConfig {
        destination: String::new(),
        port: None,
        identity: None,
        config_file: None,
        forward_agent: None,
        ssh_options: Vec::new(),
        jump_hosts: Vec::new(),
        command: None,
    };
    let mut name = None;
    let mut focus = None;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        if flag == "--" {
            if connection.command.is_some() {
                return Err(CliError(
                    "--command conflicts with trailing remote arguments".into(),
                ));
            }
            if index + 1 < args.len() {
                connection.command = Some(args[index + 1..].to_vec());
            }
            break;
        }
        match flag.as_str() {
            "-A" | "--forward-agent" | "-a" | "--no-forward-agent" => {
                if connection.forward_agent.is_some() {
                    return Err(CliError(
                        "repeated or conflicting agent-forwarding policy".into(),
                    ));
                }
                connection.forward_agent = Some(matches!(flag.as_str(), "-A" | "--forward-agent"));
            }
            "--no-focus" => {
                if focus.replace(false).is_some() {
                    return Err(CliError("repeated focus option".into()));
                }
            }
            "--port" | "--identity" | "--ssh-config" | "--name" | "--command" | "--ssh-option"
            | "--jump" | "--focus" | "--transport" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
                match flag.as_str() {
                    "--port" => {
                        if connection.port.is_some() {
                            return Err(CliError("repeated --port".into()));
                        }
                        connection.port = Some(
                            value
                                .parse()
                                .map_err(|_| CliError("SSH port must be 1–65535".into()))?,
                        );
                    }
                    "--identity" | "--ssh-config" => {
                        let target = if flag == "--identity" {
                            &mut connection.identity
                        } else {
                            &mut connection.config_file
                        };
                        if target.is_some() {
                            return Err(CliError(format!("repeated {flag}")));
                        }
                        if value.is_empty() {
                            return Err(CliError(format!("{flag} requires a nonempty path")));
                        }
                        let path = PathBuf::from(value);
                        *target = Some(
                            if path.is_absolute()
                                || value.starts_with("~/")
                                || (flag == "--ssh-config" && value == "none")
                            {
                                value.clone()
                            } else {
                                env::current_dir()?
                                    .join(path)
                                    .into_os_string()
                                    .into_string()
                                    .map_err(|_| CliError("SSH path must be valid UTF-8".into()))?
                            },
                        );
                    }
                    "--name" => set_once(&mut name, value.clone(), "--name")?,
                    "--command" => {
                        set_once(&mut connection.command, vec![value.clone()], "--command")?
                    }
                    "--ssh-option" => connection.ssh_options.push(value.clone()),
                    "--jump" => connection.jump_hosts.push(JumpConfig {
                        destination: value.clone(),
                        port: None,
                    }),
                    "--focus" => {
                        let value = match value.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => return Err(CliError("--focus requires true or false".into())),
                        };
                        if focus.replace(value).is_some() {
                            return Err(CliError("repeated focus option".into()));
                        }
                    }
                    "--transport" if value != "ssh" => {
                        return Err(CliError(
                            "only SSH transport is implemented; mosh remains unavailable".into(),
                        ));
                    }
                    _ => {}
                }
            }
            value if !value.starts_with('-') && connection.destination.is_empty() => {
                connection.destination = value.into()
            }
            _ => {
                return Err(CliError(format!(
                    "unknown SSH option or extra destination: {flag}"
                )));
            }
        }
        index += 1;
    }
    connection.validate().map_err(|e| CliError(e.to_string()))?;
    let name = name.unwrap_or_else(|| {
        format!("ssh {}", connection.destination)
            .chars()
            .take(zmux_workspace_core::MAX_NAME_CHARS)
            .collect()
    });
    Ok(Parsed::App(ControlCommand::Action {
        action: Action::CreateSshWorkspace {
            name,
            connection,
            focus: focus.unwrap_or(true),
        },
    }))
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError(format!("repeated {name}")));
    }
    *slot = Some(value);
    Ok(())
}
