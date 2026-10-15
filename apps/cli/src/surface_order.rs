//! Same-pane reorder grammar and exact, optional native focus acknowledgement.
use super::*;
use zmux_desktop_runtime::surface_order::{OrderPlacement, OrderRequest};
use zmux_workspace_core::targets::{TargetRef, TargetScope};

pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(
            "reorder-surface".into(),
        ))));
    }
    let args = utf8_args(tail)?;
    let mut scope = TargetScope::default();
    let (mut placement, mut revision, mut focus) = (None, None, None);
    let mut operands = Vec::new();
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--no-focus" && focus.is_none() {
            focus = Some(false);
            cursor += 1;
            continue;
        }
        if !flag.starts_with("--") {
            operands.push(flag);
            cursor += 1;
            continue;
        }
        let value = args
            .get(cursor + 1)
            .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
        match flag {
            "--surface" | "--tab" if scope.surface.is_none() => {
                scope.surface = Some(target(value)?)
            }
            "--pane" if scope.pane.is_none() => scope.pane = Some(target(value)?),
            "--workspace" if scope.workspace.is_none() => scope.workspace = Some(target(value)?),
            "--window" if scope.window.is_none() => scope.window = Some(target(value)?),
            "--index" if placement.is_none() => {
                placement = Some(OrderPlacement::Index {
                    index: integer(value)?,
                })
            }
            "--before" | "--before-surface" if placement.is_none() => {
                placement = Some(OrderPlacement::Before {
                    surface: target(value)?,
                })
            }
            "--after" | "--after-surface" if placement.is_none() => {
                placement = Some(OrderPlacement::After {
                    surface: target(value)?,
                })
            }
            "--expect-revision" if revision.is_none() => {
                revision = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| CliError("revision must be a nonnegative integer".into()))?,
                )
            }
            "--focus" if focus.is_none() => {
                focus = Some(match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(CliError("--focus requires true or false".into())),
                })
            }
            _ => {
                return Err(CliError(format!(
                    "Unknown, repeated or conflicting reorder-surface option {flag}"
                )));
            }
        }
        cursor += 2;
    }
    if placement.is_some() {
        // Canonical grammar always names the subject once.
        match operands.as_slice() {
            [surface] if scope.surface.is_none() => scope.surface = Some(target(surface)?),
            [] if scope.surface.is_some() => {}
            _ => {
                return Err(CliError(
                    "reorder-surface requires one subject and exactly one placement".into(),
                ));
            }
        }
    } else {
        // Retain the original local [SURFACE] INDEX migration form, including
        // INDEX-only ambient selection. All public indices are signed/clamped.
        let index =
            match operands.as_slice() {
                [index] => integer(index)?,
                [surface, index] if scope.surface.is_none() => {
                    scope.surface = Some(target(surface)?);
                    integer(index)?
                }
                _ => return Err(CliError(
                    "reorder-surface requires --index, --before, --after or legacy [SURFACE] INDEX"
                        .into(),
                )),
            };
        placement = Some(OrderPlacement::Index { index });
    }
    let caller = targets::caller(&scope)?;
    Ok(Parsed::SurfaceOrder(ControlCommand::SurfaceOrder {
        request: OrderRequest {
            scope,
            caller,
            identity_namespace: None,
            expected_model_revision: revision,
            focus: focus.unwrap_or(false),
            placement: placement.unwrap(),
        },
    }))
}
fn target(value: &str) -> Result<TargetRef, CliError> {
    value
        .parse()
        .map_err(|error: zmux_workspace_core::targets::TargetError| CliError(error.to_string()))
}
fn integer(value: &str) -> Result<i64, CliError> {
    value
        .parse()
        .map_err(|_| CliError("surface index requires a signed integer".into()))
}
pub(super) fn wants_focus(command: &ControlCommand) -> bool {
    matches!(command, ControlCommand::SurfaceOrder { request } if request.focus)
}
pub(super) fn run(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    let budget = windows::remaining(options, deadline)?;
    let mut result = response_result(request(&budget, command)?)?;
    if result["state"] != "order_committed" {
        return Err(CliError("Surface order response omitted its commitment; inspect current order before another mutation".into()));
    }
    if result["focus_requested"] == true && result["native_focus"].is_null() {
        return Err(CliError("Surface order already committed, but native focus receipt is missing; mutation was not replayed".into()));
    }
    if let Some(receipt) = result.get("native_focus").filter(|v| !v.is_null()).cloned() {
        if receipt["window_id"] != result["window_id"]
            || receipt["window_uuid"] != result["window_uuid"]
        {
            return Err(CliError("Surface order already committed, but native focus receipt belongs to another window; mutation was not replayed".into()));
        }
        result["native_focus"] = windows::wait_receipt(options, receipt, "focus", deadline).map_err(|error| CliError(format!("Surface order already committed; native focus failed or is unknown: {error}. Order was not replayed or rolled back")))?;
    }
    Ok(result)
}
