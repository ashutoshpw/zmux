//! The move command's window flag is a destination, never a source constraint.
use super::*;
use zmux_desktop_runtime::targets::{TargetOperation, TargetRequest};
use zmux_workspace_core::targets::{CallerContext, TargetAction, TargetRef, TargetScope};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(
            "move-workspace-to-window".into(),
        ))));
    }
    let args = utf8_args(tail)?;
    let (mut source, mut destination, mut revision) = (None, None, None);
    let mut no_focus = false;
    let mut focus = None;
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--no-focus" {
            if no_focus || focus.is_some() {
                return Err(CliError(
                    "Repeated --no-focus or conflict with --focus".into(),
                ));
            }
            no_focus = true;
            cursor += 1;
            continue;
        }
        if flag == "--focus" && (focus.is_some() || no_focus) {
            return Err(CliError(
                "Repeated --focus or conflict with --no-focus".into(),
            ));
        }
        let value = args
            .get(cursor + 1)
            .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
        match flag {
            "--focus" if focus.is_none() && !no_focus => {
                focus = Some(match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(CliError("--focus requires true or false".into())),
                })
            }
            "--workspace" if source.is_none() => source = Some(target(value)?),
            "--window" if destination.is_none() => destination = Some(target(value)?),
            "--expect-revision" if revision.is_none() => {
                revision =
                    Some(value.parse::<u64>().map_err(|_| {
                        CliError("model revision must be a nonnegative integer".into())
                    })?)
            }
            _ => return Err(CliError(format!("Unknown or repeated move option {flag}"))),
        }
        cursor += 2;
    }
    let workspace = source
        .ok_or_else(|| CliError("move-workspace-to-window requires --workspace HANDLE".into()))?;
    let destination = destination
        .ok_or_else(|| CliError("move-workspace-to-window requires --window DESTINATION".into()))?;
    Ok(Parsed::WorkspaceMove(ControlCommand::Targeted {
        request: TargetRequest {
            scope: TargetScope {
                workspace: Some(workspace),
                ..Default::default()
            },
            caller: CallerContext::default(),
            expected_model_revision: revision,
            operation: TargetOperation::Action {
                action: TargetAction::MoveWorkspaceToWindow {
                    destination,
                    focus: focus.unwrap_or(false),
                },
            },
        },
    }))
}
fn target(value: &str) -> Result<TargetRef, CliError> {
    value
        .parse()
        .map_err(|error: zmux_workspace_core::targets::TargetError| CliError(error.to_string()))
}
pub(super) fn run(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    let budget = windows::remaining(options, deadline)?;
    let mut result = response_result(request(&budget, command)?)?;
    if result["state"] != "topology_committed" {
        return Err(CliError("Move response omitted its topology commitment; inspect current membership before another mutation".into()));
    }
    if result["focus_requested"] == true && result["native_focus"].is_null() {
        return Err(CliError("Workspace topology already committed, but its requested native focus receipt is missing; inspect window state before another mutation".into()));
    }
    if let Some(receipt) = result
        .get("native_focus")
        .filter(|value| !value.is_null())
        .cloned()
    {
        if receipt["window_id"] != result["destination_window_id"]
            || receipt["window_uuid"] != result["destination_window_uuid"]
        {
            return Err(CliError("Workspace topology already committed, but its native focus receipt belongs to another destination; mutation was not replayed".into()));
        }
        match windows::wait_receipt(options, receipt, "focus", deadline) {
            Ok(receipt) => result["native_focus"] = receipt,
            Err(error) => {
                return Err(CliError(format!(
                    "Workspace {} topology already committed to window {}; native focus failed or is unknown: {error}. Move was not replayed or rolled back",
                    result["workspace_id"], result["destination_window_id"]
                )));
            }
        }
    }
    Ok(result)
}
