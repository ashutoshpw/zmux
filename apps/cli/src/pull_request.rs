//! Manual repository-verified PR handoff. External work never runs on Desktop.
use super::*;
use bounded_command::{Cancellation, capture};
use serde_json::Value;
use std::{path::Path, process::Command};
use zmux_desktop_runtime::{
    metadata::{MetadataAction, PullRequest, positive_pr_number, valid_repository},
    targets::{TargetOperation, TargetRequest},
};
use zmux_workspace_core::targets::{TargetRef, TargetScope};

pub(super) struct Invocation {
    target: TargetRequest,
    input: Option<String>,
}
impl Invocation {
    /// Offline command discovery describes the preflight; it never runs tools.
    pub(crate) fn discovery(&self) -> Value {
        serde_json::json!({"method":"pull_request", "target":self.target, "input":self.input})
    }
}
pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("pr".into()))));
    }
    let args = utf8_args(tail)?;
    let mut scope = TargetScope::default();
    let mut revision = None;
    let mut value = None;
    let mut cursor = 0;
    while cursor < args.len() {
        let item = &args[cursor];
        match item.as_str() {
            "--workspace" | "--window" | "--expect-revision" => {
                let next = args
                    .get(cursor + 1)
                    .ok_or_else(|| CliError(format!("{item} requires a value")))?;
                match item.as_str() {
                    "--workspace" if scope.workspace.is_none() => {
                        scope.workspace = Some(next.parse().map_err(|e| CliError(format!("{e}")))?)
                    }
                    "--window" if scope.window.is_none() => {
                        scope.window = Some(next.parse().map_err(|e| CliError(format!("{e}")))?)
                    }
                    "--expect-revision" if revision.is_none() => {
                        revision = Some(next.parse::<u64>().map_err(|_| {
                            CliError("model revision must be a nonnegative integer".into())
                        })?)
                    }
                    _ => return Err(CliError(format!("repeated {item}"))),
                }
                cursor += 2;
            }
            _ if !item.starts_with('-') && value.is_none() => {
                value = Some(item.clone());
                cursor += 1;
            }
            _ => {
                return Err(CliError(
                    "pr accepts one URL, positive number or clear; read zmux help pr".into(),
                ));
            }
        }
    }
    let value =
        value.ok_or_else(|| CliError("pr requires a URL, positive number or clear".into()))?;
    if value != "clear" && positive_pr_number(&value).is_none() {
        PullRequest::parse(&value).map_err(|e| CliError(e.to_string()))?;
    }
    let caller = targets::caller(&scope)?;
    if scope.workspace.is_none()
        && caller.workspace.is_none()
        && caller.surface.is_none()
        && caller.local_tmux.is_none()
    {
        return Err(CliError("pr requires --workspace or a live caller workspace/surface; foreground focus is not a PR target".into()));
    }
    Ok(Parsed::PullRequest(Invocation {
        target: TargetRequest {
            scope,
            caller,
            expected_model_revision: revision,
            operation: TargetOperation::Metadata {
                action: MetadataAction::ListStatus,
            },
        },
        input: (value != "clear").then_some(value),
    }))
}
fn ask(
    options: &ClientOptions,
    deadline: Instant,
    cancellation: &Cancellation,
    command: ControlCommand,
) -> Result<Value, CliError> {
    cancellation.check(deadline)?;
    let budget = ClientOptions {
        socket: options.socket.clone(),
        password: options.password.clone(),
        timeout: deadline.saturating_duration_since(Instant::now()),
    };
    response_result(request(&budget, command)?)
}
fn tool_output(
    program: &str,
    directory: &Path,
    args: &[&str],
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<bounded_command::Output, CliError> {
    let mut command = Command::new(program);
    command.env_clear();
    for key in [
        "HOME",
        "PATH",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "TMP",
        "TEMP",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "SSH_AUTH_SOCK",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("LC_ALL", "C")
        .current_dir(directory)
        .args(args);
    capture(&mut command, deadline, cancellation)
}
fn lookup_failed() -> CliError {
    CliError(
        "Git/GitHub lookup failed; check the repository and gh authentication (metadata unchanged)"
            .into(),
    )
}
fn tool(
    program: &str,
    directory: &Path,
    args: &[&str],
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<String, CliError> {
    let output = tool_output(program, directory, args, deadline, cancellation)?;
    if output.status != zmux_session_runtime::child::ExitObservation::Exited(0) {
        return Err(lookup_failed());
    }
    Ok(output.stdout)
}
fn git_root(
    directory: &Path,
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<PathBuf, CliError> {
    git_root_probe(directory, deadline, cancellation)?
        .ok_or_else(|| CliError("CLI directory is not a Git worktree".into()))
}
fn git_root_probe(
    directory: &Path,
    deadline: Instant,
    cancellation: &Cancellation,
) -> Result<Option<PathBuf>, CliError> {
    let output = tool_output(
        "git",
        directory,
        &["rev-parse", "--show-toplevel"],
        deadline,
        cancellation,
    )?;
    cancellation.check(deadline)?;
    if output.status == zmux_session_runtime::child::ExitObservation::Exited(128)
        && output.stdout.is_empty()
        && output.stderr.trim()
            == "fatal: not a git repository (or any of the parent directories): .git"
    {
        return Ok(None);
    }
    if output.status != zmux_session_runtime::child::ExitObservation::Exited(0) {
        return Err(lookup_failed());
    }
    let path = PathBuf::from(output.stdout.trim_end_matches('\n'));
    if !path.is_absolute() || path.as_os_str().is_empty() {
        return Err(CliError(
            "git did not return an absolute worktree root".into(),
        ));
    }
    Ok(Some(path.canonicalize()?))
}
fn local_directories(value: &Value, result: &mut Vec<PathBuf>) {
    if let Some(pane) = value.get("pane") {
        if let Some(surfaces) = pane["surfaces"].as_array() {
            for surface in surfaces {
                if surface["remote"].is_null()
                    && surface["kind"] == "terminal"
                    && let Some(cwd) = surface["cwd"].as_str()
                    && Path::new(cwd).is_absolute()
                {
                    result.push(PathBuf::from(cwd));
                }
            }
        }
    } else {
        // The shared split node has first/second children, independent of focus.
        local_directories_child(value, "first", result);
        local_directories_child(value, "second", result);
    }
}
fn local_directories_child(value: &Value, key: &str, result: &mut Vec<PathBuf>) {
    if let Some(child) = value.get(key) {
        local_directories(child, result);
    }
}
pub(super) fn run(options: &ClientOptions, mut invocation: Invocation) -> Result<Value, CliError> {
    let deadline = Instant::now() + options.timeout;
    let cancellation = Cancellation::new()?;
    let row = ask(
        options,
        deadline,
        &cancellation,
        ControlCommand::Targeted {
            request: invocation.target.clone(),
        },
    )?;
    let workspace = row["workspace_uuid"]
        .as_str()
        .ok_or_else(|| CliError("app does not expose stable PR workspace identity".into()))?;
    let catalog = ask(
        options,
        deadline,
        &cancellation,
        ControlCommand::ListWorkspaces,
    )?;
    let identities = catalog["targets"]
        .as_array()
        .ok_or_else(|| CliError("invalid workspace catalog".into()))?;
    let identity = identities
        .iter()
        .find(|v| v["kind"] == "workspace" && v["uuid"] == workspace)
        .ok_or_else(|| CliError("PR workspace was replaced during lookup".into()))?;
    let window_catalog = ask(
        options,
        deadline,
        &cancellation,
        ControlCommand::Windows {
            request: zmux_desktop_runtime::windows::WindowRequest {
                scope: TargetScope {
                    workspace: Some(workspace.parse().map_err(|e| CliError(format!("{e}")))?),
                    ..Default::default()
                },
                caller: Default::default(),
                expected_model_revision: invocation.target.expected_model_revision,
                action: zmux_desktop_runtime::windows::WindowAction::Current,
            },
        },
    )?;
    if window_catalog["window_id"] != identity["window_id"] {
        return Err(CliError("PR workspace changed windows".into()));
    }
    let window = window_catalog["window_uuid"]
        .as_str()
        .ok_or_else(|| CliError("PR window identity unavailable".into()))?;
    invocation.target.scope = TargetScope {
        workspace: Some(
            workspace
                .parse::<TargetRef>()
                .map_err(|e| CliError(e.to_string()))?,
        ),
        window: Some(window.parse().map_err(|e| CliError(format!("{e}")))?),
        ..Default::default()
    };
    invocation.target.caller = Default::default();
    let action = if let Some(input) = invocation.input {
        let current = env::current_dir()?;
        let root = git_root(&current, deadline, &cancellation)?;
        let owner = catalog["workspaces"]
            .as_array()
            .and_then(|items| items.iter().find(|v| v["id"] == row["workspace_id"]))
            .ok_or_else(|| CliError("PR workspace no longer exists".into()))?;
        let mut directories = Vec::new();
        local_directories(&owner["root"], &mut directories);
        directories.sort();
        directories.dedup();
        let mut matched = false;
        for directory in directories.into_iter().take(8) {
            if git_root_probe(&directory, deadline, &cancellation)?
                .is_some_and(|candidate| candidate == root)
            {
                matched = true;
                break;
            }
            cancellation.check(deadline)?;
        }
        cancellation.check(deadline)?;
        if !matched {
            return Err(CliError(
                "CLI Git worktree does not match a local terminal in the target workspace".into(),
            ));
        }
        let remote = tool(
            "git",
            &root,
            &["remote", "get-url", "origin"],
            deadline,
            &cancellation,
        )?;
        let repository = remote.trim().strip_prefix("https://github.com/").or_else(|| remote.trim().strip_prefix("git@github.com:")).and_then(|path| { let path = path.strip_suffix(".git").unwrap_or(path); valid_repository(path).then_some(path) }).ok_or_else(|| CliError("origin must identify one GitHub repository without credentials or URL decorations".into()))?;
        let info: Value = serde_json::from_str(&tool(
            "gh",
            &root,
            &["repo", "view", repository, "--json", "nameWithOwner,parent"],
            deadline,
            &cancellation,
        )?)
        .map_err(|_| CliError("gh returned invalid repository JSON".into()))?;
        if !info["nameWithOwner"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(repository))
        {
            return Err(CliError("gh repository does not match Git origin".into()));
        }
        let parent = if info["parent"].is_null() {
            None
        } else {
            let owner = info["parent"]["owner"]["login"]
                .as_str()
                .ok_or_else(|| CliError("gh returned invalid fork-parent identity".into()))?;
            let name = info["parent"]["name"]
                .as_str()
                .ok_or_else(|| CliError("gh returned invalid fork-parent identity".into()))?;
            let repository = format!("{owner}/{name}");
            if !valid_repository(&repository) {
                return Err(CliError(
                    "gh returned invalid fork-parent repository".into(),
                ));
            }
            Some(repository)
        };
        let number = positive_pr_number(&input);
        let requested = number
            .is_none()
            .then(|| PullRequest::parse(&input))
            .transpose()
            .map_err(|e| CliError(e.to_string()))?;
        let selected = requested
            .as_ref()
            .map(|pr| pr.repository())
            .unwrap_or(parent.as_deref().unwrap_or(repository));
        if !selected.eq_ignore_ascii_case(repository)
            && !parent
                .as_ref()
                .is_some_and(|name| selected.eq_ignore_ascii_case(name))
        {
            return Err(CliError(
                "PR repository does not match origin or its verified fork parent".into(),
            ));
        }
        let number = number
            .unwrap_or_else(|| requested.as_ref().expect("validated URL").number)
            .to_string();
        let response: Value = serde_json::from_str(&tool(
            "gh",
            &root,
            &[
                "pr",
                "view",
                &number,
                "--repo",
                selected,
                "--json",
                "url,number",
            ],
            deadline,
            &cancellation,
        )?)
        .map_err(|_| CliError("gh returned invalid PR JSON".into()))?;
        let pr = PullRequest::parse(response["url"].as_str().unwrap_or(""))
            .map_err(|e| CliError(e.to_string()))?;
        if !pr.repository().eq_ignore_ascii_case(selected)
            || pr.number.to_string() != number
            || response["number"].as_u64() != Some(pr.number)
        {
            return Err(CliError(
                "gh PR response does not match the requested repository and number".into(),
            ));
        }
        MetadataAction::SetPullRequest { pull_request: pr }
    } else {
        MetadataAction::ClearPullRequest {}
    };
    invocation.target.operation = TargetOperation::Metadata { action };
    ask(
        options,
        deadline,
        &cancellation,
        ControlCommand::Targeted {
            request: invocation.target,
        },
    )
}
