//! Target grammar for existing public commands. Native and CLI requests converge on shared actions.
use super::*;
use zmux_desktop_runtime::targets::{TargetOperation, TargetRequest};
use zmux_workspace_core::targets::{CallerContext, TargetAction, TargetRef, TargetScope};

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let mut args = utf8_args(tail)?;
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
            "--surface" => Some(&mut scope.surface),
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
            _ if flag.starts_with("--")
                && !(command == "notify" && ["--title", "--body"].contains(&flag.as_str()))
                && !(command == "send-text" && flag == "--stdin") =>
            {
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
        "notify" => {
            let mut title = None;
            let mut body = None;
            if values.len() % 2 != 0 {
                return Err(CliError("notify expects --title TEXT [--body TEXT]".into()));
            }
            for pair in values.chunks_exact(2) {
                match pair[0].as_str() {
                    "--title" if title.is_none() => title = Some(pair[1].clone()),
                    "--body" if body.is_none() => body = Some(pair[1].clone()),
                    // Preserve existing notify argument order as well as leading selector options.
                    "--surface" if scope.surface.is_none() => {
                        scope.surface = Some(target(&pair[1])?)
                    }
                    "--workspace" if scope.workspace.is_none() => {
                        scope.workspace = Some(target(&pair[1])?)
                    }
                    "--pane" if scope.pane.is_none() => scope.pane = Some(target(&pair[1])?),
                    other => {
                        return Err(CliError(format!(
                            "invalid or repeated notify option {other}"
                        )));
                    }
                }
            }
            return request(
                scope,
                expected_model_revision,
                TargetOperation::Notify {
                    title: title.ok_or_else(|| CliError("notify requires --title".into()))?,
                    body: body.unwrap_or_default(),
                },
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
fn request(
    scope: TargetScope,
    expected_model_revision: Option<u64>,
    operation: TargetOperation,
) -> Result<Parsed, CliError> {
    let caller = if scope.workspace.is_none() && scope.pane.is_none() && scope.surface.is_none() {
        CallerContext {
            workspace: environment_target("ZMUX_WORKSPACE_ID")?,
            surface: match environment_target("ZMUX_SURFACE_ID")? {
                Some(surface) => Some(surface),
                None => environment_target("ZMUX_TAB_ID")?,
            },
        }
    } else {
        CallerContext::default()
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
