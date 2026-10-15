//! Browser automation submits once, then polls bounded native execution receipts.
use super::*;
use zmux_desktop_runtime::browser::{
    BrowserAction, BrowserRequest, DEFAULT_OPERATION_TIMEOUT_MS, GetKind, MAX_OPERATION_TIMEOUT_MS,
    SnapshotOptions,
};
use zmux_workspace_core::targets::{TargetRef, TargetScope};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    let args = utf8_args(tail)?;
    if help_requested(tail) || args.as_slice() == ["help"] {
        return help();
    }
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(CliError(
            "browser requires a subcommand; run `zmux help browser`".into(),
        ));
    };
    if matches!(verb, "open" | "navigate" | "goto") {
        return targets::parse("browser", tail);
    }
    if !matches!(
        verb,
        "get" | "eval" | "snapshot" | "back" | "forward" | "reload"
    ) {
        return Err(CliError(format!("unknown browser subcommand `{verb}`")));
    }
    if help_requested(&tail[1..]) {
        return help();
    }
    let (kind, values_start) = if verb == "get" {
        let value = args
            .get(1)
            .ok_or_else(|| CliError("browser get requires a kind".into()))?;
        let kind = match value.as_str() {
            "url" => GetKind::Url,
            "title" => GetKind::Title,
            "text" => GetKind::Text,
            "html" => GetKind::Html,
            "value" => GetKind::Value,
            "attr" => GetKind::Attr,
            "count" => GetKind::Count,
            "box" => GetKind::Box,
            "styles" => GetKind::Styles,
            _ => return Err(CliError(format!("unknown browser get kind `{value}`"))),
        };
        if help_requested(&tail[2..]) {
            return help();
        }
        (Some(kind), 2)
    } else {
        (None, 1)
    };
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut selector = None;
    let mut script = None;
    let mut attribute = None;
    let mut property = None;
    let mut max_depth = None;
    let mut interactive = false;
    let mut cursor = false;
    let mut compact = false;
    let mut snapshot_after = false;
    let mut values = Vec::new();
    let mut index = values_start;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--" {
            values.extend_from_slice(&args[index + 1..]);
            break;
        }
        let target_slot = match flag {
            "--workspace" => Some(&mut scope.workspace),
            "--pane" => Some(&mut scope.pane),
            "--surface" | "--tab" => Some(&mut scope.surface),
            _ => None,
        };
        if let Some(slot) = target_slot {
            if slot.is_some() {
                return Err(CliError(format!("repeated {flag}")));
            }
            *slot = Some(
                value(&args, index, flag)?
                    .parse::<TargetRef>()
                    .map_err(|e| CliError(e.to_string()))?,
            );
            index += 2;
            continue;
        }
        match flag {
            "--expect-revision" => {
                if revision.is_some() {
                    return Err(CliError("repeated --expect-revision".into()));
                }
                revision = Some(value(&args, index, flag)?.parse::<u64>().map_err(|_| {
                    CliError("model revision must be a nonnegative integer".into())
                })?);
                index += 2;
            }
            "--selector" if verb == "get" || verb == "snapshot" => {
                set(&mut selector, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--script" if verb == "eval" => {
                set(&mut script, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--attr" if matches!(kind, Some(GetKind::Attr)) => {
                set(&mut attribute, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--property" if matches!(kind, Some(GetKind::Styles)) => {
                set(&mut property, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--max-depth" if verb == "snapshot" => {
                if max_depth.is_some() {
                    return Err(CliError("repeated --max-depth".into()));
                }
                max_depth =
                    Some(value(&args, index, flag)?.parse::<u32>().map_err(|_| {
                        CliError("--max-depth must be a nonnegative integer".into())
                    })?);
                index += 2;
            }
            "--interactive" | "-i" if verb == "snapshot" => {
                flag_once(&mut interactive, "--interactive")?;
                index += 1;
            }
            "--cursor" if verb == "snapshot" => {
                flag_once(&mut cursor, flag)?;
                index += 1;
            }
            "--compact" if verb == "snapshot" => {
                flag_once(&mut compact, flag)?;
                index += 1;
            }
            "--snapshot-after" if matches!(verb, "back" | "forward" | "reload") => {
                flag_once(&mut snapshot_after, flag)?;
                index += 1;
            }
            other if other.starts_with('-') => {
                return Err(CliError(format!(
                    "unknown browser {verb} option `{other}`; use -- before literal text"
                )));
            }
            _ => {
                values.push(args[index].clone());
                index += 1;
            }
        }
    }
    let action = match verb {
        "eval" => {
            if script.is_some() && !values.is_empty() {
                return Err(CliError(
                    "--script conflicts with trailing script arguments".into(),
                ));
            }
            let script = script.unwrap_or_else(|| values.join(" "));
            if script.trim().is_empty() {
                return Err(CliError("browser eval requires a nonempty script".into()));
            }
            BrowserAction::Eval { script }
        }
        "get" => {
            let kind = kind.expect("parsed get kind");
            if matches!(kind, GetKind::Url | GetKind::Title) {
                if selector.is_some() || !values.is_empty() {
                    return Err(CliError(
                        "browser get url/title accepts no selector or trailing arguments".into(),
                    ));
                }
            } else {
                if selector.is_none() && !values.is_empty() {
                    selector = Some(values.remove(0));
                }
                if selector.as_ref().is_none_or(|s| s.is_empty()) {
                    return Err(CliError("browser get requires a nonempty selector".into()));
                }
                if matches!(kind, GetKind::Attr) && attribute.is_none() && !values.is_empty() {
                    attribute = Some(values.remove(0));
                }
                if !values.is_empty() {
                    return Err(CliError(
                        "browser get received conflicting or extra arguments".into(),
                    ));
                }
                if matches!(kind, GetKind::Attr) && attribute.is_none() {
                    return Err(CliError(
                        "browser get attr requires --attr NAME or a trailing attribute name".into(),
                    ));
                }
            }
            BrowserAction::Get {
                kind,
                selector,
                attribute,
                property,
            }
        }
        _ => {
            if !values.is_empty() {
                return Err(CliError(format!("browser {verb} accepts options only")));
            }
            match verb {
                "snapshot" => BrowserAction::Snapshot {
                    options: SnapshotOptions {
                        selector,
                        interactive,
                        cursor,
                        compact,
                        max_depth,
                    },
                },
                "back" => BrowserAction::Back { snapshot_after },
                "forward" => BrowserAction::Forward { snapshot_after },
                "reload" => BrowserAction::Reload { snapshot_after },
                _ => unreachable!("validated browser verb"),
            }
        }
    };
    action
        .validate()
        .map_err(|error| CliError(format!("{}: {error}", error.code())))?;
    let caller = targets::caller(&scope)?;
    Ok(Parsed::Browser(ControlCommand::Browser {
        request: BrowserRequest {
            scope,
            caller,
            expected_model_revision: revision,
            action,
            timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
        },
    }))
}

fn help() -> Result<Parsed, CliError> {
    Ok(Parsed::Offline(Offline::Help(Some("browser".into()))))
}
fn value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str, CliError> {
    args.get(index + 1)
        .filter(|value| !value.is_empty() && !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| CliError(format!("{flag} requires a nonempty value")))
}
fn set(slot: &mut Option<String>, value: &str, flag: &str) -> Result<(), CliError> {
    if slot.replace(value.to_owned()).is_some() {
        return Err(CliError(format!("repeated {flag}")));
    }
    Ok(())
}
fn flag_once(slot: &mut bool, flag: &str) -> Result<(), CliError> {
    if std::mem::replace(slot, true) {
        return Err(CliError(format!("repeated {flag}")));
    }
    Ok(())
}

pub(super) fn run(
    options: &ClientOptions,
    mut command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    if let ControlCommand::Browser { request } = &mut command {
        let budget = deadline.saturating_duration_since(Instant::now());
        // Leave room to retrieve a terminal receipt when native execution times out.
        let headroom = Duration::from_millis(100).min(budget / 5);
        request.timeout_ms = budget
            .saturating_sub(headroom)
            .as_millis()
            .clamp(1, u128::from(MAX_OPERATION_TIMEOUT_MS)) as u64;
    }
    let admission = response_result(request(&remaining(options, deadline, None)?, command)?)?;
    let id = admission["operation_id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| CliError("running app returned an invalid browser operation ID".into()))?;
    let mut receipt = admission;
    loop {
        if receipt["operation_id"].as_u64() != Some(id) {
            return Err(CliError(
                "running app returned a mismatched browser operation ID".into(),
            ));
        }
        match receipt["state"].as_str() {
            Some("completed") => {
                return receipt
                    .get("result")
                    .cloned()
                    .filter(|value| !value.is_null())
                    .ok_or_else(|| {
                        CliError(
                            "running app returned a browser completion without a result".into(),
                        )
                    });
            }
            Some("failed") => {
                let code = receipt["error"]["code"]
                    .as_str()
                    .unwrap_or("browser_failed");
                let message = receipt["error"]["message"]
                    .as_str()
                    .unwrap_or("browser operation failed");
                let uncertain = if receipt["error"]["outcome_unknown"].as_bool() == Some(true) {
                    "; its outcome may be unknown and the command was not retried"
                } else {
                    ""
                };
                return Err(CliError(format!("{code}: {message}{uncertain}")));
            }
            Some("pending") => (),
            _ => {
                return Err(CliError(
                    "running app returned an invalid browser receipt state".into(),
                ));
            }
        }
        let budget = remaining(options, deadline, Some(id))?;
        thread::sleep(Duration::from_millis(20).min(budget.timeout));
        receipt = request(
            &remaining(options, deadline, Some(id))?,
            ControlCommand::BrowserStatus { operation_id: id },
        )
        .and_then(response_result)
        .map_err(|error| {
            CliError(format!(
                "browser operation {id}: {error}; the admitted command was not retried"
            ))
        })?;
    }
}
fn remaining(
    options: &ClientOptions,
    deadline: Instant,
    id: Option<u64>,
) -> Result<ClientOptions, CliError> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout < Duration::from_millis(1) {
        let operation = id.map(|id| format!(" {id}")).unwrap_or_default();
        return Err(CliError(format!(
            "browser operation{operation} exceeded the client deadline; its outcome may be unknown and the command was not retried"
        )));
    }
    Ok(ClientOptions {
        socket: options.socket.clone(),
        timeout,
        password: options.password.clone(),
    })
}
