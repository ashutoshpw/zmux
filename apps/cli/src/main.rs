use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt,
    io::{self, Read, Write},
    os::fd::AsFd,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::ExitCode,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::attention::{AttentionCommand, NotificationSelector};
use zmux_desktop_runtime::control::{
    CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, MAX_CONTROL_REQUEST_BYTES,
    MAX_CONTROL_RESPONSE_BYTES,
};
use zmux_desktop_runtime::control_policy::SocketPassword;
use zmux_session_runtime::{Launch, Session, SessionError, State};
use zmux_workspace_core::{Action, Axis, FocusHistoryDirection};
mod agent_messages;
mod bounded_command;
mod browser;
mod browser_download;
mod buffers;
mod hooks;
mod inspection;
mod local_multiplexer;
mod local_zellij;
mod pull_request;
mod registry;
mod session;
mod ssh;
mod surface_order;
mod targets;
mod windows;
mod workspace_groups;
mod workspace_move;
mod workspace_order;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_TIMEOUT_MS: u64 = 50;
const MAX_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CliError {}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

struct ClientOptions {
    socket: PathBuf,
    timeout: Duration,
    password: Option<SocketPassword>,
}

enum Parsed {
    Hooks(hooks::Invocation),
    AgentMessages(agent_messages::Invocation),
    Buffers(buffers::Invocation),
    PullRequest(pull_request::Invocation),
    Offline(Offline),
    LocalMultiplexer(local_multiplexer::Invocation),
    LocalZellij(local_zellij::Invocation),
    LocalTmuxApp(ControlCommand),
    App(ControlCommand),
    Browser(ControlCommand, Option<Duration>),
    BrowserProfile(ControlCommand),
    BrowserDownload(ControlCommand, Option<Duration>),
    Window(ControlCommand),
    WorkspaceMove(ControlCommand),
    SurfaceOrder(ControlCommand),
}

enum Offline {
    Help(Option<String>),
    Version,
    Doctor,
    Prototype(Vec<String>),
    Guide,
    Catalog,
    CatalogCheck,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zmux: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, Box<dyn Error>> {
    let (parsed, socket_override, timeout, password) = parse(env::args_os().skip(1).collect())?;
    match parsed {
        Parsed::Hooks(invocation) => hooks::run(invocation, socket_override, timeout, password),
        Parsed::AgentMessages(invocation) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&agent_messages::run(&options, invocation)?)?
            );
            Ok(0)
        }
        Parsed::Buffers(invocation) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&buffers::run(
                    invocation,
                    socket_override,
                    timeout,
                    password
                )?)?
            );
            Ok(0)
        }
        Parsed::PullRequest(invocation) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&pull_request::run(&options, invocation)?)?
            );
            Ok(0)
        }
        Parsed::Offline(Offline::Help(command)) => {
            print_help(command.as_deref())?;
            Ok(0)
        }
        Parsed::Offline(Offline::Guide) => {
            print!("{}", registry::guide());
            Ok(0)
        }
        Parsed::Offline(Offline::Catalog) => {
            println!("{}", serde_json::to_string_pretty(&registry::catalog()?)?);
            Ok(0)
        }
        Parsed::Offline(Offline::CatalogCheck) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&registry::check_catalog()?)?
            );
            Ok(0)
        }
        Parsed::Offline(Offline::Version) => {
            println!("zmux {VERSION}");
            Ok(0)
        }
        Parsed::Offline(Offline::Prototype(args)) => run_prototype(args),
        Parsed::Offline(Offline::Doctor) => {
            let options = client_options(socket_override, timeout, password)?;
            doctor(&options)?;
            Ok(0)
        }
        Parsed::SurfaceOrder(command) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&surface_order::run(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::WorkspaceMove(command) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&workspace_move::run(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::Window(command) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&windows::run(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::App(command) => {
            let options = client_options(socket_override, timeout, password)?;
            let result = response_result(request(&options, command)?)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(0)
        }
        Parsed::LocalMultiplexer(invocation) => local_multiplexer::run(invocation),
        Parsed::LocalZellij(invocation) => local_zellij::run(invocation),
        Parsed::LocalTmuxApp(command) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&local_multiplexer::run_gui(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::BrowserDownload(command, _) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&browser_download::run(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::BrowserProfile(command) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&browser::run_profile(&options, command)?)?
            );
            Ok(0)
        }
        Parsed::Browser(command, wait_timeout) => {
            let options = client_options(socket_override, timeout, password)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&browser::run(&options, command, wait_timeout)?)?
            );
            Ok(0)
        }
    }
}

fn parse(
    args: Vec<OsString>,
) -> Result<(Parsed, Option<PathBuf>, Duration, Option<SocketPassword>), CliError> {
    let mut index = 0;
    let mut socket = None;
    let mut password = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let mut explicit_timeout = false;
    while let Some(argument) = args.get(index).and_then(|value| value.to_str()) {
        match argument {
            "--socket" => {
                let path = args
                    .get(index + 1)
                    .ok_or_else(|| CliError("--socket requires a path".into()))?;
                socket = Some(PathBuf::from(path));
                index += 2;
            }
            "--password" => {
                if password.is_some() {
                    return Err(CliError("repeated --password".into()));
                }
                password = Some(
                    SocketPassword::new(
                        utf8_arg(args.get(index + 1), "socket password")?.to_owned(),
                    )
                    .map_err(|e| CliError(e.to_string()))?,
                );
                index += 2;
            }
            "--timeout" => {
                let milliseconds = utf8_arg(args.get(index + 1), "timeout")?
                    .parse::<u64>()
                    .map_err(|_| CliError("timeout must be an integer in milliseconds".into()))?;
                if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&milliseconds) {
                    return Err(CliError("timeout must be 50–30000 milliseconds".into()));
                }
                timeout = Duration::from_millis(milliseconds);
                explicit_timeout = true;
                index += 2;
            }
            _ => break,
        }
    }
    let command = args
        .get(index)
        .and_then(|value| value.to_str())
        .map(|name| registry::find(name).map_or(name, |spec| spec.name));
    let tail = &args[(index + 1).min(args.len())..];
    if let Some(command @ ("set-buffer" | "list-buffers" | "paste-buffer")) = command {
        return Ok((buffers::parse(command, tail)?, socket, timeout, password));
    }
    if command == Some("workspace") {
        let group_tail = if tail.first().and_then(|v| v.to_str()) == Some("group") {
            &tail[1..]
        } else if help_requested(tail) {
            tail
        } else {
            return Err(CliError(
                "workspace currently supports only the group namespace".into(),
            ));
        };
        return Ok((
            workspace_groups::parse("workspace", group_tail)?,
            socket,
            timeout,
            password,
        ));
    }
    if command == Some("workspace-group") {
        return Ok((
            workspace_groups::parse("workspace-group", tail)?,
            socket,
            timeout,
            password,
        ));
    }
    if command == Some("hooks") {
        return Ok((hooks::parse(tail)?, socket, timeout, password));
    }
    if command == Some("agent") {
        return Ok((agent_messages::parse(tail)?, socket, timeout, password));
    }
    if command == Some("local-zellij") {
        return Ok((local_zellij::parse(tail)?, socket, timeout, password));
    }
    if command == Some("__local-zellij-workload") {
        if tail.len() != 1 {
            return Err(CliError("invalid internal workload descriptor".into()));
        }
        return Ok((
            Parsed::LocalZellij(local_zellij::Invocation::Workload(PathBuf::from(&tail[0]))),
            socket,
            timeout,
            password,
        ));
    }
    if command == Some("__local-zellij-supervisor") {
        if tail.len() != 1 {
            return Err(CliError("invalid internal supervisor descriptor".into()));
        }
        return Ok((
            Parsed::LocalZellij(local_zellij::Invocation::Supervisor(PathBuf::from(
                &tail[0],
            ))),
            socket,
            timeout,
            password,
        ));
    }
    if let Some(command @ ("local-tmux" | "tmux")) = command {
        return Ok((
            local_multiplexer::parse(command, tail)?,
            socket,
            timeout,
            password,
        ));
    }
    if let Some(command @ ("reorder-workspace" | "reorder-workspaces")) = command {
        return Ok((
            workspace_order::parse(command, tail)?,
            socket,
            timeout,
            password,
        ));
    }
    if command == Some("reorder-surface") {
        let parsed = surface_order::parse(tail)?;
        if !explicit_timeout
            && let Parsed::SurfaceOrder(request) = &parsed
            && surface_order::wants_focus(request)
        {
            timeout = windows::NATIVE_RECEIPT_BUDGET;
        }
        return Ok((parsed, socket, timeout, password));
    }
    if command == Some("move-workspace-to-window") {
        let parsed = workspace_move::parse(tail)?;
        if !explicit_timeout {
            timeout = windows::NATIVE_RECEIPT_BUDGET;
        }
        return Ok((parsed, socket, timeout, password));
    }
    if let Some(
        command @ ("list-windows" | "current-window" | "new-window" | "focus-window"
        | "close-window"),
    ) = command
    {
        let parsed = windows::parse(command, tail)?;
        if !explicit_timeout
            && let Parsed::Window(request) = &parsed
            && windows::is_mutation(request)
        {
            timeout = windows::NATIVE_RECEIPT_BUDGET;
        }
        return Ok((parsed, socket, timeout, password));
    }
    if command == Some("browser") {
        let parsed = browser::parse(tail)?;
        if !explicit_timeout && let Parsed::Browser(_, Some(wait)) = &parsed {
            timeout = *wait + Duration::from_secs(1);
        }
        if !explicit_timeout && let Parsed::BrowserDownload(_, Some(wait)) = &parsed {
            timeout = *wait;
        }
        return Ok((parsed, socket, timeout, password));
    }
    if command == Some("pr") {
        return Ok((pull_request::parse(tail)?, socket, timeout, password));
    }
    if let Some(command) = command
        && registry::find(command).is_some_and(|spec| spec.targeted)
    {
        return Ok((targets::parse(command, tail)?, socket, timeout, password));
    }
    let parsed = match command {
        None | Some("-h" | "--help") => Parsed::Offline(Offline::Help(None)),
        Some("-V" | "--version") => Parsed::Offline(Offline::Version),
        Some("help") => {
            if tail.len() > 1 {
                return Err(CliError("help accepts at most one command".into()));
            }
            let command = tail
                .first()
                .map(|value| utf8_arg(Some(value), "command"))
                .transpose()?;
            Parsed::Offline(Offline::Help(match command {
                Some("-h" | "--help") => Some("help".into()),
                Some(command) => Some(command.to_owned()),
                None => None,
            }))
        }
        Some("guide") => help_or_no_args("guide", tail, Offline::Guide)?,
        Some("commands") if tail == [OsString::from("--check")] => {
            Parsed::Offline(Offline::CatalogCheck)
        }
        Some("commands") => help_or_no_args("commands", tail, Offline::Catalog)?,
        Some("version") => help_or_no_args("version", tail, Offline::Version)?,
        Some("capabilities") => app_no_args("capabilities", tail, ControlCommand::Capabilities)?,
        Some("doctor") => help_or_no_args("doctor", tail, Offline::Doctor)?,
        Some("ssh") => ssh::parse(tail)?,
        Some("restore-session" | "save-session" | "session-status") => {
            session::parse(command.unwrap(), tail)?
        }
        Some("focus-history") => help_or_exact("focus-history", tail, 1, |values| {
            let direction = match values[0].as_str() {
                "back" => FocusHistoryDirection::Back,
                "forward" => FocusHistoryDirection::Forward,
                _ => {
                    return Err(CliError(
                        "focus-history requires back or forward and accepts no target options"
                            .into(),
                    ));
                }
            };
            Ok(ControlCommand::Action {
                action: Action::NavigateFocus { direction },
            })
        })?,
        Some("list-uploads") => app_no_args("list-uploads", tail, ControlCommand::ListUploads)?,
        Some("cancel-upload") => {
            let args = utf8_args(tail)?;
            match args.as_slice() {
                _ if help_requested(tail) => {
                    Parsed::Offline(Offline::Help(Some("cancel-upload".into())))
                }
                [id] => Parsed::App(ControlCommand::Transfer {
                    action: zmux_desktop_runtime::transfers::TransferCommand::Cancel {
                        id: id
                            .parse()
                            .map_err(|_| CliError("upload ID must be an integer".into()))?,
                    },
                }),
                _ => return Err(CliError("cancel-upload requires an upload ID".into())),
            }
        }
        Some("status") => app_no_args("status", tail, ControlCommand::Status)?,
        Some("current") => inspection::parse(tail)?,
        Some("list-workspaces") => {
            app_no_args("list-workspaces", tail, ControlCommand::ListWorkspaces)?
        }
        Some("new-workspace") => help_or_exact("new-workspace", tail, 1, |values| {
            Ok(ControlCommand::NewWorkspace {
                name: values[0].clone(),
            })
        })?,
        Some("list-notifications") => app_no_args(
            "list-notifications",
            tail,
            ControlCommand::ListNotifications,
        )?,
        Some("jump-to-unread") => app_no_args(
            "jump-to-unread",
            tail,
            ControlCommand::Attention {
                action: AttentionCommand::JumpToUnread,
            },
        )?,
        Some("open-notification") => help_or_exact("open-notification", tail, 2, |values| {
            if values[0] != "--id" {
                return Err(CliError("open-notification requires --id ID".into()));
            }
            let id = parse_notification_selector(&values[1])?;
            Ok(ControlCommand::Attention {
                action: AttentionCommand::Open { id },
            })
        })?,
        Some("recently-closed") => {
            app_no_args("recently-closed", tail, ControlCommand::RecentlyClosed)?
        }
        Some("reopen-closed") => {
            if help_requested(tail) {
                Parsed::Offline(Offline::Help(Some("reopen-closed".into())))
            } else {
                let values = utf8_args(tail)?;
                let closed_id = match values.as_slice() {
                    [] => None,
                    [id] => Some(parse_id(id, "closed item")?),
                    _ => return Err(CliError("usage: zmux reopen-closed [CLOSED_ID]".into())),
                };
                Parsed::App(ControlCommand::Action {
                    action: Action::ReopenClosed { closed_id },
                })
            }
        }
        Some("prototype") => {
            let values = utf8_args(tail)?;
            if values
                .first()
                .is_some_and(|value| value == "-h" || value == "--help")
            {
                Parsed::Offline(Offline::Help(Some("prototype".into())))
            } else {
                Parsed::Offline(Offline::Prototype(values))
            }
        }
        Some(other) => {
            return Err(CliError(format!(
                "unknown command `{other}`\n\n{}",
                registry::top_help()
            )));
        }
    };
    Ok((parsed, socket, timeout, password))
}

fn help_or_no_args(command: &str, tail: &[OsString], value: Offline) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    if !tail.is_empty() {
        return Err(CliError(format!("{command} accepts no arguments")));
    }
    Ok(Parsed::Offline(value))
}

fn app_no_args(
    command: &str,
    tail: &[OsString],
    value: ControlCommand,
) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    if !tail.is_empty() {
        return Err(CliError(format!("{command} accepts no arguments")));
    }
    Ok(Parsed::App(value))
}

fn help_or_exact(
    command: &str,
    tail: &[OsString],
    count: usize,
    build: impl FnOnce(&[String]) -> Result<ControlCommand, CliError>,
) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some(command.into()))));
    }
    if tail.len() != count {
        return Err(CliError(format!(
            "{command} expects {count} argument(s); run `zmux help {command}`"
        )));
    }
    let values = utf8_args(tail)?;
    Ok(Parsed::App(build(&values)?))
}

fn help_requested(tail: &[OsString]) -> bool {
    tail.len() == 1
        && tail[0]
            .to_str()
            .is_some_and(|value| value == "-h" || value == "--help")
}

fn utf8_args(args: &[OsString]) -> Result<Vec<String>, CliError> {
    args.iter()
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| CliError("command arguments must be valid UTF-8".into()))
        })
        .collect()
}

fn utf8_arg<'a>(argument: Option<&'a OsString>, label: &str) -> Result<&'a str, CliError> {
    argument
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError(format!("missing or invalid {label}")))
}

fn parse_notification_selector(value: &str) -> Result<NotificationSelector, CliError> {
    value.parse().map_err(|error| CliError(format!("{error}")))
}

fn parse_id(value: &str, label: &str) -> Result<u64, CliError> {
    let id = value
        .parse::<u64>()
        .map_err(|_| CliError(format!("{label} ID must be a positive integer")))?;
    if id == 0 {
        return Err(CliError(format!("{label} ID must be a positive integer")));
    }
    Ok(id)
}

fn client_options(
    socket_override: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
) -> Result<ClientOptions, CliError> {
    let socket = match socket_override {
        Some(path) => path,
        None => match env::var_os("ZMUX_SOCKET_PATH").or_else(|| env::var_os("ZMUX_SOCKET")) {
            Some(path) => PathBuf::from(path),
            None => default_socket_path()?,
        },
    };
    if !socket.is_absolute() {
        return Err(CliError(format!(
            "control socket path must be absolute: {}",
            socket.display()
        )));
    }
    let password =
        zmux_desktop_runtime::control_credentials::password(password).map_err(CliError)?;
    Ok(ClientOptions {
        socket,
        timeout,
        password,
    })
}

fn default_socket_path() -> Result<PathBuf, CliError> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| CliError("HOME is unset; pass --socket <ABSOLUTE_PATH>".into()))?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join("zmux")
            .join("control.sock"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
            && runtime.is_absolute()
        {
            return Ok(runtime.join("zmux").join("control.sock"));
        }
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| CliError("HOME is unset; pass --socket <ABSOLUTE_PATH>".into()))?;
        Ok(home
            .join(".local")
            .join("run")
            .join("zmux")
            .join("control.sock"))
    }
}

fn request(options: &ClientOptions, command: ControlCommand) -> Result<ControlResponse, CliError> {
    let deadline = Instant::now() + options.timeout;
    let request_id = request_id();
    let request = ControlRequest {
        version: CONTROL_VERSION,
        request_id,
        password: options.password.clone(),
        command,
    };
    let bytes = serde_json::to_vec(&request)
        .map_err(|error| CliError(format!("encode local-control request: {error}")))?;
    if bytes.len() > MAX_CONTROL_REQUEST_BYTES {
        return Err(CliError(format!(
            "request exceeds {MAX_CONTROL_REQUEST_BYTES} bytes"
        )));
    }
    let connect = || -> io::Result<UnixStream> {
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
        let address = socket2::SockAddr::unix(&options.socket)?;
        socket.connect_timeout(&address, socket_budget(deadline)?)?;
        socket.set_nonblocking(true)?;
        Ok(socket.into())
    };
    let mut stream = connect().map_err(|error| {
        let guidance = match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                "; start zmux and try again"
            }
            io::ErrorKind::PermissionDenied => "; verify that the socket belongs to your user",
            _ => "",
        };
        CliError(format!(
            "cannot connect to {}: {error}{guidance}",
            options.socket.display()
        ))
    })?;
    write_before(&mut stream, &(bytes.len() as u32).to_be_bytes(), deadline)
        .and_then(|()| write_before(&mut stream, &bytes, deadline))
        .map_err(|error| CliError(format!(
            "sending zmux request failed: {error}; its outcome may be unknown and the command was not retried"
        )))?;
    let mut response_length = [0_u8; 4];
    read_before(&mut stream, &mut response_length, deadline).map_err(|error| {
        CliError(format!(
            "waiting for zmux at {} failed: {error}; the command was not retried",
            options.socket.display()
        ))
    })?;
    let response_length = u32::from_be_bytes(response_length) as usize;
    if response_length == 0 || response_length > MAX_CONTROL_RESPONSE_BYTES {
        return Err(CliError(
            "running app returned an invalid response length".into(),
        ));
    }
    let mut response = vec![0; response_length];
    read_before(&mut stream, &mut response, deadline).map_err(|error| {
        CliError(format!(
            "reading zmux response failed: {error}; the command was not retried"
        ))
    })?;
    decode_response(&response, request_id, deadline)
}

fn decode_response(
    bytes: &[u8],
    request_id: u64,
    deadline: Instant,
) -> Result<ControlResponse, CliError> {
    let response: ControlResponse = serde_json::from_slice(bytes)
        .map_err(|error| CliError(format!("running app returned invalid JSON: {error}")))?;
    if response.version != CONTROL_VERSION {
        return Err(CliError(format!(
            "running app uses unsupported protocol version {}; this CLI requires v{CONTROL_VERSION}. Restart or update the app to match the CLI; the command was not retried",
            response.version
        )));
    }
    if response.request_id != request_id {
        return Err(CliError(
            "running app returned a mismatched request ID".into(),
        ));
    }
    // Byte-bounded JSON decoding is not preemptible. Reject an expired decoded
    // response before exposing success; subsequent output rendering has its own
    // CPU and stdout costs and is outside this response-acceptance deadline.
    socket_budget(deadline).map_err(|_| CliError(
        "decoding zmux response exceeded the request deadline; its outcome may be unknown and the command was not retried".into()
    ))?;
    Ok(response)
}

fn socket_budget(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline elapsed"))
}

// A disconnected Darwin Unix socket rejects setsockopt with EINVAL even when
// reply bytes remain buffered. Use nonblocking I/O and poll rather than changing
// SO_RCVTIMEO/SO_SNDTIMEO between partial transfers. A hangup is readiness: drain
// buffered bytes before accepting EOF, and retain one deadline across all waits.
fn wait_socket_ready(stream: &UnixStream, events: PollFlags, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = socket_budget(deadline)?;
        let timeout = PollTimeout::try_from(remaining.as_nanos().div_ceil(1_000_000))
            .unwrap_or(PollTimeout::MAX);
        let mut descriptors = [PollFd::new(stream.as_fd(), events)];
        match poll(&mut descriptors, timeout) {
            Ok(0) => continue,
            Ok(_) => return socket_budget(deadline).map(|_| ()),
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        socket_budget(deadline)?;
        match stream.read(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete zmux response",
                ));
            }
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_socket_ready(stream, PollFlags::POLLIN, deadline)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    socket_budget(deadline).map(|_| ())
}

fn write_before(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        socket_budget(deadline)?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "incomplete zmux request",
                ));
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_socket_ready(stream, PollFlags::POLLOUT, deadline)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    socket_budget(deadline).map(|_| ())
}

fn response_result(response: ControlResponse) -> Result<serde_json::Value, CliError> {
    if !response.ok {
        let error = response
            .error
            .ok_or_else(|| CliError("running app returned an invalid error response".into()))?;
        return Err(CliError(format!("{}: {}", error.code, error.message)));
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

fn request_id() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let id = nanos ^ u64::from(std::process::id()).rotate_left(32);
    id.max(1)
}

fn doctor(options: &ClientOptions) -> Result<(), CliError> {
    println!("socket: {}", options.socket.display());
    let response = request(options, ControlCommand::Status)?;
    if !response.ok {
        let error = response
            .error
            .map(|error| format!("{}: {}", error.code, error.message))
            .unwrap_or_else(|| "invalid error response".into());
        return Err(CliError(error));
    }
    println!("connection: ok");
    println!("protocol: v{CONTROL_VERSION}");
    println!(
        "status: {}",
        serde_json::to_string(&response.result.unwrap_or(serde_json::Value::Null))
            .map_err(|error| CliError(error.to_string()))?
    );
    Ok(())
}

fn print_help(command: Option<&str>) -> Result<(), CliError> {
    print!("{}", registry::help(command)?);
    Ok(())
}

fn run_prototype(args: Vec<String>) -> Result<u8, Box<dyn Error>> {
    let mut args = args.into_iter();
    let mut timeout = Duration::from_secs(10);
    loop {
        match args.next().as_deref() {
            Some("--timeout") => {
                let seconds: u64 = args.next().ok_or("missing timeout")?.parse()?;
                if seconds == 0 || seconds > 3600 {
                    return Err("timeout must be 1–3600 seconds".into());
                }
                timeout = Duration::from_secs(seconds);
            }
            Some("--") => break,
            _ => {
                return Err(
                    "usage: zmux prototype [--timeout SECONDS] -- PROGRAM [ARG ...]".into(),
                );
            }
        }
    }
    let program = args.next().ok_or("missing program")?;
    let mut session = Session::spawn(Launch {
        program,
        args: args.collect(),
        cwd: None,
        size: zmux_session_runtime::default_size(),
    })?;
    let deadline = Instant::now() + timeout;
    let result = loop {
        match session.snapshot() {
            Ok(snapshot) if snapshot.state != State::Running => break Ok(snapshot),
            Ok(_) | Err(SessionError::Busy) => {}
            Err(error) => break Err(error),
        }
        if Instant::now() >= deadline {
            break Err(SessionError::Failure(
                "prototype execution exceeded its timeout".into(),
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    session.shutdown(Duration::from_secs(3))?;
    let snapshot = result?;
    println!("{}", serde_json::to_string_pretty(&*snapshot)?);
    if let Some(failure) = &snapshot.failure {
        return Err(failure.clone().into());
    }
    Ok(snapshot.exit_code.unwrap_or(1).min(255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_peer_reply_bytes_are_read_before_eof() {
        let (mut client, mut peer) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        peer.write_all(b"headerbody").unwrap();
        drop(peer);
        let deadline = Instant::now() + Duration::from_secs(5);
        // The peer is already closed before either phase starts. Darwin must
        // not be asked to reset a socket option between these buffered reads.
        let mut header = [0; 6];
        read_before(&mut client, &mut header, deadline).unwrap();
        assert_eq!(&header, b"header");
        let mut body = [0; 4];
        read_before(&mut client, &mut body, deadline).unwrap();
        assert_eq!(&body, b"body");
        assert_eq!(
            read_before(&mut client, &mut body, deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn decoded_response_acceptance_requires_a_remaining_deadline() {
        let bytes = br#"{"version":2,"request_id":42,"ok":true,"result":{"answer":7}}"#;
        let valid = decode_response(bytes, 42, Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(valid.result.unwrap()["answer"], 7);

        let expired =
            decode_response(bytes, 42, Instant::now() - Duration::from_secs(1)).unwrap_err();
        assert!(
            expired
                .0
                .contains("decoding zmux response exceeded the request deadline")
        );
        assert!(expired.0.contains("not retried"));
    }
}
