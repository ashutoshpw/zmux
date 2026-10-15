//! Workspace ordering has its own signed placement grammar and hard window scope.
use super::*;
use zmux_desktop_runtime::workspace_order::{OrderCommand, OrderPlacement, OrderRequest};
use zmux_workspace_core::targets::{CallerContext, TargetRef, TargetScope};

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let args = utf8_args(tail)?;
    let (mut workspace, mut window, mut placement, mut order, mut revision) =
        (None, None, None, None, None);
    let mut dry_run = false;
    let mut cursor = 0;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--dry-run" && !dry_run {
            dry_run = true;
            cursor += 1;
            continue;
        }
        if !flag.starts_with('-') && command == "reorder-workspace" && workspace.is_none() {
            workspace = Some(target(flag)?);
            cursor += 1;
            continue;
        }
        let value = args
            .get(cursor + 1)
            .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
        match flag {
            "--window" if window.is_none() => window = Some(target(value)?),
            "--workspace" if command == "reorder-workspace" && workspace.is_none() => {
                workspace = Some(target(value)?)
            }
            "--index" if command == "reorder-workspace" && placement.is_none() => {
                placement = Some(OrderPlacement::Index {
                    index: value
                        .parse()
                        .map_err(|_| CliError("--index requires a signed integer".into()))?,
                })
            }
            "--before" | "--before-workspace"
                if command == "reorder-workspace" && placement.is_none() =>
            {
                placement = Some(OrderPlacement::Before {
                    workspace: target(value)?,
                })
            }
            "--after" | "--after-workspace"
                if command == "reorder-workspace" && placement.is_none() =>
            {
                placement = Some(OrderPlacement::After {
                    workspace: target(value)?,
                })
            }
            "--order" if command == "reorder-workspaces" && order.is_none() => {
                let values = value
                    .split(',')
                    .map(str::trim)
                    .map(target)
                    .collect::<Result<Vec<_>, _>>()?;
                order = Some(values);
            }
            "--expect-revision" if revision.is_none() => {
                revision = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| CliError("revision must be a nonnegative integer".into()))?,
                )
            }
            _ => {
                return Err(CliError(format!(
                    "Unknown, repeated or conflicting {command} option {flag}"
                )));
            }
        }
        cursor += 2;
    }
    let order = if command == "reorder-workspace" {
        OrderCommand::Single {
            workspace: workspace
                .ok_or_else(|| CliError("reorder-workspace requires a workspace".into()))?,
            placement: placement.ok_or_else(|| {
                CliError(
                    "reorder-workspace requires exactly one of --index, --before or --after".into(),
                )
            })?,
        }
    } else {
        OrderCommand::Bulk {
            workspaces: order.ok_or_else(|| {
                CliError("reorder-workspaces requires --order HANDLE,HANDLE,...".into())
            })?,
        }
    };
    // Index selectors use caller window context, while explicit stable selectors
    // derive their owner and ignore ambient object claims at execution time.
    let first = match &order {
        OrderCommand::Single { workspace, .. } => workspace,
        OrderCommand::Bulk { workspaces } => &workspaces[0],
    };
    let caller = if window.is_some() || !first.is_index().map_err(|e| CliError(e.to_string()))? {
        CallerContext::default()
    } else {
        targets::caller(&TargetScope::default())?
    };
    Ok(Parsed::App(ControlCommand::WorkspaceOrder {
        request: OrderRequest {
            window,
            caller,
            expected_model_revision: revision,
            dry_run,
            order,
        },
    }))
}
fn target(value: &str) -> Result<TargetRef, CliError> {
    value
        .parse()
        .map_err(|e: zmux_workspace_core::targets::TargetError| CliError(e.to_string()))
}
