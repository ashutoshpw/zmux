//! Submit one window mutation and observe its exact native acknowledgement.
use super::*;
use zmux_desktop_runtime::windows::{WindowAction, WindowRequest};
use zmux_workspace_core::targets::{CallerContext, TargetScope};

pub(super) const NATIVE_RECEIPT_BUDGET: Duration = Duration::from_secs(35);

pub(super) fn parse(command: &str, tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    let args = utf8_args(tail)?;
    let mut scope = TargetScope::default();
    let action = match (
        command,
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice(),
    ) {
        ("list-windows", []) => WindowAction::List,
        ("current-window", []) => WindowAction::Current,
        ("new-window", []) => WindowAction::New { focus: true },
        ("new-window", ["--no-focus"]) => WindowAction::New { focus: false },
        ("current-window" | "focus-window" | "close-window", ["--window", handle]) => {
            scope.window = Some(handle.parse().map_err(
                |error: zmux_workspace_core::targets::TargetError| CliError(error.to_string()),
            )?);
            match command {
                "focus-window" => WindowAction::Focus,
                "close-window" => WindowAction::Close,
                _ => WindowAction::Current,
            }
        }
        _ => {
            return Err(CliError(format!(
                "Invalid {command} arguments; read zmux help {command}"
            )));
        }
    };
    // Window roots override ambient object context, and remain addressable while
    // empty. Object commands use targets::caller and treat --window as a constraint.
    let caller = if scope.window.is_some()
        || matches!(action, WindowAction::List | WindowAction::New { .. })
    {
        CallerContext::default()
    } else {
        targets::caller(&scope)?
    };
    Ok(Parsed::Window(ControlCommand::Windows {
        request: WindowRequest {
            scope,
            caller,
            expected_model_revision: None,
            action,
        },
    }))
}

pub(super) fn is_mutation(command: &ControlCommand) -> bool {
    matches!(command, ControlCommand::Windows { request } if matches!(request.action, WindowAction::New { .. } | WindowAction::Focus | WindowAction::Close))
}

pub(super) fn run(
    options: &ClientOptions,
    command: ControlCommand,
) -> Result<serde_json::Value, CliError> {
    let mutation = is_mutation(&command);
    let expected_kind = match &command {
        ControlCommand::Windows { request } => match request.action {
            WindowAction::New { .. } => "open",
            WindowAction::Focus => "focus",
            WindowAction::Close => "close",
            _ => "",
        },
        _ => return Err(CliError("Invalid window dispatch".into())),
    };
    let deadline = Instant::now() + options.timeout;
    let budget = remaining(options, deadline)?;
    let result = response_result(request(&budget, command)?)?;
    if !mutation {
        return Ok(result);
    }
    wait_receipt(options, result, expected_kind, deadline)
}

pub(super) fn wait_receipt(
    options: &ClientOptions,
    mut result: serde_json::Value,
    expected_kind: &str,
    deadline: Instant,
) -> Result<serde_json::Value, CliError> {
    let operation_id = result.get("operation_id").and_then(|id| id.as_str()).ok_or_else(|| CliError("Window admission omitted its operation UUID; outcome is unknown and mutation was not replayed".into()))?.parse().map_err(|_| CliError("Invalid window operation UUID; mutation was not replayed".into()))?;
    let epoch = result
        .get("epoch")
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            CliError("Window admission omitted its runtime epoch; mutation was not replayed".into())
        })?
        .parse()
        .map_err(|_| CliError("Invalid window runtime epoch; mutation was not replayed".into()))?;
    let window_id = result
        .get("window_id")
        .and_then(|id| id.as_u64())
        .filter(|id| *id != 0)
        .ok_or_else(|| {
            CliError(
                "Window admission omitted its window identity; mutation was not replayed".into(),
            )
        })?;
    let window_uuid = result
        .get("window_uuid")
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            CliError(
                "Window admission omitted its stable identity; mutation was not replayed".into(),
            )
        })?
        .to_owned();
    let unknown = |message: String| {
        CliError(format!(
            "Window operation {operation_id} (epoch {epoch}, window {window_uuid}): {message}; native outcome may be unknown and mutation was not replayed"
        ))
    };
    loop {
        if result.get("operation_id") != Some(&serde_json::json!(operation_id))
            || result.get("epoch") != Some(&serde_json::json!(epoch))
            || result.get("window_id").and_then(|id| id.as_u64()) != Some(window_id)
            || result.get("window_uuid").and_then(|id| id.as_str()) != Some(window_uuid.as_str())
        {
            return Err(unknown(
                "receipt belongs to another operation or window".into(),
            ));
        }
        let kind = result.get("kind").and_then(|kind| kind.as_str());
        if kind != Some(expected_kind) && !(expected_kind == "close" && kind == Some("terminate")) {
            return Err(unknown(
                "receipt has an unexpected native operation kind".into(),
            ));
        }
        match result.get("state").and_then(|state| state.as_str()) {
            Some("completed") => return Ok(result),
            Some("failed") => {
                return Err(CliError(format!(
                    "Window operation {operation_id} failed: {}",
                    result
                        .get("error")
                        .and_then(|message| message.as_str())
                        .unwrap_or("native acknowledgement failed")
                )));
            }
            Some("pending") => {}
            _ => return Err(unknown("receipt has an invalid state".into())),
        }
        let sleep = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(25));
        if sleep.is_zero() {
            return Err(unknown("client deadline elapsed".into()));
        }
        thread::sleep(sleep);
        let budget = remaining(options, deadline).map_err(|error| unknown(error.to_string()))?;
        result = response_result(
            request(
                &budget,
                ControlCommand::WindowStatus {
                    epoch,
                    operation_id,
                },
            )
            .map_err(|error| unknown(error.to_string()))?,
        )
        .map_err(|error| unknown(error.to_string()))?;
    }
}
pub(super) fn remaining(
    options: &ClientOptions,
    deadline: Instant,
) -> Result<ClientOptions, CliError> {
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .filter(|time| !time.is_zero())
        .ok_or_else(|| CliError("client deadline elapsed".into()))?;
    Ok(ClientOptions {
        timeout,
        socket: options.socket.clone(),
        password: options.password.clone(),
    })
}
