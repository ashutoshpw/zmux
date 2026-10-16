//! Explicit nonempty groups; parsing never guesses a selection or creates a shell.
use super::*;
use zmux_desktop_runtime::workspace_groups::{GroupCommand, GroupRequest};
use zmux_workspace_core::targets::{CallerContext, TargetRef, TargetScope};

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) || tail.first().and_then(|v| v.to_str()) == Some("help") {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let args = utf8_args(tail)?;
    let verb = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| CliError("workspace group requires a supported subcommand".into()))?;
    if ![
        "list",
        "create",
        "rename",
        "collapse",
        "expand",
        "add",
        "remove",
        "set-anchor",
        "move",
        "ungroup",
        "delete",
    ]
    .contains(&verb)
    {
        return Err(CliError(format!(
            "unsupported workspace group subcommand {verb}; generated anchors, pin/color/icon/focus/new-workspace forms remain unavailable"
        )));
    }
    let (
        mut group,
        mut workspace,
        mut window,
        mut name,
        mut members,
        mut external,
        mut index,
        mut revision,
    ) = (None, None, None, None, None, None, None, None);
    let mut json = false;
    let mut cursor = 1;
    while cursor < args.len() {
        let flag = args[cursor].as_str();
        if flag == "--json" && !json {
            json = true;
            cursor += 1;
            continue;
        }
        if !flag.starts_with('-')
            && ["rename", "collapse", "expand", "move", "ungroup", "delete"].contains(&verb)
            && group.is_none()
        {
            group = Some(target(flag)?);
            cursor += 1;
            continue;
        }
        if ["--close-workspaces", "--remove-generated-anchor"].contains(&flag) {
            return Err(CliError(format!(
                "{flag} is unavailable in this non-destructive existing-workspace group slice"
            )));
        }
        let value = args
            .get(cursor + 1)
            .ok_or_else(|| CliError(format!("{flag} requires a value")))?;
        match flag {
            "--window" if window.is_none() => window = Some(target(value)?),
            "--group"
                if group.is_none()
                    && [
                        "rename",
                        "collapse",
                        "expand",
                        "add",
                        "set-anchor",
                        "move",
                        "ungroup",
                        "delete",
                    ]
                    .contains(&verb) =>
            {
                group = Some(target(value)?)
            }
            "--workspace"
                if workspace.is_none() && ["add", "remove", "set-anchor"].contains(&verb) =>
            {
                workspace = Some(target(value)?)
            }
            "--name" if name.is_none() && ["create", "rename"].contains(&verb) => {
                name = Some(text(value, "group name")?)
            }
            "--from" if members.is_none() && verb == "create" => {
                let values = value
                    .split(',')
                    .map(str::trim)
                    .map(target)
                    .collect::<Result<Vec<_>, _>>()?;
                if values.is_empty() || values.len() > zmux_workspace_core::MAX_WORKSPACES {
                    return Err(CliError(
                        "--from requires 1–32 explicit existing workspace handles".into(),
                    ));
                }
                members = Some(values);
            }
            "--idempotency-key" | "--external-id" if external.is_none() && verb == "create" => {
                external = Some(text(value, "external identity")?)
            }
            "--index" if index.is_none() && verb == "move" => {
                index = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| CliError("--index requires a signed integer".into()))?,
                )
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
                    "unknown, repeated or conflicting workspace group option {flag}"
                )));
            }
        }
        cursor += 2;
    }
    let group = || {
        group
            .clone()
            .ok_or_else(|| CliError(format!("workspace group {verb} requires a group handle")))
    };
    let workspace = || {
        workspace
            .clone()
            .ok_or_else(|| CliError(format!("workspace group {verb} requires --workspace")))
    };
    let action=match verb {
        "list"=>GroupCommand::List,
        "create"=>GroupCommand::Create{name:name.ok_or_else(||CliError("group create requires --name".into()))?,workspaces:members.ok_or_else(||CliError("group create requires --from with explicit existing workspaces; empty generated anchors are unavailable".into()))?,external_id:external},
        "rename"=>GroupCommand::Rename{group:group()?,name:name.ok_or_else(||CliError("group rename requires --name".into()))?},
        "collapse"|"expand"=>GroupCommand::SetCollapsed{group:group()?,collapsed:verb=="collapse"},
        "add"=>GroupCommand::Add{group:group()?,workspace:workspace()?},
        "remove"=>GroupCommand::Remove{workspace:workspace()?},
        "set-anchor"=>GroupCommand::SetAnchor{group:group()?,workspace:workspace()?},
        "move"=>GroupCommand::Move{group:group()?,index:index.ok_or_else(||CliError("group move requires --index".into()))?},
        "ungroup"|"delete"=>GroupCommand::Ungroup{group:group()?},
        _=>unreachable!(),
    };
    let needs_context = match &action {
        GroupCommand::List | GroupCommand::Create { .. } => true,
        GroupCommand::Remove { workspace } => {
            workspace.is_index().map_err(|e| CliError(e.to_string()))?
        }
        GroupCommand::Rename { group, .. }
        | GroupCommand::SetCollapsed { group, .. }
        | GroupCommand::Add { group, .. }
        | GroupCommand::SetAnchor { group, .. }
        | GroupCommand::Move { group, .. }
        | GroupCommand::Ungroup { group } => {
            group.is_index().map_err(|e| CliError(e.to_string()))?
        }
    };
    let caller = if window.is_none() && needs_context {
        targets::caller(&TargetScope::default())?
    } else {
        CallerContext::default()
    };
    Ok(Parsed::App(ControlCommand::WorkspaceGroup {
        request: GroupRequest {
            window,
            caller,
            expected_model_revision: revision,
            action,
        },
    }))
}
fn target(value: &str) -> Result<TargetRef, CliError> {
    value
        .parse()
        .map_err(|e: zmux_workspace_core::targets::TargetError| CliError(e.to_string()))
}
fn text(value: &str, kind: &str) -> Result<String, CliError> {
    if value.is_empty()
        || value.chars().count() > zmux_workspace_core::MAX_NAME_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(CliError(format!(
            "{kind} requires 1–128 characters without controls"
        )));
    }
    Ok(value.into())
}
