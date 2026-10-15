use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::ExitCode,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zmux_desktop_runtime::attention::AttentionCommand;
use zmux_desktop_runtime::control::{
    CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, MAX_CONTROL_REQUEST_BYTES,
    MAX_CONTROL_RESPONSE_BYTES,
};
use zmux_session_runtime::{Launch, Session, SessionError, State};
use zmux_workspace_core::{Action, Axis};
mod registry;
mod targets;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

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
}

enum Parsed {
    Offline(Offline),
    App(ControlCommand),
}

enum Offline {
    Help(Option<String>),
    Version,
    Doctor,
    Prototype(Vec<String>),
    Guide,
    Catalog,
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
    let (parsed, socket_override, timeout) = parse(env::args_os().skip(1).collect())?;
    match parsed {
        Parsed::Offline(Offline::Help(command)) => {
            print_help(command.as_deref())?;
            Ok(0)
        }
        Parsed::Offline(Offline::Guide) => {
            print!("{}", registry::guide());
            Ok(0)
        }
        Parsed::Offline(Offline::Catalog) => {
            println!("{}", serde_json::to_string_pretty(&registry::catalog())?);
            Ok(0)
        }
        Parsed::Offline(Offline::Version) => {
            println!("zmux {VERSION}");
            Ok(0)
        }
        Parsed::Offline(Offline::Prototype(args)) => run_prototype(args),
        Parsed::Offline(Offline::Doctor) => {
            let options = client_options(socket_override, timeout)?;
            doctor(&options)?;
            Ok(0)
        }
        Parsed::App(command) => {
            let options = client_options(socket_override, timeout)?;
            let response = request(&options, command)?;
            if !response.ok {
                let error = response.error.ok_or_else(|| {
                    CliError("running app returned an invalid error response".into())
                })?;
                return Err(CliError(format!("{}: {}", error.code, error.message)).into());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&response.result.unwrap_or(serde_json::Value::Null))?
            );
            Ok(0)
        }
    }
}

fn parse(args: Vec<OsString>) -> Result<(Parsed, Option<PathBuf>, Duration), CliError> {
    let mut index = 0;
    let mut socket = None;
    let mut timeout = DEFAULT_TIMEOUT;
    while let Some(argument) = args.get(index).and_then(|value| value.to_str()) {
        match argument {
            "--socket" => {
                let path = args
                    .get(index + 1)
                    .ok_or_else(|| CliError("--socket requires a path".into()))?;
                socket = Some(PathBuf::from(path));
                index += 2;
            }
            "--timeout" => {
                let milliseconds = utf8_arg(args.get(index + 1), "timeout")?
                    .parse::<u64>()
                    .map_err(|_| CliError("timeout must be an integer in milliseconds".into()))?;
                if !(50..=30_000).contains(&milliseconds) {
                    return Err(CliError("timeout must be 50–30000 milliseconds".into()));
                }
                timeout = Duration::from_millis(milliseconds);
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
    if let Some(command) = command
        && registry::find(command).is_some_and(|spec| spec.targeted)
    {
        return Ok((targets::parse(command, tail)?, socket, timeout));
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
        Some("commands") => help_or_no_args("commands", tail, Offline::Catalog)?,
        Some("version") => help_or_no_args("version", tail, Offline::Version)?,
        Some("capabilities") => app_no_args("capabilities", tail, ControlCommand::Capabilities)?,
        Some("doctor") => help_or_no_args("doctor", tail, Offline::Doctor)?,
        Some("status") => app_no_args("status", tail, ControlCommand::Status)?,
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
        Some(
            "mark-notification-read"
            | "open-notification"
            | "dismiss-notification"
            | "clear-notifications",
        ) => {
            let name = command.unwrap();
            help_or_exact(name, tail, 2, |v| {
                let flag = if name == "clear-notifications" {
                    "--through"
                } else {
                    "--id"
                };
                if v[0] != flag {
                    return Err(CliError(format!("{name} requires {flag} ID")));
                }
                let id = parse_id(&v[1], "notification")?;
                Ok(ControlCommand::Attention {
                    action: match name {
                        "mark-notification-read" => AttentionCommand::Read { id },
                        "open-notification" => AttentionCommand::Open { id },
                        "dismiss-notification" => AttentionCommand::Dismiss { id },
                        _ => AttentionCommand::Clear { through_id: id },
                    },
                })
            })?
        }
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
    Ok((parsed, socket, timeout))
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
) -> Result<ClientOptions, CliError> {
    let socket = match socket_override {
        Some(path) => path,
        None => match env::var_os("ZMUX_SOCKET") {
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
    Ok(ClientOptions { socket, timeout })
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
    let request_id = request_id();
    let request = ControlRequest {
        version: CONTROL_VERSION,
        request_id,
        command,
    };
    let bytes = serde_json::to_vec(&request)
        .map_err(|error| CliError(format!("encode local-control request: {error}")))?;
    if bytes.len() > MAX_CONTROL_REQUEST_BYTES {
        return Err(CliError(format!(
            "request exceeds {MAX_CONTROL_REQUEST_BYTES} bytes"
        )));
    }
    let mut stream = UnixStream::connect(&options.socket).map_err(|error| {
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
    stream.set_read_timeout(Some(options.timeout))?;
    stream.set_write_timeout(Some(options.timeout))?;
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    let mut response_length = [0_u8; 4];
    stream.read_exact(&mut response_length).map_err(|error| {
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
    stream.read_exact(&mut response).map_err(|error| {
        CliError(format!(
            "reading zmux response failed: {error}; the command was not retried"
        ))
    })?;
    let response: ControlResponse = serde_json::from_slice(&response)
        .map_err(|error| CliError(format!("running app returned invalid JSON: {error}")))?;
    if response.version != CONTROL_VERSION {
        return Err(CliError(format!(
            "running app uses unsupported protocol version {}",
            response.version
        )));
    }
    if response.request_id != request_id {
        return Err(CliError(
            "running app returned a mismatched request ID".into(),
        ));
    }
    Ok(response)
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
