//! Thin local inbox client. Message bodies never enter a terminal input path.
use super::*;
use serde_json::{Value, json};
use uuid::Uuid;
use zmux_desktop_runtime::agent_messages::{
    MAX_BODY_BYTES, MAX_PAGE, MessageAction, MessageRequest, MessageState, SettingScope,
};
use zmux_workspace_core::targets::TargetScope;

pub(super) struct Invocation {
    request: MessageRequest,
    stdin: bool,
    mark_read: bool,
}
impl Invocation {
    pub(super) fn discovery(&self) -> Value {
        json!({"method":"agent_messages", "request":self.request,
            "stdin":self.stdin, "mark_read":self.mark_read})
    }
}
fn value(args: &[String], index: &mut usize) -> Result<String, CliError> {
    let flag = &args[*index];
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| CliError(format!("{flag} requires a value")))
}
fn once<T>(slot: &mut Option<T>, item: T, flag: &str) -> Result<(), CliError> {
    if slot.replace(item).is_some() {
        return Err(CliError(format!("repeated {flag}")));
    }
    Ok(())
}
fn uuid(text: &str) -> Result<Uuid, CliError> {
    let id = text
        .parse::<Uuid>()
        .map_err(|_| CliError("message identifier must be a UUID".into()))?;
    if id.is_nil() {
        return Err(CliError("message identifier cannot be nil".into()));
    }
    Ok(id)
}
fn body(text: &str) -> Result<(), CliError> {
    if text.is_empty() || text.len() > MAX_BODY_BYTES {
        return Err(CliError(format!(
            "message body must contain 1–{MAX_BODY_BYTES} UTF-8 bytes"
        )));
    }
    Ok(())
}
pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if tail.is_empty() || tail.first().is_some_and(|a| a == "-h" || a == "--help") {
        return Ok(Parsed::Offline(Offline::Help(Some("agent".into()))));
    }
    let args = utf8_args(tail)?;
    let mut request_id = None;
    let mut stdin = false;
    let mut mark_read = false;
    let action = match args[0].as_str() {
        "message" | "msg" => {
            let (mut reply_to, mut sender_name, mut thread_id) = (None, None, None);
            let mut positional = Vec::new();
            let mut index = 1;
            while index < args.len() {
                let flag = args[index].as_str();
                match flag {
                    "--help" | "-h" => {
                        return Ok(Parsed::Offline(Offline::Help(Some("agent".into()))));
                    }
                    "--" => {
                        positional.extend_from_slice(&args[index + 1..]);
                        break;
                    }
                    "--reply-to" => {
                        let v = uuid(&value(&args, &mut index)?)?;
                        once(&mut reply_to, v, flag)?;
                    }
                    "--from" => {
                        let v = value(&args, &mut index)?;
                        once(&mut sender_name, v, flag)?;
                    }
                    "--thread" => {
                        let v = uuid(&value(&args, &mut index)?)?;
                        once(&mut thread_id, v, flag)?;
                    }
                    "--request-id" => {
                        let v = uuid(&value(&args, &mut index)?)?;
                        once(&mut request_id, v, flag)?;
                    }
                    "--json" => (),
                    _ if flag.starts_with('-') && flag != "-" => {
                        return Err(CliError(format!(
                            "unknown agent message option {flag}; use -- for literal text"
                        )));
                    }
                    _ => positional.push(args[index].clone()),
                }
                index += 1;
            }
            let target = if reply_to.is_none() {
                if positional.len() < 2 {
                    return Err(CliError("agent message requires TARGET and TEXT|-".into()));
                }
                Some(positional.remove(0))
            } else {
                None
            };
            if positional.is_empty() {
                return Err(CliError("agent message requires TEXT|-".into()));
            }
            let text = positional.join(" ");
            stdin = positional.as_slice() == ["-"];
            if !stdin {
                body(&text)?;
            }
            MessageAction::Send {
                target,
                reply_to,
                body: if stdin { String::new() } else { text },
                sender_name,
                thread_id,
            }
        }
        "inbox" => {
            let (mut surface, mut states, mut limit, mut cursor) = (None, None, None, None);
            let mut index = 1;
            while index < args.len() {
                let flag = args[index].as_str();
                match flag {
                    "--help" | "-h" => {
                        return Ok(Parsed::Offline(Offline::Help(Some("agent".into()))));
                    }
                    "--surface" => {
                        let v = value(&args, &mut index)?;
                        once(&mut surface, v, flag)?;
                    }
                    "--state" => {
                        let v = value(&args, &mut index)?;
                        let list = v
                            .split(',')
                            .map(|state| match state {
                                "queued" => Ok(MessageState::Queued),
                                "delivered" => Ok(MessageState::Delivered),
                                "read" => Ok(MessageState::Read),
                                "failed" => Ok(MessageState::Failed),
                                _ => Err(CliError(
                                    "state must be queued, delivered, read or failed".into(),
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        once(&mut states, list, flag)?;
                    }
                    "--limit" => {
                        let n = value(&args, &mut index)?
                            .parse::<usize>()
                            .map_err(|_| CliError("inbox limit must be an integer".into()))?;
                        if !(1..=MAX_PAGE).contains(&n) {
                            return Err(CliError(format!("inbox limit must be 1–{MAX_PAGE}")));
                        }
                        once(&mut limit, n, flag)?;
                    }
                    "--cursor" => {
                        let n = value(&args, &mut index)?.parse::<u64>().map_err(|_| {
                            CliError("inbox cursor must be a nonnegative integer".into())
                        })?;
                        once(&mut cursor, n, flag)?;
                    }
                    "--mark-read" if !mark_read => mark_read = true,
                    "--json" => (),
                    _ => {
                        return Err(CliError(format!(
                            "unknown or repeated agent inbox option {flag}"
                        )));
                    }
                }
                index += 1;
            }
            MessageAction::List {
                surface,
                states: states.unwrap_or_default(),
                limit: limit.unwrap_or(MAX_PAGE),
                cursor,
            }
        }
        "messages" => {
            let mut index = 1;
            let enabled = match args.get(index).map(String::as_str) {
                Some("on") => {
                    index += 1;
                    Some(true)
                }
                Some("off") => {
                    index += 1;
                    Some(false)
                }
                Some("status") => {
                    index += 1;
                    None
                }
                _ => None,
            };
            let (mut target, mut scope) = (None, None);
            while index < args.len() {
                let flag = args[index].as_str();
                match flag {
                    "--help" | "-h" => {
                        return Ok(Parsed::Offline(Offline::Help(Some("agent".into()))));
                    }
                    "--workspace" => once(&mut scope, SettingScope::Workspace, "settings scope")?,
                    "--global" => once(&mut scope, SettingScope::Global, "settings scope")?,
                    "--json" => (),
                    _ if !flag.starts_with('-') => {
                        once(&mut target, args[index].clone(), "target")?
                    }
                    _ => return Err(CliError(format!("unknown agent messages option {flag}"))),
                }
                index += 1;
            }
            let scope = scope.unwrap_or_default();
            if scope == SettingScope::Global && target.is_some() {
                return Err(CliError("global message settings accept no target".into()));
            }
            MessageAction::Settings {
                scope,
                target,
                enabled,
            }
        }
        _ => {
            return Err(CliError(
                "agent requires message, inbox or messages; read zmux help agent".into(),
            ));
        }
    };
    Ok(Parsed::AgentMessages(Invocation {
        request: MessageRequest {
            request_id: request_id.unwrap_or_else(Uuid::new_v4),
            caller: Default::default(),
            action,
        },
        stdin,
        mark_read,
    }))
}

fn completed(
    options: &ClientOptions,
    deadline: Instant,
    message: MessageRequest,
) -> Result<Value, CliError> {
    let id = message.request_id;
    let mut command = ControlCommand::AgentMessages { request: message };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CliError(format!(
                "agent message operation {id} deadline expired; outcome unknown, request was not replayed"
            )));
        }
        let budget = ClientOptions {
            socket: options.socket.clone(),
            timeout: remaining,
            password: options.password.clone(),
        };
        let response = request(&budget, command).map_err(|error| {
            CliError(format!(
                "agent message operation {id}: {error}; outcome unknown, request was not replayed"
            ))
        })?;
        let receipt = response_result(response)?;
        if receipt.get("request_id").and_then(Value::as_str) != Some(id.to_string().as_str()) {
            return Err(CliError(
                "agent message operation receipt identity mismatch".into(),
            ));
        }
        match receipt.get("state").and_then(Value::as_str) {
            Some("completed") => {
                return receipt
                    .get("result")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .ok_or_else(|| {
                        CliError("completed agent message operation has no result".into())
                    });
            }
            Some("pending") => (),
            Some("failed") => {
                return Err(CliError(format!(
                    "agent message operation {id} failed: {}",
                    receipt["error"]
                )));
            }
            Some("unknown_or_expired") => {
                return Err(CliError(format!(
                    "agent message operation {id} is unknown or expired; outcome unknown, request was not replayed"
                )));
            }
            _ => return Err(CliError("invalid agent message operation receipt".into())),
        }
        thread::sleep(
            Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
        );
        command = ControlCommand::AgentMessagesStatus { request_id: id };
    }
}
pub(super) fn run(options: &ClientOptions, mut invocation: Invocation) -> Result<Value, CliError> {
    // Discovery is offline; resolve ambient attribution only for execution.
    // Recipient selectors must not suppress independently resolved sender attribution.
    invocation.request.caller = targets::caller(&TargetScope::default())?;
    if invocation.stdin {
        let mut bytes = Vec::new();
        io::stdin()
            .take(MAX_BODY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| CliError("message stdin must be valid UTF-8".into()))?;
        body(&text)?;
        let MessageAction::Send { body, .. } = &mut invocation.request.action else {
            return Err(CliError("stdin is only valid for agent message".into()));
        };
        *body = text;
    }
    let deadline = Instant::now() + options.timeout;
    let caller = invocation.request.caller.clone();
    let mut result = completed(options, deadline, invocation.request)?;
    if invocation.mark_read {
        let rows = result
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| CliError("agent inbox result has no messages".into()))?;
        let ids = rows
            .iter()
            .map(|row| {
                row.get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CliError("agent inbox row has no message ID".into()))
                    .and_then(uuid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !ids.is_empty() {
            let marked = completed(
                options,
                deadline,
                MessageRequest {
                    request_id: Uuid::new_v4(),
                    caller,
                    action: MessageAction::MarkRead { ids },
                },
            )?;
            result["messages"] = marked
                .get("messages")
                .filter(|v| v.is_array())
                .cloned()
                .ok_or_else(|| CliError("agent mark-read result has no messages".into()))?;
        }
    }
    Ok(result)
}
