//! Download history and wait observation are distinct from document automation.
use super::*;
use zmux_desktop_runtime::downloads::{
    DEFAULT_WAIT_MS, DownloadAction, DownloadRequest, MAX_WAIT_MS,
};
use zmux_workspace_core::targets::{TargetRef, TargetScope};

pub(super) fn parse(args: &[String]) -> Result<Parsed, CliError> {
    let (verb, start) = match args.first().map(String::as_str) {
        Some("list" | "ls") => ("list", 1),
        Some("wait") => ("wait", 1),
        Some("cancel") => ("cancel", 1),
        _ => ("wait", 0),
    };
    let mut scope = TargetScope::default();
    let mut path = None;
    let mut timeout_ms = None;
    let mut limit = None;
    let mut positional = Vec::new();
    let mut after_separator = false;
    let mut json_output = false;
    let mut index = start;
    while index < args.len() {
        let argument = &args[index];
        if after_separator {
            positional.push(argument.clone());
            index += 1;
            continue;
        }
        if argument == "--" {
            after_separator = true;
            index += 1;
            continue;
        }
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        if flag == "--json" {
            if verb != "list" || inline.is_some() || json_output {
                return Err(CliError("Download list accepts one --json flag".into()));
            }
            json_output = true;
            index += 1;
            continue;
        }
        let known = matches!(
            flag,
            "--workspace"
                | "--pane"
                | "--surface"
                | "--tab"
                | "--limit"
                | "--path"
                | "--timeout-ms"
                | "--timeout"
        );
        if !known {
            if flag.starts_with('-') {
                return Err(CliError(format!("Unknown browser download option {flag}")));
            }
            positional.push(argument.clone());
            index += 1;
            continue;
        }
        let value = match inline {
            Some(value) => value,
            None => args
                .get(index + 1)
                .filter(|value| !value.starts_with("--"))
                .map(String::as_str)
                .ok_or_else(|| CliError(format!("{flag} requires a value")))?,
        };
        index += if inline.is_some() { 1 } else { 2 };
        let slot = match flag {
            "--workspace" => Some(&mut scope.workspace),
            "--pane" => Some(&mut scope.pane),
            "--surface" | "--tab" => Some(&mut scope.surface),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return Err(CliError(format!("Repeated {flag}")));
            }
            *slot = Some(
                value
                    .parse::<TargetRef>()
                    .map_err(|e| CliError(e.to_string()))?,
            );
            continue;
        }
        match flag {
            "--limit" if verb == "list" => {
                if limit.is_some() {
                    return Err(CliError("Download list accepts one limit".into()));
                }
                let parsed = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=25).contains(n))
                    .ok_or_else(|| CliError("Download list limit must be 1 through 25".into()))?;
                limit = Some(parsed);
            }
            "--path" if verb == "wait" => {
                if value.is_empty() || path.is_some() {
                    return Err(CliError("Download wait accepts one nonempty path".into()));
                }
                path = Some(value.to_owned());
            }
            "--timeout-ms" | "--timeout" if verb == "wait" => {
                if timeout_ms.is_some() {
                    return Err(CliError("Download wait accepts one timeout".into()));
                }
                let milliseconds = if flag == "--timeout-ms" {
                    value
                        .parse::<i64>()
                        .map_err(|_| CliError("Download timeout-ms must be an integer".into()))?
                        .max(1) as u64
                } else {
                    let seconds = value
                        .parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite() && n.abs() <= 9_000_000_000_000_000.0)
                        .ok_or_else(|| {
                            CliError("Download timeout must be a finite number".into())
                        })?;
                    (seconds * 1000.0).max(1.0) as u64
                };
                timeout_ms = Some(milliseconds.clamp(1, MAX_WAIT_MS));
            }
            _ => {
                return Err(CliError(format!(
                    "{flag} is unavailable for download {verb}"
                )));
            }
        }
    }
    let (action, wait) = match verb {
        "list" if positional.is_empty() => (
            DownloadAction::List { limit },
            Some(Duration::from_secs(20)),
        ),
        "cancel" if positional.len() == 1 => (
            DownloadAction::Cancel {
                download_id: positional[0]
                    .parse()
                    .map_err(|_| CliError("Download cancel requires a UUID".into()))?,
            },
            None,
        ),
        "wait" if positional.len() <= 1 && (path.is_none() || positional.is_empty()) => {
            let raw = path.or_else(|| positional.first().cloned());
            let path = raw
                .map(|value| {
                    let path = PathBuf::from(value);
                    if path.is_absolute() {
                        Ok(path.to_string_lossy().into_owned())
                    } else {
                        env::current_dir().map(|cwd| cwd.join(path).to_string_lossy().into_owned())
                    }
                })
                .transpose()?;
            (
                DownloadAction::Wait { path, timeout_ms },
                Some(
                    Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_MS))
                        + Duration::from_secs(5),
                ),
            )
        }
        _ => {
            return Err(CliError(format!(
                "Invalid browser download {verb} arguments"
            )));
        }
    };
    let caller = targets::caller(&scope)?;
    Ok(Parsed::BrowserDownload(
        ControlCommand::Downloads {
            request: DownloadRequest {
                scope,
                caller,
                action,
            },
        },
        wait,
    ))
}

pub(super) fn run(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    let mut result = response_result(request(options, command)?)?;
    let Some(id) = result.get("ticket_id").and_then(|id| id.as_str()) else {
        return Ok(result);
    };
    let ticket_id = id
        .parse()
        .map_err(|_| CliError("Invalid download wait ticket".into()))?;
    let epoch = result
        .get("epoch")
        .and_then(|id| id.as_str())
        .ok_or_else(|| CliError("Download wait owner is missing".into()))?
        .parse()
        .map_err(|_| CliError("Invalid download wait owner".into()))?;
    loop {
        match result.get("state").and_then(|state| state.as_str()) {
            Some("completed") => {
                return result
                    .get("result")
                    .cloned()
                    .ok_or_else(|| CliError("Download wait omitted its result".into()));
            }
            Some("failed") => {
                return Err(CliError(
                    result
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Download wait failed")
                        .into(),
                ));
            }
            Some("pending") => {}
            _ => return Err(CliError("Invalid download wait state".into())),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(1) {
            return Err(CliError(format!(
                "Download wait {ticket_id} exceeded the client deadline; observation was not replayed"
            )));
        }
        thread::sleep(Duration::from_millis(25).min(remaining));
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CliError(format!(
                "Download wait {ticket_id} exceeded the client deadline; observation was not replayed"
            )));
        }
        let budget = ClientOptions {
            timeout: remaining,
            socket: options.socket.clone(),
            password: options.password.clone(),
        };
        result = response_result(request(
            &budget,
            ControlCommand::DownloadWaitStatus { epoch, ticket_id },
        )?)?;
    }
}
