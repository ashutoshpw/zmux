//! Read-only observer for the already-running Codex app-server daemon.
//!
//! The SessionStart hook supplies the exact opaque thread ID. This module never
//! creates, resumes, steers, or interrupts a thread; it reads the existing
//! thread and its turns, then consumes daemon lifecycle notifications.

use super::*;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::ErrorKind,
    os::{
        fd::AsFd,
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
};
use tungstenite::{Message, WebSocket, client::client_with_config, protocol::WebSocketConfig};
use zmux_remote_runtime::agent_lifecycle::{
    ProviderKind, ProviderLifecycleObservation, ProviderMarker, ProviderState, ProviderTransition,
};

const MAX_RPC_FRAME: usize = 256 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 128;
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);
const RPC_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SOCKET_PATH_BYTES: usize = 100;

pub(super) fn launch_args(executable: &Path, args: &[OsString]) -> Result<Vec<OsString>, CliError> {
    if invocation_overrides_hooks(args)? {
        return Err(CliError(
            "Codex hook settings were supplied on this invocation; tracking was skipped to preserve them".into(),
        ));
    }
    let cwd = env::current_dir()?;
    let mut server = AppServer::connect(executable)?;
    let config = server.rpc("config/read", json!({"cwd": cwd, "includeLayers": false}))?;
    let mut hooks = match &config["config"]["hooks"]["SessionStart"] {
        Value::Null => Vec::new(),
        Value::Array(hooks) => hooks.clone(),
        _ => {
            return Err(CliError(
                "Codex SessionStart hook settings are not an array; tracking was skipped".into(),
            ));
        }
    };
    if hooks.len() >= MAX_PROVIDER_HOOK_ENTRIES {
        return Err(CliError(
            "Codex SessionStart hook list exceeds the adapter bound".into(),
        ));
    }
    hooks.push(json!({
        "hooks": [{
            "type": "command",
            "command": "\"$ZMUX_CLI_PATH\" hooks provider codex-session-start"
        }]
    }));
    let value = toml_value(&Value::Array(hooks), 0)?;
    if value.len() > MAX_PROVIDER_HOOK_CONFIG_BYTES {
        return Err(CliError(
            "Codex SessionStart hook settings exceed the adapter byte bound".into(),
        ));
    }
    let hook = format!("hooks.SessionStart={value}");
    let mut result = vec![OsString::from("--config"), OsString::from(hook)];
    result.extend_from_slice(args);
    Ok(result)
}

const MAX_PROVIDER_HOOK_ENTRIES: usize = 128;
const MAX_PROVIDER_HOOK_CONFIG_BYTES: usize = 32_768;

fn invocation_overrides_hooks(args: &[OsString]) -> Result<bool, CliError> {
    let mut index = 0;
    while index < args.len() {
        let argument = args[index]
            .to_str()
            .ok_or_else(|| CliError("Codex arguments are not valid UTF-8".into()))?;
        let value = if matches!(argument, "-c" | "--config") {
            index += 1;
            Some(
                args.get(index)
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        CliError("Codex --config argument is missing or not valid UTF-8".into())
                    })?,
            )
        } else {
            argument
                .strip_prefix("--config=")
                .or_else(|| argument.strip_prefix("-c="))
        };
        if let Some(value) = value {
            let key = value
                .split_once('=')
                .map(|(key, _)| key.trim().trim_matches(['"', '\'']))
                .unwrap_or_default();
            if key == "hooks" || key.starts_with("hooks.") {
                return Ok(true);
            }
        }
        index += 1;
    }
    Ok(false)
}

fn toml_value(value: &Value, depth: usize) -> Result<String, CliError> {
    if depth > 16 {
        return Err(CliError(
            "Codex hook configuration nesting exceeds its bound".into(),
        ));
    }
    match value {
        Value::Null => Err(CliError(
            "Codex hook configuration contains a value TOML cannot represent".into(),
        )),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => serde_json::to_string(value)
            .map_err(|_| CliError("Codex hook string could not be serialized".into())),
        Value::Array(items) if items.len() <= 256 => {
            let values = items
                .iter()
                .map(|item| toml_value(item, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", values.join(",")))
        }
        Value::Array(_) => Err(CliError("Codex hook array exceeds its bound".into())),
        Value::Object(values) if values.len() <= 64 => {
            let mut entries = Vec::with_capacity(values.len());
            for (key, value) in values {
                if value.is_null() {
                    continue;
                }
                let key = serde_json::to_string(key)
                    .map_err(|_| CliError("Codex hook key could not be serialized".into()))?;
                entries.push(format!("{key}={}", toml_value(value, depth + 1)?));
            }
            Ok(format!("{{{}}}", entries.join(",")))
        }
        Value::Object(_) => Err(CliError("Codex hook table exceeds its bound".into())),
    }
}

pub(super) fn interactive(args: &[OsString]) -> bool {
    let options = args
        .iter()
        .take_while(|argument| argument.as_os_str() != "--");
    if options
        .filter_map(|argument| argument.to_str())
        .any(|argument| matches!(argument, "--help" | "-h" | "--version" | "-V"))
    {
        return false;
    }
    if args
        .iter()
        .filter_map(|argument| argument.to_str())
        .any(|argument| {
            argument == "--no-daemon"
                || matches!(
                    argument.split('=').next(),
                    Some("--cd" | "-C" | "--profile" | "-p")
                )
                || argument == "--dangerously-bypass-hook-trust"
                || argument == "--remote"
                || argument.starts_with("--remote=")
                || argument == "--remote-auth-token-env"
                || argument.starts_with("--remote-auth-token-env=")
        })
    {
        return false;
    }
    match first_positional(args) {
        Ok(None) => true,
        Err(()) => false,
        Ok(Some(command)) => !matches!(
            command,
            "exec"
                | "review"
                | "login"
                | "logout"
                | "mcp"
                | "mcp-server"
                | "plugin"
                | "completion"
                | "features"
                | "app-server"
                | "remote-control"
                | "debug"
                | "sandbox"
                | "agents"
                | "queue"
                | "archive"
                | "delete"
                | "unarchive"
                | "apply"
                | "migrate-rollouts"
                | "migrate-nfs-db"
                | "update"
                | "doctor"
                | "cloud"
                | "exec-server"
                | "help"
        ),
    }
}

fn first_positional(args: &[OsString]) -> Result<Option<&str>, ()> {
    const VALUE_OPTIONS: &[&str] = &[
        "-c",
        "--config",
        "--enable",
        "--disable",
        "-i",
        "--image",
        "-m",
        "--model",
        "-p",
        "--profile",
        "-s",
        "--sandbox",
        "-C",
        "--cd",
        "--add-dir",
        "-a",
        "--ask-for-approval",
    ];
    const BOOLEAN_OPTIONS: &[&str] = &[
        "--strict-config",
        "--oss",
        "--approve-for-me",
        "--dangerously-bypass-approvals-and-sandbox",
        "--worktree",
        "--search",
        "--no-alt-screen",
        "-h",
        "--help",
        "-V",
        "--version",
    ];

    let mut index = 0;
    while index < args.len() {
        let argument = args[index].to_str().ok_or(())?;
        if argument == "--" {
            return Ok(None);
        }
        if !argument.starts_with('-') {
            return Ok(Some(argument));
        }
        let option = argument.split_once('=').map_or(argument, |(name, _)| name);
        if VALUE_OPTIONS.contains(&option) {
            if !argument.contains('=') {
                index += 1;
                if index >= args.len() || args[index].to_str().is_none() {
                    return Err(());
                }
            }
        } else if !BOOLEAN_OPTIONS.contains(&option) {
            // An option added by a newer Codex release may consume a value.
            // Leave that invocation untouched until its CLI shape is reviewed.
            return Err(());
        }
        index += 1;
    }
    Ok(None)
}

#[derive(Debug)]
struct ThreadSnapshot {
    state: ProviderState,
    turn_id: Option<String>,
}

pub(super) struct Watcher {
    thread_id: String,
    runtime: Uuid,
    generation: Uuid,
    sequence: u64,
    active_turn: Option<String>,
    last_state: Option<ProviderState>,
    last_snapshot: Instant,
    server: AppServer,
}

impl Watcher {
    pub(super) fn attach(
        executable: &Path,
        options: &ClientOptions,
        route: &Route,
        runtime: Uuid,
        generation: Uuid,
        thread_id: String,
    ) -> Result<Self, CliError> {
        validate_id(&thread_id)?;
        let mut server = AppServer::connect(executable)?;
        let snapshot = server.snapshot(&thread_id)?;
        let mut watcher = Self {
            thread_id,
            runtime,
            generation,
            sequence: 0,
            active_turn: snapshot
                .turn_id
                .as_ref()
                .filter(|_| {
                    matches!(
                        snapshot.state,
                        ProviderState::Working | ProviderState::Waiting
                    )
                })
                .cloned(),
            last_state: Some(snapshot.state),
            last_snapshot: Instant::now(),
            server,
        };
        watcher.publish(
            options,
            route,
            snapshot.state,
            snapshot.turn_id,
            ProviderTransition::Snapshot,
            ProviderMarker::Baseline,
        )?;
        Ok(watcher)
    }

    pub(super) fn poll_if_due(
        &mut self,
        options: &ClientOptions,
        route: &Route,
    ) -> Result<(), CliError> {
        while let Some(notification) = self.server.try_notification()? {
            self.notification(options, route, notification)?;
        }
        if self.last_snapshot.elapsed() >= SNAPSHOT_INTERVAL {
            // This is a current source refresh only. It never manufactures a
            // state transition or a completion outcome.
            let snapshot = self.server.snapshot(&self.thread_id)?;
            self.active_turn = if matches!(
                snapshot.state,
                ProviderState::Working | ProviderState::Waiting
            ) {
                snapshot.turn_id.clone()
            } else {
                None
            };
            self.last_state = Some(snapshot.state);
            self.publish(
                options,
                route,
                snapshot.state,
                snapshot.turn_id,
                ProviderTransition::Snapshot,
                ProviderMarker::Live,
            )?;
            self.last_snapshot = Instant::now();
        }
        Ok(())
    }

    pub(super) fn evidence_lost(
        &mut self,
        options: &ClientOptions,
        route: &Route,
    ) -> Result<(), CliError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(sequence_exhausted)?;
        super::submit_session_end(
            options,
            route,
            super::ProviderSessionEnd {
                provider: ProviderKind::Codex,
                session_id: self.thread_id.clone(),
                runtime: self.runtime,
                generation: self.generation,
                sequence: self.sequence,
                lost: true,
            },
        )
    }

    pub(super) fn session_ended(
        &mut self,
        options: &ClientOptions,
        route: &Route,
    ) -> Result<(), CliError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(sequence_exhausted)?;
        super::submit_session_end(
            options,
            route,
            super::ProviderSessionEnd {
                provider: ProviderKind::Codex,
                session_id: self.thread_id.clone(),
                runtime: self.runtime,
                generation: self.generation,
                sequence: self.sequence,
                lost: false,
            },
        )
    }

    fn notification(
        &mut self,
        options: &ClientOptions,
        route: &Route,
        value: Value,
    ) -> Result<(), CliError> {
        let method = value["method"].as_str().unwrap_or_default();
        let params = &value["params"];
        match method {
            "turn/started" if params["threadId"] == self.thread_id => {
                let turn_id = required_id(&params["turn"]["id"])?;
                if self.active_turn.as_deref() == Some(&turn_id)
                    && self.last_state == Some(ProviderState::Working)
                {
                    return Ok(());
                }
                self.active_turn = Some(turn_id.clone());
                self.last_state = Some(ProviderState::Working);
                self.publish(
                    options,
                    route,
                    ProviderState::Working,
                    Some(turn_id),
                    ProviderTransition::StateChanged,
                    ProviderMarker::Live,
                )
            }
            "turn/completed" if params["threadId"] == self.thread_id => {
                let turn_id = required_id(&params["turn"]["id"])?;
                let status = params["turn"]["status"].as_str().unwrap_or_default();
                let transition = match status {
                    "completed" => ProviderTransition::TurnCompleted,
                    "failed" => ProviderTransition::TurnFailed,
                    "interrupted" => ProviderTransition::TurnInterrupted,
                    _ => {
                        return Err(CliError(
                            "Codex returned an unsupported turn outcome".into(),
                        ));
                    }
                };
                self.active_turn = None;
                self.last_state = Some(ProviderState::Idle);
                self.publish(
                    options,
                    route,
                    ProviderState::Idle,
                    Some(turn_id),
                    transition,
                    ProviderMarker::Live,
                )
            }
            "thread/status/changed" if params["threadId"] == self.thread_id => {
                let status = &params["status"];
                let state = match status["type"].as_str() {
                    Some("active") => {
                        let flags = status["activeFlags"].as_array().ok_or_else(|| {
                            CliError("Codex returned an invalid active status".into())
                        })?;
                        if flags.iter().any(|flag| {
                            matches!(
                                flag.as_str(),
                                Some("waitingOnApproval" | "waitingOnUserInput")
                            )
                        }) {
                            ProviderState::Waiting
                        } else {
                            ProviderState::Working
                        }
                    }
                    Some("idle") => ProviderState::Idle,
                    Some("notLoaded" | "systemError") => {
                        return Err(CliError(
                            "Codex thread no longer has live daemon status".into(),
                        ));
                    }
                    _ => return Err(CliError("Codex returned an unknown thread status".into())),
                };
                let turn_id = if matches!(state, ProviderState::Working | ProviderState::Waiting) {
                    self.active_turn.clone().or_else(|| {
                        self.server
                            .snapshot(&self.thread_id)
                            .ok()
                            .and_then(|snapshot| snapshot.turn_id)
                    })
                } else {
                    self.active_turn.take()
                };
                if matches!(state, ProviderState::Working | ProviderState::Waiting)
                    && turn_id.is_none()
                {
                    return Err(CliError(
                        "Codex active thread has no exact live turn ID".into(),
                    ));
                }
                if self.last_state == Some(state) {
                    return Ok(());
                }
                self.active_turn =
                    if matches!(state, ProviderState::Working | ProviderState::Waiting) {
                        turn_id.clone()
                    } else {
                        None
                    };
                self.last_state = Some(state);
                let transition = if state == ProviderState::Idle {
                    ProviderTransition::Snapshot
                } else {
                    ProviderTransition::StateChanged
                };
                self.publish(
                    options,
                    route,
                    state,
                    turn_id,
                    transition,
                    ProviderMarker::Live,
                )
            }
            _ => Ok(()),
        }
    }

    fn publish(
        &mut self,
        options: &ClientOptions,
        route: &Route,
        state: ProviderState,
        turn_id: Option<String>,
        transition: ProviderTransition,
        marker: ProviderMarker,
    ) -> Result<(), CliError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(sequence_exhausted)?;
        super::submit_observation(
            options,
            route,
            self.runtime,
            self.generation,
            self.sequence,
            simple_observation(
                ProviderKind::Codex,
                self.thread_id.clone(),
                turn_id,
                state,
                transition,
                marker,
            ),
        )
    }
}

fn sequence_exhausted() -> CliError {
    CliError("provider lifecycle sequence exhausted".into())
}

fn validate_id(value: &str) -> Result<(), CliError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(CliError(
            "Codex supplied an invalid opaque thread ID".into(),
        ))
    } else {
        Ok(())
    }
}

fn required_id(value: &Value) -> Result<String, CliError> {
    let value = value
        .as_str()
        .ok_or_else(|| CliError("Codex lifecycle event has no exact turn ID".into()))?;
    validate_id(value)?;
    Ok(value.to_owned())
}

fn daemon_socket_path(executable: &Path) -> Result<PathBuf, CliError> {
    let cancellation = bounded_command::Cancellation::new()?;
    let output = bounded_command::capture(
        Command::new(executable).args(["app-server", "daemon", "version"]),
        Instant::now() + RPC_TIMEOUT,
        &cancellation,
    )?;
    if output.status != zmux_session_runtime::child::ExitObservation::Exited(0) {
        return Err(CliError(
            "Codex does not report a running shared app-server daemon".into(),
        ));
    }
    let status: Value = serde_json::from_str(output.stdout.trim())
        .map_err(|_| CliError("Codex app-server daemon status was invalid".into()))?;
    if status["status"] != "running" {
        return Err(CliError(
            "Codex does not report a running shared app-server daemon".into(),
        ));
    }
    let socket_path = status["socketPath"]
        .as_str()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| CliError("Codex daemon did not report its control socket".into()))?;
    if !socket_path.is_absolute()
        || socket_path.as_os_str().as_encoded_bytes().len() > MAX_SOCKET_PATH_BYTES
    {
        return Err(CliError(
            "Codex daemon control socket path is outside its bounds".into(),
        ));
    }
    let metadata = fs::metadata(&socket_path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(CliError(
            "Codex daemon control socket is not private to the current user".into(),
        ));
    }
    Ok(socket_path)
}

struct AppServer {
    socket: WebSocket<UnixStream>,
    pending: VecDeque<Value>,
    next_id: u64,
}

impl AppServer {
    fn connect(executable: &Path) -> Result<Self, CliError> {
        let socket_path = daemon_socket_path(executable)?;
        let transport = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        transport
            .connect_timeout(&socket2::SockAddr::unix(&socket_path)?, RPC_TIMEOUT)
            .map_err(|_| {
                CliError("cannot attach to the existing Codex app-server socket".into())
            })?;
        transport.set_nonblocking(false)?;
        let stream: UnixStream = transport.into();
        stream.set_read_timeout(Some(RPC_TIMEOUT))?;
        stream.set_write_timeout(Some(RPC_TIMEOUT))?;
        let config = WebSocketConfig {
            write_buffer_size: 0,
            max_write_buffer_size: MAX_RPC_FRAME * 2,
            max_message_size: Some(MAX_RPC_FRAME),
            max_frame_size: Some(MAX_RPC_FRAME),
            ..WebSocketConfig::default()
        };
        let (mut socket, _) = client_with_config("ws://localhost/", stream, Some(config))
            .map_err(|_| CliError("Codex app-server WebSocket handshake failed".into()))?;
        socket
            .get_mut()
            .set_nonblocking(true)
            .map_err(|_| CliError("cannot configure Codex app-server socket".into()))?;
        let mut server = Self {
            socket,
            pending: VecDeque::new(),
            next_id: 0,
        };
        let response = server.rpc(
            "initialize",
            json!({
                "clientInfo": {"name":"zmux","title":"Zmux","version":env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": false}
            }),
        )?;
        if !["userAgent", "platformFamily", "platformOs"]
            .iter()
            .all(|key| response.get(*key).is_some())
        {
            return Err(CliError(
                "Codex app-server initialization was incomplete".into(),
            ));
        }
        server.notify("initialized", json!({}))?;
        Ok(server)
    }

    fn snapshot(&mut self, thread_id: &str) -> Result<ThreadSnapshot, CliError> {
        let thread = self.rpc(
            "thread/read",
            json!({"threadId":thread_id,"includeTurns":false}),
        )?;
        if thread["thread"]["id"].as_str() != Some(thread_id) {
            return Err(CliError(
                "Codex app-server returned a different thread".into(),
            ));
        }
        let thread_info = thread["thread"]
            .as_object()
            .ok_or_else(|| CliError("Codex app-server returned an invalid thread".into()))?;
        if thread_info
            .get("parentThreadId")
            .is_some_and(|parent| !parent.is_null())
        {
            return Err(CliError(
                "Codex SessionStart identity belongs to a subagent thread".into(),
            ));
        }
        let status = &thread["thread"]["status"];
        let turn_response = self.rpc(
            "thread/turns/list",
            json!({"threadId":thread_id,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
        )?;
        let turn = turn_response["data"]
            .as_array()
            .and_then(|turns| turns.first());
        let turn_id = turn.and_then(|turn| turn["id"].as_str()).map(str::to_owned);
        if let Some(id) = turn_id.as_deref() {
            validate_id(id)?;
        }
        let state = match status["type"].as_str() {
            Some("idle") => ProviderState::Idle,
            Some("active") => {
                let flags = status["activeFlags"]
                    .as_array()
                    .ok_or_else(|| CliError("Codex returned an invalid active status".into()))?;
                if flags.iter().any(|flag| {
                    matches!(
                        flag.as_str(),
                        Some("waitingOnApproval" | "waitingOnUserInput")
                    )
                }) {
                    ProviderState::Waiting
                } else {
                    ProviderState::Working
                }
            }
            Some("notLoaded" | "systemError") => {
                return Err(CliError("Codex thread has no live daemon status".into()));
            }
            _ => return Err(CliError("Codex returned an unknown thread status".into())),
        };
        let turn_id = if matches!(state, ProviderState::Working | ProviderState::Waiting) {
            let id = turn_id
                .ok_or_else(|| CliError("Codex active thread has no live turn ID".into()))?;
            if turn.is_none_or(|turn| turn["status"] != "inProgress") {
                return Err(CliError(
                    "Codex active thread has no in-progress turn".into(),
                ));
            }
            Some(id)
        } else {
            turn_id
        };
        Ok(ThreadSnapshot { state, turn_id })
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, CliError> {
        self.next_id = self.next_id.checked_add(1).ok_or_else(sequence_exhausted)?;
        let id = self.next_id;
        self.send_json(&json!({"method":method,"id":id,"params":params}))?;
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            let response = self.receive_json(deadline)?;
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                if response.get("error").is_some() {
                    return Err(CliError("Codex app-server read request failed".into()));
                }
                return response
                    .get("result")
                    .cloned()
                    .ok_or_else(|| CliError("Codex app-server response was incomplete".into()));
            }
            if response.get("method").is_some() {
                self.queue_notification(response)?;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), CliError> {
        self.send_json(&json!({"method":method,"params":params}))
    }

    fn try_notification(&mut self) -> Result<Option<Value>, CliError> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(Some(value));
        }
        let deadline = Instant::now() + Duration::from_millis(1);
        match self.receive_json(deadline) {
            Ok(value) if value.get("method").is_some() => Ok(Some(value)),
            Ok(_) => Ok(None),
            Err(error) if error.0 == "Codex app-server response timed out" => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn queue_notification(&mut self, value: Value) -> Result<(), CliError> {
        if self.pending.len() >= MAX_PENDING_NOTIFICATIONS {
            return Err(CliError(
                "Codex app-server notification queue overflowed".into(),
            ));
        }
        self.pending.push_back(value);
        Ok(())
    }

    fn receive_json(&mut self, deadline: Instant) -> Result<Value, CliError> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(message)) => {
                    if message.len() > MAX_RPC_FRAME {
                        return Err(CliError(
                            "Codex app-server message exceeded its byte bound".into(),
                        ));
                    }
                    return serde_json::from_str(&message)
                        .map_err(|_| CliError("Codex app-server returned invalid JSON".into()));
                }
                Ok(Message::Ping(_)) => self.flush_until(deadline)?,
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => {
                    return Err(CliError("Codex app-server connection was closed".into()));
                }
                Ok(_) => {
                    return Err(CliError(
                        "Codex app-server returned a non-text message".into(),
                    ));
                }
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    self.wait_for(PollFlags::POLLIN | PollFlags::POLLHUP, deadline)?;
                }
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::Interrupted => {}
                Err(tungstenite::Error::Capacity(_)) => {
                    return Err(CliError(
                        "Codex app-server message exceeded its byte bound".into(),
                    ));
                }
                Err(tungstenite::Error::Protocol(_)) => {
                    return Err(CliError(
                        "Codex app-server returned an invalid WebSocket frame".into(),
                    ));
                }
                Err(_) => {
                    return Err(CliError(
                        "Codex app-server response could not be read".into(),
                    ));
                }
            }
        }
    }

    fn send_json(&mut self, value: &Value) -> Result<(), CliError> {
        let message = serde_json::to_string(value)
            .map_err(|_| CliError("Codex app-server request was invalid".into()))?;
        if message.len() > MAX_RPC_FRAME {
            return Err(CliError(
                "Codex app-server request exceeded its byte bound".into(),
            ));
        }
        let deadline = Instant::now() + RPC_TIMEOUT;
        match self.socket.write(Message::Text(message)) {
            Ok(()) => self.flush_until(deadline),
            Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                self.flush_until(deadline)
            }
            Err(_) => Err(CliError("Codex app-server request write failed".into())),
        }
    }

    fn flush_until(&mut self, deadline: Instant) -> Result<(), CliError> {
        loop {
            match self.socket.flush() {
                Ok(()) => return Ok(()),
                Err(tungstenite::Error::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                    self.wait_for(PollFlags::POLLOUT, deadline)?;
                }
                Err(_) => return Err(CliError("Codex app-server request flush failed".into())),
            }
        }
    }

    fn wait_for(&self, flags: PollFlags, deadline: Instant) -> Result<(), CliError> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CliError("Codex app-server response timed out".into()));
            }
            let timeout =
                PollTimeout::try_from(remaining.as_millis().clamp(1, i32::MAX as u128) as u32)
                    .unwrap_or(PollTimeout::MAX);
            let mut descriptors = [PollFd::new(self.socket.get_ref().as_fd(), flags)];
            match poll(&mut descriptors, timeout) {
                Ok(0) => return Err(CliError("Codex app-server response timed out".into())),
                Ok(_) => return Ok(()),
                Err(nix::errno::Errno::EINTR) => continue,
                Err(_) => return Err(CliError("Codex app-server socket poll failed".into())),
            }
        }
    }
}

fn simple_observation(
    provider: ProviderKind,
    session_id: String,
    turn_id: Option<String>,
    state: ProviderState,
    transition: ProviderTransition,
    marker: ProviderMarker,
) -> ProviderLifecycleObservation {
    ProviderLifecycleObservation {
        provider,
        scope: zmux_remote_runtime::agent_lifecycle::ProviderScope::Root,
        session_id,
        turn_id,
        state,
        transition,
        marker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    use tungstenite::{Message, accept};

    #[test]
    #[ignore = "requires the installed shared Codex app-server daemon; read-only and starts no thread or turn"]
    fn installed_app_server_accepts_read_only_hook_and_thread_reads() {
        let executable = super::super::find_executable("codex")
            .expect("find installed Codex without invoking a provider turn");
        let mut server = AppServer::connect(&executable)
            .expect("attach to the already-running shared Codex app-server");
        let response = server
            .rpc(
                "config/read",
                json!({
                    "cwd": env::current_dir().expect("current directory"),
                    "includeLayers": false
                }),
            )
            .expect("read the effective config from the existing app-server");
        assert!(response["config"].is_object());

        let loaded = server
            .rpc("thread/loaded/list", json!({}))
            .expect("list existing loaded threads without resuming one");
        let Some(thread_id) = loaded["data"]
            .as_array()
            .and_then(|threads| threads.first())
            .and_then(Value::as_str)
        else {
            eprintln!("Codex daemon has no currently loaded thread for a live read-only snapshot");
            return;
        };
        validate_id(thread_id).expect("existing daemon thread ID is bounded and opaque");
        let snapshot = server
            .snapshot(thread_id)
            .expect("read the exact existing thread and its latest turn without resuming it");
        assert!(matches!(
            snapshot.state,
            ProviderState::Idle | ProviderState::Working | ProviderState::Waiting
        ));
    }

    #[test]
    fn existing_daemon_websocket_is_bounded_and_queues_interleaved_notifications() {
        let directory = env::temp_dir().join(format!("zmux-codex-ws-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .expect("create private fake Codex daemon directory");
        let socket_path = directory.join("app-server.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind fake app-server socket");
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .expect("make fake app-server socket private");
        let server_thread = thread::Builder::new()
            .name("zmux-codex-websocket-fixture".into())
            .spawn(move || {
                let (stream, _) = listener.accept().expect("accept WebSocket client");
                stream
                    .set_read_timeout(Some(RPC_TIMEOUT))
                    .expect("bound fake app-server read wait");
                let mut socket = accept(stream).expect("complete WebSocket handshake");
                while let Ok(Message::Text(line)) = socket.read() {
                    let request: Value = serde_json::from_str(&line).expect("parse RPC request");
                    let method = request["method"].as_str().unwrap_or_default();
                    let Some(id) = request.get("id") else {
                        continue;
                    };
                    if method == "initialize" {
                        socket
                            .send(Message::Text(
                                json!({
                                    "id": id,
                                    "result": {
                                        "userAgent": "synthetic",
                                        "platformFamily": "unix",
                                        "platformOs": "linux"
                                    }
                                })
                                .to_string(),
                            ))
                            .expect("reply to initialize");
                    } else if method == "thread/list" {
                        socket
                            .send(Message::Text(
                                json!({
                                    "method": "thread/status/changed",
                                    "params": {
                                        "threadId": "opaque-thread",
                                        "status": {"type": "idle"}
                                    }
                                })
                                .to_string(),
                            ))
                            .expect("send lifecycle notification");
                        socket
                            .send(Message::Text(
                                json!({
                                    "id": id,
                                    "result": {"data": []}
                                })
                                .to_string(),
                            ))
                            .expect("reply to read-only thread list");
                        break;
                    }
                }
            })
            .expect("start fake app-server");
        let executable = directory.join("codex");
        let status = serde_json::to_string(&json!({
            "status": "running",
            "socketPath": socket_path
        }))
        .expect("serialize daemon status")
        .replace('\'', "'\\''");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nif [ \"$1\" = app-server ] && [ \"$2\" = daemon ] && [ \"$3\" = version ]; then printf '%s\\n' '{status}'; else exit 91; fi\n"
            ),
        )
        .expect("write fake Codex proxy");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("make fake Codex proxy executable");

        let result = (|| {
            let mut server = AppServer::connect(&executable)?;
            let listed = server.rpc("thread/list", json!({"limit":1}))?;
            if listed["data"]
                .as_array()
                .is_none_or(|rows| !rows.is_empty())
            {
                return Err(CliError("fake Codex thread list was malformed".into()));
            }
            let notification = server
                .try_notification()?
                .ok_or_else(|| CliError("Codex proxy notification was not queued".into()))?;
            if notification["method"] != "thread/status/changed"
                || notification["params"]["threadId"] != "opaque-thread"
            {
                return Err(CliError(
                    "Codex proxy notification changed in transit".into(),
                ));
            }
            Ok::<(), CliError>(())
        })();
        server_thread
            .join()
            .expect("fake app-server thread completed");
        let _ = fs::remove_dir_all(directory);
        result.expect("Codex attaches read-only to the existing Unix WebSocket daemon");
    }
}
