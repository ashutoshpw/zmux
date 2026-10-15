//! Session operations submit asynchronous filesystem work to the running app.
use super::*;
use zmux_desktop_runtime::sessions::SessionCommand;

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    match command {
        "restore-session" => restore(tail),
        "save-session" => app_no_args(
            command,
            tail,
            ControlCommand::Session {
                action: SessionCommand::Save,
            },
        ),
        "session-status" => status(tail),
        _ => unreachable!("session parser only receives registered session commands"),
    }
}

fn restore(tail: &[OsString]) -> Result<Parsed, CliError> {
    let mut from = None;
    let mut export = None;
    let mut force = false;
    let mut index = 0;
    while index < tail.len() {
        let flag = utf8_arg(tail.get(index), "restore-session option")?;
        match flag {
            "--from" | "--export" => {
                let destination = if flag == "--from" {
                    &mut from
                } else {
                    &mut export
                };
                if destination.is_some() {
                    return Err(CliError(format!("repeated {flag}")));
                }
                index += 1;
                let value = utf8_arg(tail.get(index), &format!("{flag} path"))?;
                if value.is_empty() || value.starts_with('-') {
                    return Err(CliError(format!("{flag} requires a nonempty path")));
                }
                let path = PathBuf::from(value);
                *destination = Some(if path.is_absolute() {
                    path
                } else {
                    env::current_dir()?.join(path)
                });
            }
            "--force" => {
                if force {
                    return Err(CliError("repeated --force".into()));
                }
                force = true;
            }
            other => {
                return Err(CliError(format!(
                    "unknown restore-session option or argument `{other}`"
                )));
            }
        }
        index += 1;
    }
    if from.is_some() && export.is_some() {
        return Err(CliError("--from conflicts with --export".into()));
    }
    if force && export.is_none() {
        return Err(CliError("--force requires --export".into()));
    }
    Ok(Parsed::App(ControlCommand::Session {
        action: match export {
            Some(path) => SessionCommand::Export { path, force },
            None => SessionCommand::Restore { path: from },
        },
    }))
}

fn status(tail: &[OsString]) -> Result<Parsed, CliError> {
    let mut operation_id = None;
    let mut index = 0;
    while index < tail.len() {
        let flag = utf8_arg(tail.get(index), "session-status option")?;
        if flag != "--id" {
            return Err(CliError(format!(
                "unknown session-status option or argument `{flag}`"
            )));
        }
        if operation_id.is_some() {
            return Err(CliError("repeated --id".into()));
        }
        index += 1;
        operation_id = Some(parse_id(
            utf8_arg(tail.get(index), "--id operation ID")?,
            "operation",
        )?);
        index += 1;
    }
    Ok(Parsed::App(ControlCommand::SessionStatus { operation_id }))
}
