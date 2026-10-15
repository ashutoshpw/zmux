//! Browser automation submits once, then polls bounded native execution receipts.
use super::*;
use zmux_desktop_runtime::browser::{
    BrowserAction, BrowserInteraction, BrowserKeyboardOperation, BrowserLocator, BrowserRequest,
    BrowserZoom, DEFAULT_OPERATION_TIMEOUT_MS, GetKind, LocatorKind, MAX_OPERATION_TIMEOUT_MS,
    SnapshotOptions, WaitState,
};
use zmux_workspace_core::targets::{TargetRef, TargetScope};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    let mut args = utf8_args(tail)?;
    if help_requested(tail) || args.as_slice() == ["help"] {
        return help();
    }
    if args.first().is_some_and(|arg| arg == "profiles") {
        return match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["profiles", "list"] => Ok(Parsed::App(ControlCommand::BrowserProfilesStatus {
                operation_id: None,
                epoch: None,
            })),
            ["profiles", "add", name] => {
                zmux_desktop_runtime::profiles::validate_name(name).map_err(CliError)?;
                Ok(Parsed::BrowserProfile(ControlCommand::BrowserProfiles {
                    action: zmux_desktop_runtime::profiles::ProfileCommand::Add {
                        name: (*name).into(),
                    },
                }))
            }
            _ => Err(CliError(
                "browser profiles requires list or add NAME".into(),
            )),
        };
    }
    let mut prefix = Vec::new();
    while args.first().is_some_and(|flag| {
        matches!(
            flag.as_str(),
            "--window" | "--workspace" | "--pane" | "--surface" | "--tab" | "--expect-revision"
        )
    }) {
        let flag = args[0].clone();
        let target = value(&args, 0, &flag)?.to_owned();
        prefix.extend([flag, target]);
        args.drain(..2);
    }
    // Legacy open/navigation consumes leading target options before its URL.
    // Automation consumes options anywhere before the literal operand delimiter.
    let prefix_at = if args
        .first()
        .is_some_and(|verb| matches!(verb.as_str(), "open" | "navigate" | "goto"))
    {
        1
    } else {
        args.iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len())
    };
    args.splice(prefix_at..prefix_at, prefix);
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(CliError(
            "browser requires a subcommand; run `zmux help browser`".into(),
        ));
    };
    if verb == "download" {
        return super::browser_download::parse(&args[1..]);
    }
    if args.get(1).is_some_and(|verb| verb == "download") {
        let target = args[0].clone();
        let mut download = args[2..].to_vec();
        let before_literal = download
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(download.len());
        download.splice(before_literal..before_literal, ["--surface".into(), target]);
        return super::browser_download::parse(&download);
    }
    if matches!(verb, "open" | "navigate" | "goto") {
        return targets::parse(
            "browser",
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
        );
    }
    let interaction = match verb {
        "dblclick" => Some(BrowserInteraction::Dblclick),
        "hover" => Some(BrowserInteraction::Hover),
        "focus" => Some(BrowserInteraction::Focus),
        "check" => Some(BrowserInteraction::Check),
        "uncheck" => Some(BrowserInteraction::Uncheck),
        "scroll-into-view" | "scrollintoview" | "scrollinto" => {
            Some(BrowserInteraction::ScrollIntoView)
        }
        _ => None,
    };
    if interaction.is_none()
        && !matches!(
            verb,
            "get"
                | "is"
                | "find"
                | "wait"
                | "eval"
                | "snapshot"
                | "back"
                | "forward"
                | "reload"
                | "zoom"
                | "fill"
                | "type"
                | "select"
                | "scroll"
                | "click"
                | "press"
                | "key"
                | "keydown"
                | "keyup"
        )
    {
        return Err(CliError(format!("unknown browser subcommand `{verb}`")));
    }
    if help_requested(&tail[1..]) {
        return help();
    }
    let (kind, values_start) = if verb == "get" || verb == "is" {
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
            "visible" => GetKind::Visible,
            "enabled" => GetKind::Enabled,
            "checked" => GetKind::Checked,
            _ => return Err(CliError(format!("unknown browser get kind `{value}`"))),
        };
        if verb == "is" && !matches!(kind, GetKind::Visible | GetKind::Enabled | GetKind::Checked) {
            return Err(CliError(
                "browser is requires visible, enabled or checked".into(),
            ));
        }
        if help_requested(&tail[2..]) {
            return help();
        }
        (Some(kind), 2)
    } else {
        (None, 1)
    };
    let locator_kind = if verb == "find" {
        Some(match args.get(1).map(String::as_str) {
            Some("role") => LocatorKind::Role, Some("text") => LocatorKind::Text,
            Some("label") => LocatorKind::Label, Some("placeholder") => LocatorKind::Placeholder,
            Some("alt") => LocatorKind::Alt, Some("title") => LocatorKind::Title,
            Some("testid") => LocatorKind::Testid, Some("first") => LocatorKind::First,
            Some("last") => LocatorKind::Last, Some("nth") => LocatorKind::Nth,
            _ => return Err(CliError("browser find requires role, text, label, placeholder, alt, title, testid, first, last or nth".into())),
        })
    } else {
        None
    };
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut selector = None;
    let mut script = None;
    let mut fill_text = None;
    let mut selected_value = None;
    let mut dx = None;
    let mut dy = None;
    let mut attribute = None;
    let mut property = None;
    let mut max_depth = None;
    let mut interactive = false;
    let mut cursor = false;
    let mut compact = false;
    let mut snapshot_after = false;
    let mut name = None;
    let mut exact = false;
    let mut locator_index = None;
    let mut wait_state = None;
    let mut wait_timeout = None;
    let mut values = Vec::new();
    let mut index = if verb == "find" { 2 } else { values_start };
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--" {
            values.extend_from_slice(&args[index + 1..]);
            break;
        }
        let target_slot = match flag {
            "--window" => Some(&mut scope.window),
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
            "--selector"
                if interaction.is_some()
                    || matches!(
                        verb,
                        "get"
                            | "is"
                            | "snapshot"
                            | "find"
                            | "wait"
                            | "fill"
                            | "type"
                            | "select"
                            | "scroll"
                            | "click"
                    ) =>
            {
                set(&mut selector, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--text" if matches!(verb, "fill" | "type") => {
                let text = args.get(index + 1).ok_or_else(|| {
                    CliError("--text requires a value (which may be empty)".into())
                })?;
                set(&mut fill_text, text, flag)?;
                index += 2;
            }
            "--value" if verb == "select" => {
                let text = args
                    .get(index + 1)
                    .ok_or_else(|| CliError("--value requires a value".into()))?;
                set(&mut selected_value, text, flag)?;
                index += 2;
            }
            "--dx" | "--dy" if verb == "scroll" => {
                let slot = if flag == "--dx" { &mut dx } else { &mut dy };
                if slot.is_some() {
                    return Err(CliError(format!("repeated {flag}")));
                }
                *slot =
                    Some(value(&args, index, flag)?.parse::<i64>().map_err(|_| {
                        CliError(format!("{flag} requires an integer pixel delta"))
                    })?);
                index += 2;
            }
            "--name" if verb == "find" => {
                set(&mut name, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--exact" if verb == "find" => {
                flag_once(&mut exact, flag)?;
                index += 1;
            }
            "--index" if verb == "find" => {
                if locator_index.is_some() {
                    return Err(CliError("repeated --index".into()));
                }
                locator_index = Some(
                    value(&args, index, flag)?
                        .parse::<u32>()
                        .map_err(|_| CliError("--index must be 0..4095".into()))?,
                );
                index += 2;
            }
            "--state" if verb == "wait" => {
                if wait_state.is_some() {
                    return Err(CliError("repeated --state".into()));
                }
                wait_state = Some(match value(&args, index, flag)? {
                    "attached" => WaitState::Attached,
                    "detached" => WaitState::Detached,
                    "visible" => WaitState::Visible,
                    "hidden" => WaitState::Hidden,
                    "enabled" => WaitState::Enabled,
                    "disabled" => WaitState::Disabled,
                    "checked" => WaitState::Checked,
                    "unchecked" => WaitState::Unchecked,
                    _ => return Err(CliError("unknown browser wait state".into())),
                });
                index += 2;
            }
            "--timeout-ms" if verb == "wait" => {
                if wait_timeout.is_some() {
                    return Err(CliError("repeated --timeout-ms".into()));
                }
                let millis = value(&args, index, flag)?
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (1..=30000).contains(n))
                    .ok_or_else(|| CliError("--timeout-ms must be 1..30000".into()))?;
                wait_timeout = Some(Duration::from_millis(millis));
                index += 2;
            }
            "--script" if verb == "eval" => {
                set(&mut script, value(&args, index, flag)?, flag)?;
                index += 2;
            }
            "--key" if matches!(verb, "press" | "key" | "keydown" | "keyup") => {
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
            "--snapshot-after"
                if interaction.is_some()
                    || matches!(
                        verb,
                        "back"
                            | "forward"
                            | "reload"
                            | "fill"
                            | "type"
                            | "select"
                            | "scroll"
                            | "click"
                            | "press"
                            | "key"
                            | "keydown"
                            | "keyup"
                    ) =>
            {
                flag_once(&mut snapshot_after, flag)?;
                index += 1;
            }
            other
                if (verb == "zoom" && other.parse::<f64>().is_ok())
                    || (verb == "scroll" && other.parse::<i64>().is_ok()) =>
            {
                values.push(args[index].clone());
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
        "press" | "key" | "keydown" | "keyup" => {
            let key = match (script, values.as_slice()) {
                (Some(key), []) => key,
                (None, [key]) => key.clone(),
                _ => {
                    return Err(CliError(
                        "browser keyboard requires exactly one --key or positional key".into(),
                    ));
                }
            };
            let operation = match verb {
                "keydown" => BrowserKeyboardOperation::Down,
                "keyup" => BrowserKeyboardOperation::Up,
                _ => BrowserKeyboardOperation::Press,
            };
            BrowserAction::Keyboard {
                operation,
                key,
                snapshot_after,
            }
        }
        _ if interaction.is_some() || verb == "click" => {
            if selector.is_none() && values.len() == 1 {
                selector = values.pop();
            }
            if !values.is_empty() {
                return Err(CliError(format!(
                    "browser {verb} accepts exactly one selector"
                )));
            }
            let selector =
                selector.ok_or_else(|| CliError(format!("browser {verb} requires a selector")))?;
            if let Some(operation) = interaction {
                BrowserAction::Interact {
                    operation,
                    selector,
                    snapshot_after,
                }
            } else {
                BrowserAction::Click {
                    selector,
                    snapshot_after,
                }
            }
        }
        "fill" | "type" => {
            if selector.is_none() && !values.is_empty() {
                selector = Some(values.remove(0));
            }
            let selector =
                selector.ok_or_else(|| CliError(format!("browser {verb} requires a selector")))?;
            let text = fill_text.unwrap_or_else(|| values.join(" "));
            if verb == "type" {
                BrowserAction::Type {
                    selector,
                    text,
                    snapshot_after,
                }
            } else {
                BrowserAction::Fill {
                    selector,
                    text,
                    snapshot_after,
                }
            }
        }
        "select" => {
            if selector.is_none() && !values.is_empty() {
                selector = Some(values.remove(0));
            }
            let selector =
                selector.ok_or_else(|| CliError("browser select requires a selector".into()))?;
            let value = match selected_value {
                Some(value) => value,
                None if !values.is_empty() => values.remove(0),
                None => return Err(CliError("browser select requires a value".into())),
            };
            if !values.is_empty() {
                return Err(CliError(
                    "browser select accepts exactly one selector and value".into(),
                ));
            }
            BrowserAction::Select {
                selector,
                value,
                snapshot_after,
            }
        }
        "scroll" => {
            if values.len() > 1 || (dy.is_some() && !values.is_empty()) {
                return Err(CliError(
                    "browser scroll accepts one positional delta or --dy".into(),
                ));
            }
            if let Some(value) = values.pop() {
                dy = Some(value.parse::<i64>().map_err(|_| {
                    CliError("browser scroll requires an integer pixel delta".into())
                })?);
            }
            BrowserAction::Scroll {
                selector,
                dx: dx.unwrap_or(0),
                dy: dy.unwrap_or(0),
                snapshot_after,
            }
        }
        "zoom" => {
            if values.len() != 1 {
                return Err(CliError(
                    "browser zoom requires in, out, reset or one factor".into(),
                ));
            }
            let operation = match values[0].to_ascii_lowercase().as_str() {
                "in" => BrowserZoom::In,
                "out" => BrowserZoom::Out,
                "reset" => BrowserZoom::Reset,
                factor => BrowserZoom::Set {
                    factor: factor.parse::<f64>().map_err(|_| {
                        CliError(
                            "browser zoom requires in, out, reset or a finite numeric factor"
                                .into(),
                        )
                    })?,
                },
            };
            BrowserAction::Zoom { operation }
        }
        "find" => {
            if values.len() > 1 {
                return Err(CliError("browser find accepts one query".into()));
            }
            BrowserAction::Find {
                locator: BrowserLocator {
                    kind: locator_kind.expect("parsed find kind"),
                    query: values.pop(),
                    name,
                    exact,
                    selector,
                    index: locator_index,
                },
            }
        }
        "wait" => {
            if selector.is_none() && values.len() == 1 {
                selector = values.pop();
            }
            if !values.is_empty() {
                return Err(CliError("conflicting browser wait selectors".into()));
            }
            BrowserAction::Wait {
                selector: selector.ok_or_else(|| {
                    CliError("browser wait requires --selector or a selector operand".into())
                })?,
                state: wait_state.unwrap_or_default(),
            }
        }
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
        "get" | "is" => {
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
    Ok(Parsed::Browser(
        ControlCommand::Browser {
            request: BrowserRequest {
                scope,
                caller,
                expected_model_revision: revision,
                action,
                timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
            },
        },
        wait_timeout,
    ))
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
    wait_timeout: Option<Duration>,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    if let ControlCommand::Browser { request } = &mut command {
        let budget = deadline.saturating_duration_since(Instant::now());
        // The runtime execution clock starts at admission, after socket and
        // executor latency. Reserve time inside the original client deadline
        // for that admission and for observing the terminal receipt. A fixed
        // 100 ms reserve can expire before the runtime even starts its clock.
        // Short commands split their budget; longer commands reserve at most
        // one second for receipt observation rather than extending --timeout.
        let receipt_budget = (budget / 3)
            .clamp(Duration::from_millis(250), Duration::from_secs(1))
            .min(budget / 2);
        request.timeout_ms = budget
            .saturating_sub(receipt_budget)
            .min(wait_timeout.unwrap_or(Duration::MAX))
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

pub(super) fn run_profile(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    let mut receipt = response_result(request(&remaining(options, deadline, None)?, command)?)?;
    let id = receipt["operation_id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| CliError("invalid profile operation receipt".into()))?;
    let epoch = receipt["epoch"]
        .as_str()
        .ok_or_else(|| CliError("profile receipt missing runtime owner".into()))?
        .to_owned();
    let owner = epoch
        .parse()
        .map_err(|_| CliError("invalid profile runtime owner".into()))?;
    loop {
        if receipt["epoch"].as_str() != Some(epoch.as_str())
            || receipt["operation_id"].as_u64() != Some(id)
        {
            return Err(CliError("mismatched profile operation receipt".into()));
        }
        match receipt["state"].as_str() {
            Some("completed") => {
                return receipt
                    .get("profile")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .ok_or_else(|| CliError("profile completion missing identity".into()));
            }
            Some("failed") => {
                return Err(CliError(
                    receipt["error"]
                        .as_str()
                        .unwrap_or("browser profile operation failed")
                        .into(),
                ));
            }
            Some("pending") => {}
            _ => return Err(CliError("invalid profile receipt state".into())),
        }
        let budget = remaining(options, deadline, Some(id))?;
        thread::sleep(Duration::from_millis(20).min(budget.timeout));
        receipt = response_result(request(
            &remaining(options, deadline, Some(id))?,
            ControlCommand::BrowserProfilesStatus {
                operation_id: Some(id),
                epoch: Some(owner),
            },
        )?)?;
    }
}
