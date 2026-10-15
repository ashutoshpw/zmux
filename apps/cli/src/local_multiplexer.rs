//! Headless local tmux lifecycle bypasses app socket discovery and credentials.
use super::*;
use std::{collections::HashSet, io::IsTerminal, os::unix::process::CommandExt};
use zmux_remote_runtime::local_multiplexer::{LocalTmux, SessionSelector, clear_context};

pub(super) enum Invocation {
    Start {
        name: String,
        cwd: PathBuf,
        command: Option<String>,
    },
    Attach(SessionSelector),
    List,
    Status(SessionSelector),
    Detach {
        selector: SessionSelector,
        all: bool,
    },
    Close(SessionSelector),
    Cleanup {
        prune: bool,
    },
}
pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail)
        || tail
            .get(1)
            .is_some_and(|v| matches!(v.to_str(), Some("--help" | "-h")))
    {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let args = utf8_args(tail)?;
    let verb = args.first().map(String::as_str).ok_or_else(|| {
        CliError("local-tmux requires start, attach, list, status, detach, close or cleanup".into())
    })?;
    if command == "tmux" && verb != "attach" {
        return Err(CliError(
            "the tmux alias only supports attach; use local-tmux for lifecycle operations".into(),
        ));
    }
    if ![
        "start",
        "attach",
        "list",
        "status",
        "detach",
        "close",
        "cleanup",
        "operation-status",
    ]
    .contains(&verb)
    {
        return Err(CliError(format!("unknown local-tmux operation {verb}")));
    }
    if verb == "operation-status" {
        return operation_status(&args[1..]);
    }
    let mut id = None;
    let mut name = None;
    let mut cwd = None;
    let mut shell = None;
    let mut detached = false;
    let mut headless = false;
    let mut all = false;
    let mut prune = false;
    let mut workspace = None;
    let mut focus = None;
    let mut new_client = false;
    let mut seen = HashSet::new();
    let mut index = 1;
    let mut literal = false;
    while index < args.len() {
        let flag = args[index].as_str();
        if !literal && flag == "--" {
            literal = true;
            index += 1;
            continue;
        }
        if !literal && flag.starts_with('-') {
            if !seen.insert(flag) {
                return Err(CliError(format!("repeated {flag}")));
            }
            match flag {
                "--id" if matches!(verb, "attach" | "status" | "detach" | "close") => {
                    index += 1;
                    let value = args.get(index).ok_or_else(|| CliError("--id requires a UUID".into()))?;
                    id = Some(value.parse().map_err(|_| CliError("--id requires a UUID".into()))?);
                }
                "--cwd" | "--command" if verb == "start" => {
                    index += 1;
                    let value = args.get(index).ok_or_else(|| CliError(format!("{flag} requires a value")))?;
                    if value.is_empty() {
                        return Err(CliError(format!("{flag} requires a nonempty value")));
                    }
                    if flag == "--cwd" {
                        cwd = Some(PathBuf::from(value));
                    } else {
                        shell = Some(value.clone());
                    }
                }
                "--detached" if verb == "start" => detached = true,
                "--headless" if verb == "attach" => headless = true,
                "--all" if verb == "detach" => all = true,
                "--prune" if verb == "cleanup" => prune = true,
                "--json" if matches!(verb, "list" | "status") => (),
                "--client" if verb == "detach" => return Err(CliError("individual --client detach is not implemented; use --all to detach every client explicitly".into())),
                "--workspace" if matches!(verb,"start"|"attach") => {
                    index+=1;
                    workspace=Some(args.get(index).ok_or_else(||CliError("--workspace requires a target".into()))?.parse().map_err(|e:zmux_workspace_core::targets::TargetError|CliError(e.to_string()))?);
                }
                "--focus" if matches!(verb,"start"|"attach") => {
                    index+=1;
                    let value=match args.get(index).map(String::as_str) {Some("true")=>true,Some("false")=>false,_=>return Err(CliError("--focus requires true or false".into()))};
                    if focus.replace(value).is_some() {return Err(CliError("repeated/conflicting focus option".into()));}
                }
                "--no-focus" if matches!(verb,"start"|"attach") => {
                    if focus.replace(false).is_some() {return Err(CliError("repeated/conflicting focus option".into()));}
                }
                "--new-client" if verb=="attach" => new_client=true,
                _ => return Err(CliError(format!("unknown or irrelevant local-tmux {verb} option {flag}"))),
            }
        } else if name.replace(args[index].clone()).is_some() {
            return Err(CliError("local-tmux accepts one session name".into()));
        }
        index += 1;
    }
    if matches!(verb, "list" | "cleanup") {
        if name.is_some() {
            return Err(CliError(format!(
                "local-tmux {verb} does not take a session"
            )));
        }
        return Ok(Parsed::LocalMultiplexer(if verb == "list" {
            Invocation::List
        } else {
            Invocation::Cleanup { prune }
        }));
    }
    if name.is_some() && id.is_some() {
        return Err(CliError("choose a session name or --id, not both".into()));
    }
    let selector = match (name, id) {
        (Some(name), None) => SessionSelector::name(&name).map_err(|e| CliError(e.to_string()))?,
        (None, Some(id)) => SessionSelector::Id(id),
        _ => {
            return Err(CliError(
                "local-tmux operation requires a session name or --id UUID".into(),
            ));
        }
    };
    if (detached || headless) && (workspace.is_some() || focus.is_some() || new_client) {
        return Err(CliError(
            "headless/detached forms do not accept GUI workspace/focus/client options".into(),
        ));
    }
    let scope = zmux_workspace_core::targets::TargetScope {
        workspace: workspace.clone(),
        ..Default::default()
    };
    let invocation = match verb {
        "start" => {
            let SessionSelector::Name(name) = selector else {
                return Err(CliError("start requires a session name".into()));
            };
            let cwd = cwd.unwrap_or(env::current_dir()?);
            let cwd = if cwd.is_absolute() {
                cwd
            } else {
                env::current_dir()?.join(cwd)
            };
            if !detached {
                return Ok(Parsed::LocalTmuxApp(ControlCommand::LocalTmux {
                    request: zmux_desktop_runtime::local_tmux::Request {
                        operation: zmux_desktop_runtime::local_tmux::Operation::Start {
                            name,
                            cwd,
                            command: shell,
                        },
                        workspace,
                        caller: targets::caller(&scope)?,
                        focus: focus.unwrap_or(true),
                        new_client: true,
                    },
                }));
            }
            Invocation::Start {
                name,
                cwd,
                command: shell,
            }
        }
        "attach" => {
            if !headless {
                return Ok(Parsed::LocalTmuxApp(ControlCommand::LocalTmux {
                    request: zmux_desktop_runtime::local_tmux::Request {
                        operation: zmux_desktop_runtime::local_tmux::Operation::Attach { selector },
                        workspace,
                        caller: targets::caller(&scope)?,
                        focus: focus.unwrap_or(true),
                        new_client,
                    },
                }));
            }
            Invocation::Attach(selector)
        }
        "status" => Invocation::Status(selector),
        "detach" => Invocation::Detach { selector, all },
        "close" => Invocation::Close(selector),
        _ => unreachable!("validated lifecycle operation"),
    };
    Ok(Parsed::LocalMultiplexer(invocation))
}
pub(super) fn run(invocation: Invocation) -> Result<u8, Box<dyn Error>> {
    if matches!(&invocation, Invocation::Attach(_))
        && (!io::stdin().is_terminal() || !io::stdout().is_terminal())
    {
        return Err(
            CliError("headless tmux attach requires terminal stdin and stdout".into()).into(),
        );
    }
    let owner = LocalTmux::from_environment()?;
    let result = match invocation {
        Invocation::Start { name, cwd, command } => {
            serde_json::json!({"socket_path":owner.socket_path(),"session":owner.start(&name,&cwd,command.as_deref())?})
        }
        Invocation::List => serde_json::to_value(owner.list()?)?,
        Invocation::Status(selector) => {
            serde_json::json!({"socket_path":owner.socket_path(),"session":owner.status(&selector)?})
        }
        Invocation::Detach { selector, all } => {
            serde_json::to_value(owner.detach(&selector, all)?)?
        }
        Invocation::Close(selector) => serde_json::json!({"closed_id":owner.close(&selector)?}),
        Invocation::Cleanup { prune } => serde_json::to_value(owner.cleanup(prune)?)?,
        Invocation::Attach(selector) => {
            let spec = owner.attach_spec(&selector)?;
            let mut command = std::process::Command::new(spec.program);
            command.args(spec.args);
            clear_context(&mut command);
            return Err(command.exec().into());
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(0)
}

fn operation_status(args: &[String]) -> Result<Parsed, CliError> {
    let (mut id, mut epoch) = (None, None);
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        index += 1;
        let value = args
            .get(index)
            .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
        match flag {
            "--id" if id.is_none() => {
                id = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| CliError("operation --id must be positive".into()))?,
                )
            }
            "--epoch" if epoch.is_none() => {
                epoch = Some(
                    value
                        .parse()
                        .map_err(|_| CliError("operation --epoch requires a UUID".into()))?,
                )
            }
            _ => {
                return Err(CliError(format!(
                    "unknown/repeated local-tmux operation-status option {flag}"
                )));
            }
        }
        index += 1;
    }
    Ok(Parsed::LocalTmuxApp(ControlCommand::LocalTmuxStatus {
        operation_id: id.ok_or_else(|| CliError("operation-status requires --id".into()))?,
        epoch: epoch.ok_or_else(|| CliError("operation-status requires --epoch".into()))?,
    }))
}
pub(super) fn run_gui(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let status_only = matches!(command, ControlCommand::LocalTmuxStatus { .. });
    let deadline = Instant::now() + options.timeout;
    let mut receipt = response_result(request(options, command)?)?;
    if status_only {
        return Ok(receipt);
    }
    let id = receipt["operation_id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| CliError("missing tmux operation ID".into()))?;
    let epoch_text = receipt["epoch"]
        .as_str()
        .ok_or_else(|| CliError("missing tmux operation epoch".into()))?
        .to_owned();
    let epoch = epoch_text
        .parse()
        .map_err(|_| CliError("invalid tmux operation epoch".into()))?;
    loop {
        if receipt["operation_id"].as_u64() != Some(id)
            || receipt["epoch"].as_str() != Some(epoch_text.as_str())
        {
            return Err(CliError("mismatched tmux receipt identity".into()));
        }
        match receipt["state"].as_str() {
            Some("completed") => return Ok(receipt),
            Some("failed") => {
                return Err(CliError(format!(
                    "{} (owner_outcome={}, identity={}); inspect local-tmux operation-status --id {id} --epoch {epoch}",
                    receipt["error"]
                        .as_str()
                        .unwrap_or("tmux attachment failed"),
                    receipt["owner_outcome"],
                    receipt["identity"]
                )));
            }
            Some("pending") => (),
            _ => return Err(CliError("invalid tmux receipt state".into())),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CliError(format!(
                "tmux attachment deadline elapsed; outcome is unknown; inspect local-tmux operation-status --id {id} --epoch {epoch}; do not replay start"
            )));
        }
        thread::sleep(Duration::from_millis(20).min(remaining));
        let budget = ClientOptions {
            socket: options.socket.clone(),
            password: options.password.clone(),
            timeout: deadline.saturating_duration_since(Instant::now()),
        };
        if budget.timeout.is_zero() {
            continue;
        }
        receipt = response_result(request(
            &budget,
            ControlCommand::LocalTmuxStatus {
                operation_id: id,
                epoch,
            },
        )?)?;
    }
}
