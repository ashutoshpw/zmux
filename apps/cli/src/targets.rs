//! Target grammar for existing public commands. Native and CLI requests converge on shared actions.
use super::*;
use zmux_desktop_runtime::{
    attention::{DismissSelection, NotificationAction, NotificationScope, NotificationSelection},
    targets::{TargetOperation, TargetRequest},
};
use zmux_workspace_core::targets::{CallerContext, TargetAction, TargetRef, TargetScope};
use zmux_workspace_core::{Direction, Navigation};

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let mut args = utf8_args(tail)?;
    if matches!(
        command,
        "notify" | "mark-notification-read" | "dismiss-notification" | "clear-notifications"
    ) {
        return parse_notifications(command, &args);
    }
    if matches!(
        command,
        "set-status"
            | "clear-status"
            | "list-status"
            | "set-progress"
            | "clear-progress"
            | "log"
            | "clear-log"
            | "list-log"
    ) {
        return parse_metadata(command, &args);
    }
    if navigation_command(command) {
        return parse_navigation(command, &args);
    }
    let browser_verb = if command == "browser" {
        if args.as_slice() == ["help"] {
            return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
        }
        if !args
            .first()
            .is_some_and(|value| ["open", "navigate", "goto"].contains(&value.as_str()))
        {
            return Err(CliError("browser requires open or navigate".into()));
        }
        if help_requested(&tail[1..]) {
            return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
        }
        Some(args.remove(0))
    } else {
        None
    };
    let browser_focus = if browser_verb.as_deref() == Some("open") {
        extract_browser_focus(&mut args)?
    } else {
        true
    };
    let mut scope = TargetScope::default();
    let mut expected_model_revision = None;
    let mut cursor = 0;
    let mut literal = false;
    // Selector flags lead positional arguments. `--` preserves literal option-looking names/text.
    while cursor < args.len() {
        let flag = &args[cursor];
        if flag == "--" {
            literal = true;
            cursor += 1;
            break;
        }
        let slot = match flag.as_str() {
            "--workspace" => Some(&mut scope.workspace),
            "--pane" => Some(&mut scope.pane),
            "--surface" | "--tab" => Some(&mut scope.surface),
            "--expect-revision" => {
                if expected_model_revision.is_some() {
                    return Err(CliError("repeated --expect-revision".into()));
                }
                expected_model_revision = Some(
                    args.get(cursor + 1)
                        .ok_or_else(|| {
                            CliError("--expect-revision requires a model revision".into())
                        })?
                        .parse::<u64>()
                        .map_err(|_| {
                            CliError("model revision must be a nonnegative integer".into())
                        })?,
                );
                cursor += 2;
                continue;
            }
            _ if flag.starts_with("--") && !(command == "send-text" && flag == "--stdin") => {
                return Err(CliError(format!(
                    "unknown {command} option {flag}; use -- before literal option-looking text"
                )));
            }
            _ => None,
        };
        let Some(slot) = slot else { break };
        if slot.is_some() {
            return Err(CliError(format!("repeated {flag}")));
        }
        *slot =
            Some(target(args.get(cursor + 1).ok_or_else(|| {
                CliError(format!("{flag} requires a target"))
            })?)?);
        cursor += 2;
    }
    let values = &args[cursor..];
    let action = match command {
        "rename-workspace" => {
            let name = target_and_value(values, &mut scope.workspace, "workspace and name")?;
            TargetAction::RenameWorkspace { name }
        }
        "close-workspace" => {
            optional_target(values, &mut scope.workspace)?;
            TargetAction::CloseWorkspace
        }
        "focus-pane" | "close-pane" | "new-surface" => {
            optional_target(values, &mut scope.pane)?;
            match command {
                "focus-pane" => TargetAction::FocusPane,
                "close-pane" => TargetAction::ClosePane,
                _ => TargetAction::CreateSurface,
            }
        }
        "split-pane" => {
            let axis = target_and_value(values, &mut scope.pane, "pane and horizontal|vertical")?;
            TargetAction::SplitPane {
                axis: match axis.as_str() {
                    "horizontal" => Axis::Horizontal,
                    "vertical" => Axis::Vertical,
                    _ => return Err(CliError("split axis must be horizontal or vertical".into())),
                },
            }
        }
        "select-surface" | "close-surface" | "close-other-tabs" => {
            optional_target(values, &mut scope.surface)?;
            match command {
                "select-surface" => TargetAction::SelectSurface,
                "close-surface" => TargetAction::CloseSurface,
                _ => TargetAction::CloseOtherSurfaces,
            }
        }
        "rename-tab" => TargetAction::RenameSurface {
            name: target_and_value(values, &mut scope.surface, "surface and name")?,
        },
        "reorder-surface" => {
            let index = target_and_value(values, &mut scope.surface, "surface and index")?;
            TargetAction::ReorderSurface {
                index: index
                    .parse()
                    .map_err(|_| CliError("index must be a nonnegative integer".into()))?,
            }
        }
        "move-surface" => {
            let (source, destination, index) = match values {
                [source, destination, index] if scope.surface.is_none() => {
                    (Some(source), destination, index)
                }
                [destination, index] => (None, destination, index),
                _ => {
                    return Err(CliError(
                        "usage: zmux move-surface [SURFACE] DESTINATION_PANE INDEX".into(),
                    ));
                }
            };
            if let Some(source) = source {
                scope.surface = Some(target(source)?);
            }
            TargetAction::MoveSurface {
                destination: target(destination)?,
                index: index
                    .parse()
                    .map_err(|_| CliError("index must be a nonnegative integer".into()))?,
            }
        }
        "browser" => match browser_verb.as_deref() {
            Some("open") => TargetAction::CreateBrowser {
                url: target_and_value(values, &mut scope.pane, "pane and URL")?,
                focus: browser_focus,
            },
            Some("navigate" | "goto") => TargetAction::NavigateBrowser {
                url: target_and_value(values, &mut scope.surface, "surface and URL")?,
            },
            _ => return Err(CliError("browser requires open or navigate".into())),
        },
        "list-panes" => {
            if !values.is_empty() {
                return Err(CliError(
                    "list-panes accepts leading target options only".into(),
                ));
            }
            return request(scope, expected_model_revision, TargetOperation::ListPanes);
        }
        "list-pane-surfaces" => {
            optional_target(values, &mut scope.pane)?;
            return request(
                scope,
                expected_model_revision,
                TargetOperation::ListPaneSurfaces,
            );
        }
        "read-screen" => {
            optional_target(values, &mut scope.pane)?;
            return request(scope, expected_model_revision, TargetOperation::ReadScreen);
        }
        "paste-files" => {
            if values.is_empty() {
                return Err(CliError(
                    "paste-files requires paths after target options".into(),
                ));
            }
            let cwd = env::current_dir()?;
            let paths = values
                .iter()
                .map(|s| {
                    let path = PathBuf::from(s);
                    if path.is_absolute() {
                        path
                    } else {
                        cwd.join(path)
                    }
                })
                .collect();
            return request(
                scope,
                expected_model_revision,
                TargetOperation::PasteFiles { paths },
            );
        }
        "send-text" => {
            let text = target_and_value(values, &mut scope.pane, "pane and text|--stdin")?;
            let text = if !literal && text == "--stdin" {
                let mut bytes = Vec::new();
                io::stdin().take(4_097).read_to_end(&mut bytes)?;
                if bytes.len() > 4_096 {
                    return Err(CliError("stdin text exceeds 4096 bytes".into()));
                }
                String::from_utf8(bytes).map_err(|_| CliError("stdin is not valid UTF-8".into()))?
            } else {
                text
            };
            return request(
                scope,
                expected_model_revision,
                TargetOperation::SendText { text },
            );
        }
        _ => return Err(CliError("command has no target contract".into())),
    };
    request(
        scope,
        expected_model_revision,
        TargetOperation::Action { action },
    )
}
/// Notification selectors are parsed independently of text-bearing positional commands.
/// All scope references remain symbolic until the desktop executor accepts the request.
fn parse_notifications(command: &str, args: &[String]) -> Result<Parsed, CliError> {
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut title = None;
    let mut subtitle = None;
    let mut body = None;
    let mut desktop = None;
    let mut reply = false;
    let mut clear = false;
    let mut all = false;
    let mut all_read = false;
    let mut id = None;
    let mut through_id = None;
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        let slot = match flag {
            "--workspace" => Some(&mut scope.workspace),
            "--surface" | "--tab" => Some(&mut scope.surface),
            "--pane" if command == "notify" => Some(&mut scope.pane),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return Err(CliError(format!(
                    "invalid or repeated {command} option {flag}"
                )));
            }
            *slot = Some(target(option_value(args, cursor, flag)?)?);
            cursor += 2;
            continue;
        }
        match flag {
            "--title" | "--subtitle" | "--body" if command == "notify" => {
                let slot = match flag {
                    "--title" => &mut title,
                    "--subtitle" => &mut subtitle,
                    _ => &mut body,
                };
                if slot.is_some() {
                    return Err(CliError(format!(
                        "invalid or repeated notify option {flag}"
                    )));
                }
                *slot = Some(option_value(args, cursor, flag)?.to_owned());
                cursor += 2;
            }
            "--desktop" if command == "notify" && desktop.is_none() => {
                desktop = Some(match option_value(args, cursor, flag)? {
                    "true" => true,
                    "false" => false,
                    _ => return Err(CliError("--desktop requires true or false".into())),
                });
                cursor += 2;
            }
            "--reply" if command == "notify" && !reply => {
                reply = true;
                cursor += 1;
            }
            "--clear" if command == "notify" && !clear => {
                clear = true;
                cursor += 1;
            }
            "--all" if command == "mark-notification-read" && !all => {
                all = true;
                cursor += 1;
            }
            "--all-read" if command == "dismiss-notification" && !all_read => {
                all_read = true;
                cursor += 1;
            }
            "--id"
                if matches!(command, "mark-notification-read" | "dismiss-notification")
                    && id.is_none() =>
            {
                id = Some(parse_notification_selector(option_value(
                    args, cursor, flag,
                )?)?);
                cursor += 2;
            }
            "--through" if command == "clear-notifications" && through_id.is_none() => {
                through_id = Some(option_value(args, cursor, flag)?.parse::<u64>().map_err(
                    |_| CliError("--through requires a nonnegative notification cursor".into()),
                )?);
                cursor += 2;
            }
            "--expect-revision" if revision.is_none() => {
                revision = Some(option_value(args, cursor, flag)?.parse::<u64>().map_err(
                    |_| CliError("model revision must be a nonnegative integer".into()),
                )?);
                cursor += 2;
            }
            _ => {
                return Err(CliError(format!(
                    "invalid or repeated {command} option {flag}"
                )));
            }
        }
    }
    let scoped = scope.workspace.is_some() || scope.surface.is_some() || scope.pane.is_some();
    let action = match command {
        "notify" if !clear => {
            return request(
                scope,
                revision,
                TargetOperation::Notify {
                    title: title.unwrap_or_else(|| "Notification".into()),
                    subtitle: subtitle.unwrap_or_default(),
                    body: body.unwrap_or_default(),
                    desktop,
                    reply,
                },
            );
        }
        "notify" => {
            if title.is_some() || subtitle.is_some() || body.is_some() || desktop.is_some() || reply
            {
                return Err(CliError(
                    "notify --clear conflicts with content, --desktop and --reply options".into(),
                ));
            }
            if scope.pane.is_some() {
                return Err(CliError(
                    "notify --clear accepts workspace and surface scope only".into(),
                ));
            }
            let notification_scope =
                explicit_notification_scope(&mut scope)?.unwrap_or(NotificationScope::Caller);
            NotificationAction::Clear {
                scope: notification_scope,
                through_id: None,
                expected_epoch: None,
            }
        }
        "mark-notification-read" => {
            if usize::from(id.is_some()) + usize::from(all) + usize::from(scoped) != 1 {
                return Err(CliError("mark-notification-read requires exactly one of --id, --workspace, or --all; --surface further limits a workspace".into()));
            }
            let selection = if let Some(id) = id {
                NotificationSelection::Id { id }
            } else if all {
                NotificationSelection::All
            } else {
                explicit_notification_scope(&mut scope)?;
                NotificationSelection::Workspace
            };
            NotificationAction::MarkRead { selection }
        }
        "dismiss-notification" => {
            if scoped || usize::from(id.is_some()) + usize::from(all_read) != 1 {
                return Err(CliError("dismiss-notification requires exactly one of --id or --all-read and accepts no target scope".into()));
            }
            NotificationAction::Dismiss {
                selection: match id {
                    Some(id) => DismissSelection::Id { id },
                    None => DismissSelection::AllRead,
                },
            }
        }
        "clear-notifications" => {
            let notification_scope = match explicit_notification_scope(&mut scope)? {
                Some(scope) => scope,
                None => match environment_target("ZMUX_WORKSPACE_ID")? {
                    Some(workspace) => {
                        scope.workspace = Some(workspace);
                        NotificationScope::Workspace
                    }
                    None => NotificationScope::All,
                },
            };
            NotificationAction::Clear {
                scope: notification_scope,
                through_id,
                expected_epoch: None,
            }
        }
        _ => return Err(CliError("command has no notification contract".into())),
    };
    request(scope, revision, TargetOperation::Notifications { action })
}

fn explicit_notification_scope(
    scope: &mut TargetScope,
) -> Result<Option<NotificationScope>, CliError> {
    if scope.surface.is_some() {
        if scope.workspace.is_none() {
            scope.workspace = environment_target("ZMUX_WORKSPACE_ID")?;
        }
        if scope.workspace.is_none() {
            return Err(CliError(
                "--surface requires --workspace or ZMUX_WORKSPACE_ID for notification mutations"
                    .into(),
            ));
        }
        Ok(Some(NotificationScope::Surface))
    } else if scope.workspace.is_some() {
        Ok(Some(NotificationScope::Workspace))
    } else {
        Ok(None)
    }
}

fn extract_browser_focus(args: &mut Vec<String>) -> Result<bool, CliError> {
    let mut focus = None;
    let mut retained = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                retained.extend_from_slice(&args[index..]);
                break;
            }
            "--no-focus" => {
                if focus.replace(false).is_some() {
                    return Err(CliError(
                        "repeated or conflicting browser focus option".into(),
                    ));
                }
                index += 1;
            }
            "--focus" => {
                if focus.is_some() {
                    return Err(CliError(
                        "repeated or conflicting browser focus option".into(),
                    ));
                }
                focus = Some(match args.get(index + 1).map(String::as_str) {
                    Some("true") => true,
                    Some("false") => false,
                    _ => return Err(CliError("--focus requires true or false".into())),
                });
                index += 2;
            }
            "--workspace" | "--pane" | "--surface" | "--tab" | "--expect-revision" => {
                retained.push(args[index].clone());
                index += 1;
                if let Some(value) = args.get(index) {
                    retained.push(value.clone());
                    index += 1;
                }
            }
            other if other.starts_with('-') => {
                return Err(CliError(format!("unknown browser open option `{other}`")));
            }
            _ => {
                retained.push(args[index].clone());
                index += 1;
            }
        }
    }
    *args = retained;
    Ok(focus.unwrap_or(true))
}
fn request(
    scope: TargetScope,
    expected_model_revision: Option<u64>,
    operation: TargetOperation,
) -> Result<Parsed, CliError> {
    // Global and exact-ID attention mutations have no caller/focus fallback.
    let caller = if matches!(
        &operation,
        TargetOperation::Notifications {
            action: NotificationAction::MarkRead {
                selection: NotificationSelection::Id { .. } | NotificationSelection::All
            } | NotificationAction::Dismiss { .. }
                | NotificationAction::Clear {
                    scope: NotificationScope::All,
                    ..
                }
        }
    ) {
        CallerContext::default()
    } else if matches!(&operation, TargetOperation::Notify { .. })
        && scope.workspace.is_some()
        && scope.pane.is_none()
        && scope.surface.is_none()
    {
        // An explicit workspace constrains notification attribution. Preserve
        // terminal evidence separately so the executor can accept a live caller
        // in that workspace without borrowing the workspace's selected tab.
        CallerContext {
            workspace: None,
            surface: match environment_target("ZMUX_SURFACE_ID")? {
                Some(surface) => Some(surface),
                None => environment_target("ZMUX_TAB_ID")?,
            },
        }
    } else {
        caller(&scope)?
    };
    Ok(Parsed::App(ControlCommand::Targeted {
        request: TargetRequest {
            scope,
            caller,
            expected_model_revision,
            operation,
        },
    }))
}
pub(super) fn caller(scope: &TargetScope) -> Result<CallerContext, CliError> {
    Ok(
        if scope.workspace.is_none() && scope.pane.is_none() && scope.surface.is_none() {
            CallerContext {
                workspace: environment_target("ZMUX_WORKSPACE_ID")?,
                surface: match environment_target("ZMUX_SURFACE_ID")? {
                    Some(surface) => Some(surface),
                    None => environment_target("ZMUX_TAB_ID")?,
                },
            }
        } else {
            CallerContext::default()
        },
    )
}
fn environment_target(name: &str) -> Result<Option<TargetRef>, CliError> {
    match env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => target(&value)
            .map(Some)
            .map_err(|e| CliError(format!("{name}: {e}"))),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(CliError(format!("{name} must contain valid UTF-8"))),
    }
}
fn target(value: &str) -> Result<TargetRef, CliError> {
    value.parse().map_err(|e| CliError(format!("{e}")))
}
fn optional_target(values: &[String], slot: &mut Option<TargetRef>) -> Result<(), CliError> {
    match values {
        [] => Ok(()),
        [value] if slot.is_none() => {
            *slot = Some(target(value)?);
            Ok(())
        }
        _ => Err(CliError(
            "expected at most one target; do not repeat a positional and named selector".into(),
        )),
    }
}
fn target_and_value(
    values: &[String],
    slot: &mut Option<TargetRef>,
    label: &str,
) -> Result<String, CliError> {
    match values {
        [value] => Ok(value.clone()),
        [selector, value] if slot.is_none() => {
            *slot = Some(target(selector)?);
            Ok(value.clone())
        }
        _ => Err(CliError(format!(
            "expected {label}; use leading selector options or the legacy positional target"
        ))),
    }
}

fn navigation_command(command: &str) -> bool {
    matches!(
        command,
        "new-split"
            | "select-workspace"
            | "next-workspace"
            | "previous-workspace"
            | "last-workspace"
            | "next-tab"
            | "previous-tab"
            | "last-tab"
            | "next-pane"
            | "previous-pane"
            | "last-pane"
            | "focus-direction"
            | "tab-action"
    )
}

/// These navigation commands allow options on either side of their positional verb.
/// Existing text-bearing commands retain their delimiter/leading-selector grammar.
fn parse_navigation(command: &str, args: &[String]) -> Result<Parsed, CliError> {
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut focus = None;
    let mut action_name = None;
    let mut title = None;
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--" {
            values.extend_from_slice(&args[cursor + 1..]);
            break;
        }
        let workspace_only = matches!(
            command,
            "select-workspace" | "next-workspace" | "previous-workspace" | "last-workspace"
        );
        let slot = match flag {
            "--workspace" => Some(&mut scope.workspace),
            "--pane" if !workspace_only => Some(&mut scope.pane),
            "--surface" | "--tab" if !workspace_only => Some(&mut scope.surface),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return Err(CliError(format!("repeated {flag} selector")));
            }
            *slot = Some(target(option_value(args, cursor, flag)?)?);
            cursor += 2;
            continue;
        }
        match flag {
            "--expect-revision" => {
                if revision.is_some() {
                    return Err(CliError("repeated --expect-revision".into()));
                }
                revision = Some(option_value(args, cursor, flag)?.parse::<u64>().map_err(
                    |_| CliError("model revision must be a nonnegative integer".into()),
                )?);
                cursor += 2;
            }
            "--focus" if command == "new-split" => {
                if focus.is_some() {
                    return Err(CliError("repeated or conflicting focus option".into()));
                }
                focus = Some(match option_value(args, cursor, flag)? {
                    "true" => true,
                    "false" => false,
                    _ => return Err(CliError("--focus requires true or false".into())),
                });
                cursor += 2;
            }
            "--no-focus" if command == "new-split" => {
                if focus.replace(false).is_some() {
                    return Err(CliError("repeated or conflicting focus option".into()));
                }
                cursor += 1;
            }
            "--action" if command == "tab-action" => {
                if action_name.is_some() {
                    return Err(CliError("repeated --action".into()));
                }
                action_name = Some(option_value(args, cursor, flag)?.to_owned());
                cursor += 2;
            }
            "--title" if command == "tab-action" => {
                if title.is_some() {
                    return Err(CliError("repeated --title".into()));
                }
                title = Some(option_value(args, cursor, flag)?.to_owned());
                cursor += 2;
            }
            other if other.starts_with('-') => {
                return Err(CliError(format!("unknown {command} option {other}")));
            }
            _ => {
                values.push(args[cursor].clone());
                cursor += 1;
            }
        }
    }
    let action = match command {
        "new-split" => TargetAction::SplitPaneDirectional {
            direction: parse_direction(&values)?,
            focus: focus.unwrap_or(false),
        },
        "focus-direction" => TargetAction::FocusPaneDirection {
            direction: parse_direction(&values)?,
        },
        "select-workspace" => {
            optional_target(&values, &mut scope.workspace)?;
            TargetAction::SelectWorkspace
        }
        "tab-action" => {
            let name = match action_name {
                Some(name) => name,
                None if !values.is_empty() => values.remove(0),
                None => return Err(CliError("tab-action requires an action".into())),
            }
            .to_lowercase()
            .replace('_', "-");
            match name.as_str() {
                "rename" => {
                    let name = match (title, values.is_empty()) {
                        (Some(title), true) => title,
                        (None, false) => values.join(" "),
                        _ => return Err(CliError(
                            "tab-action rename requires --title or trailing title words, not both"
                                .into(),
                        )),
                    };
                    TargetAction::RenameSurface { name }
                }
                "close-others" if title.is_none() && values.is_empty() => {
                    TargetAction::CloseOtherSurfaces
                }
                "close-others" => {
                    return Err(CliError(
                        "tab-action close-others accepts no title or trailing arguments".into(),
                    ));
                }
                _ => {
                    return Err(CliError(format!(
                        "unsupported tab-action {name}; supported actions: rename, close-others"
                    )));
                }
            }
        }
        _ => {
            if !values.is_empty() {
                return Err(CliError(format!("{command} accepts target options only")));
            }
            let direction = if command.starts_with("next-") {
                Navigation::Next
            } else if command.starts_with("previous-") {
                Navigation::Previous
            } else {
                Navigation::Last
            };
            if command.ends_with("workspace") {
                TargetAction::NavigateWorkspace { direction }
            } else if command.ends_with("tab") {
                TargetAction::NavigateSurface { direction }
            } else {
                TargetAction::NavigatePane { direction }
            }
        }
    };
    request(scope, revision, TargetOperation::Action { action })
}

fn option_value<'a>(args: &'a [String], cursor: usize, flag: &str) -> Result<&'a str, CliError> {
    args.get(cursor + 1)
        .map(String::as_str)
        .ok_or_else(|| CliError(format!("{flag} requires a value")))
}

fn parse_direction(values: &[String]) -> Result<Direction, CliError> {
    match values {
        [value] => match value.as_str() {
            "left" => Ok(Direction::Left),
            "right" => Ok(Direction::Right),
            "up" => Ok(Direction::Up),
            "down" => Ok(Direction::Down),
            _ => Err(CliError("direction must be left, right, up or down".into())),
        },
        _ => Err(CliError(
            "expected one direction: left, right, up or down".into(),
        )),
    }
}

fn parse_metadata(command: &str, args: &[String]) -> Result<Parsed, CliError> {
    use zmux_desktop_runtime::metadata::{LogLevel, MetadataAction};
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut options = std::collections::BTreeMap::new();
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--" {
            values.extend_from_slice(&args[cursor + 1..]);
            break;
        }
        if flag.starts_with("--") {
            if options.contains_key(flag) {
                return Err(CliError(format!("repeated {flag}")));
            }
            let accepted = match flag {
                "--workspace" | "--expect-revision" => true,
                "--icon" | "--color" | "--priority" => command == "set-status",
                "--label" => command == "set-progress",
                "--level" | "--source" => command == "log",
                "--limit" => command == "list-log",
                _ => false,
            };
            if !accepted {
                return Err(CliError(format!("unknown {command} option {flag}")));
            }
            let value = option_value(args, cursor, flag)?.to_owned();
            if flag == "--workspace" {
                scope.workspace = Some(target(&value)?);
            }
            if flag == "--expect-revision" {
                revision = Some(value.parse::<u64>().map_err(|_| {
                    CliError("model revision must be a nonnegative integer".into())
                })?);
            }
            options.insert(flag, value);
            cursor += 2;
        } else {
            values.push(args[cursor].clone());
            cursor += 1;
        }
    }
    let action = match (command, values.as_slice()) {
        ("set-status", [key, value]) => MetadataAction::SetStatus {
            key: key.clone(),
            value: value.clone(),
            icon: options.remove("--icon"),
            color: options.remove("--color"),
            priority: options
                .remove("--priority")
                .map(|v| {
                    v.parse::<i32>()
                        .map_err(|_| CliError("priority must be a 32-bit integer".into()))
                })
                .transpose()?
                .unwrap_or(0),
        },
        ("clear-status", [key]) => MetadataAction::ClearStatus { key: key.clone() },
        ("list-status", []) => MetadataAction::ListStatus,
        ("set-progress", [value]) => MetadataAction::SetProgress {
            value: value
                .parse::<f64>()
                .map_err(|_| CliError("progress must be a number from 0 to 1".into()))?,
            label: options.remove("--label"),
        },
        ("clear-progress", []) => MetadataAction::ClearProgress,
        ("log", [message]) => MetadataAction::Log {
            message: message.clone(),
            source: options.remove("--source"),
            level: match options.get("--level").map(String::as_str).unwrap_or("info") {
                "info" => LogLevel::Info,
                "progress" => LogLevel::Progress,
                "success" => LogLevel::Success,
                "warning" => LogLevel::Warning,
                "error" => LogLevel::Error,
                _ => {
                    return Err(CliError(
                        "level must be info, progress, success, warning, or error".into(),
                    ));
                }
            },
        },
        ("clear-log", []) => MetadataAction::ClearLog,
        ("list-log", []) => MetadataAction::ListLog {
            limit: options
                .remove("--limit")
                .map(|v| {
                    v.parse::<usize>()
                        .map_err(|_| CliError("log limit must be from 1 to 256".into()))
                })
                .transpose()?,
        },
        _ => {
            return Err(CliError(format!(
                "invalid {command} arguments; read zmux help {command}"
            )));
        }
    };
    action.validate().map_err(|e| CliError(e.to_string()))?;
    request(scope, revision, TargetOperation::Metadata { action })
}
