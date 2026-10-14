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
use zmux_desktop_runtime::control::{
    CONTROL_VERSION, ControlCommand, ControlRequest, ControlResponse, MAX_CONTROL_REQUEST_BYTES,
    MAX_CONTROL_RESPONSE_BYTES,
};
use zmux_session_runtime::{Launch, Session, SessionError, State};
use zmux_workspace_core::{Action, Axis, PaneId, SurfaceId, WorkspaceId};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const TOP_HELP: &str = r#"zmux — control a running zmux app or run a headless PTY

Usage: zmux [OPTIONS] <COMMAND>

Options:
  --socket <PATH>       Absolute local-control socket path
  --timeout <MILLIS>    Client deadline in milliseconds (50–30000)
  -h, --help            Print help
  -V, --version         Print version

Commands:
  doctor                Check the socket path and running app protocol
  status                Show running app and workspace counts
  list-workspaces       List workspaces and their stable IDs
  list-panes            List panes, sessions, processes, and states
  new-workspace         Create and focus a workspace
  rename-workspace      Rename a workspace by ID
  focus-pane            Focus a pane by ID
  split-pane            Split a pane horizontally or vertically
  close-pane            Close a pane and all its tabs
  new-surface           Create a terminal tab in a pane
  list-pane-surfaces    List every terminal tab in a pane
  select-surface        Select a tab and focus its pane
  close-surface         Close one tab; preserve sibling sessions
  rename-tab            Rename a terminal tab
  reorder-surface       Reorder a tab (zero-based index)
  move-surface          Move a live tab to another pane
  close-other-tabs      Close other tabs in a pane
  close-workspace       Close a workspace as one recoverable item
  reopen-closed         Reopen the last or selected closed item
  recently-closed       List bounded in-process close history
  send-text             Send exact UTF-8 text to a pane
  read-screen           Read a pane's current visible screen
  prototype             Run a standalone PTY command without the app
  help                  Print this help or help for one command

Local-control results are printed as JSON. Run `zmux help <COMMAND>` for details.
"#;

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
    let command = args.get(index).and_then(|value| value.to_str());
    let tail = &args[(index + 1).min(args.len())..];
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
        Some("doctor") => help_or_no_args("doctor", tail, Offline::Doctor)?,
        Some("status") => app_no_args("status", tail, ControlCommand::Status)?,
        Some("list-workspaces") => {
            app_no_args("list-workspaces", tail, ControlCommand::ListWorkspaces)?
        }
        Some("list-panes") => parse_list_panes(tail)?,
        Some("new-workspace") => help_or_exact("new-workspace", tail, 1, |values| {
            Ok(ControlCommand::NewWorkspace {
                name: values[0].clone(),
            })
        })?,
        Some("rename-workspace") => help_or_exact("rename-workspace", tail, 2, |values| {
            Ok(ControlCommand::RenameWorkspace {
                workspace_id: WorkspaceId(parse_id(&values[0], "workspace")?),
                name: values[1].clone(),
            })
        })?,
        Some("focus-pane") => help_or_exact("focus-pane", tail, 1, |values| {
            Ok(ControlCommand::FocusPane {
                pane_id: PaneId(parse_id(&values[0], "pane")?),
            })
        })?,
        Some("split-pane") => help_or_exact("split-pane", tail, 2, |values| {
            let axis = match values[1].as_str() {
                "horizontal" => Axis::Horizontal,
                "vertical" => Axis::Vertical,
                _ => {
                    return Err(CliError(
                        "split axis must be `horizontal` or `vertical`".into(),
                    ));
                }
            };
            Ok(ControlCommand::SplitPane {
                pane_id: PaneId(parse_id(&values[0], "pane")?),
                axis,
            })
        })?,
        Some("close-pane") => help_or_exact("close-pane", tail, 1, |values| {
            Ok(ControlCommand::ClosePane {
                pane_id: PaneId(parse_id(&values[0], "pane")?),
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
        Some("new-surface") => help_or_exact("new-surface", tail, 1, |v| {
            Ok(ControlCommand::Action {
                action: Action::CreateSurface {
                    pane_id: PaneId(parse_id(&v[0], "pane")?),
                },
            })
        })?,
        Some("list-pane-surfaces") => help_or_exact("list-pane-surfaces", tail, 1, |v| {
            Ok(ControlCommand::ListPaneSurfaces {
                pane_id: PaneId(parse_id(&v[0], "pane")?),
            })
        })?,
        Some("select-surface" | "close-surface" | "close-other-tabs") => {
            let command = command.unwrap();
            help_or_exact(command, tail, 1, |v| {
                let surface_id = SurfaceId(parse_id(&v[0], "surface")?);
                Ok(ControlCommand::Action {
                    action: match command {
                        "select-surface" => Action::SelectSurface { surface_id },
                        "close-surface" => Action::CloseSurface { surface_id },
                        _ => Action::CloseOtherSurfaces { surface_id },
                    },
                })
            })?
        }
        Some("rename-tab") => help_or_exact("rename-tab", tail, 2, |v| {
            Ok(ControlCommand::Action {
                action: Action::RenameSurface {
                    surface_id: SurfaceId(parse_id(&v[0], "surface")?),
                    name: v[1].clone(),
                },
            })
        })?,
        Some("reorder-surface" | "move-surface") => {
            let command = command.unwrap();
            help_or_exact(
                command,
                tail,
                if command == "move-surface" { 3 } else { 2 },
                |v| {
                    let surface_id = SurfaceId(parse_id(&v[0], "surface")?);
                    let index = v
                        .last()
                        .unwrap()
                        .parse::<usize>()
                        .map_err(|_| CliError("index must be a nonnegative integer".into()))?;
                    Ok(ControlCommand::Action {
                        action: if command == "move-surface" {
                            Action::MoveSurface {
                                surface_id,
                                pane_id: PaneId(parse_id(&v[1], "pane")?),
                                index,
                            }
                        } else {
                            Action::ReorderSurface { surface_id, index }
                        },
                    })
                },
            )?
        }
        Some("close-workspace") => help_or_exact("close-workspace", tail, 1, |v| {
            Ok(ControlCommand::Action {
                action: Action::CloseWorkspace {
                    workspace_id: WorkspaceId(parse_id(&v[0], "workspace")?),
                },
            })
        })?,
        Some("send-text") => parse_send_text(tail)?,
        Some("read-screen") => help_or_exact("read-screen", tail, 1, |values| {
            Ok(ControlCommand::ReadScreen {
                pane_id: PaneId(parse_id(&values[0], "pane")?),
            })
        })?,
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
            return Err(CliError(format!("unknown command `{other}`\n\n{TOP_HELP}")));
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

fn parse_list_panes(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("list-panes".into()))));
    }
    let values = utf8_args(tail)?;
    let workspace_id = match values.as_slice() {
        [] => None,
        [flag, id] if flag == "--workspace" => Some(WorkspaceId(parse_id(id, "workspace")?)),
        _ => {
            return Err(CliError(
                "usage: zmux list-panes [--workspace WORKSPACE_ID]".into(),
            ));
        }
    };
    Ok(Parsed::App(ControlCommand::ListPanes { workspace_id }))
}

fn parse_send_text(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("send-text".into()))));
    }
    let values = utf8_args(tail)?;
    if values.len() != 2 {
        return Err(CliError(
            "usage: zmux send-text PANE_ID <TEXT|--stdin>".into(),
        ));
    }
    let text = if values[1] == "--stdin" {
        let mut bytes = Vec::new();
        io::stdin().take(4_097).read_to_end(&mut bytes)?;
        if bytes.len() > 4_096 {
            return Err(CliError("stdin text exceeds 4096 bytes".into()));
        }
        String::from_utf8(bytes).map_err(|_| CliError("stdin is not valid UTF-8".into()))?
    } else {
        values[1].clone()
    };
    Ok(Parsed::App(ControlCommand::SendText {
        pane_id: PaneId(parse_id(&values[0], "pane")?),
        text,
    }))
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
    let text = match command {
        None => TOP_HELP,
        Some("doctor") => {
            "Usage: zmux [OPTIONS] doctor\n\nChecks the socket path, connection, protocol version, and running app status.\n"
        }
        Some("status") => {
            "Usage: zmux [OPTIONS] status\n\nShows protocol version, revision, workspace/pane counts, and session state counts.\n"
        }
        Some("list-workspaces") => {
            "Usage: zmux [OPTIONS] list-workspaces\n\nLists every workspace with its stable ID, name, pane tree, and focused pane.\n"
        }
        Some("list-panes") => {
            "Usage: zmux [OPTIONS] list-panes [--workspace WORKSPACE_ID]\n\nLists pane/session IDs, owning workspaces, process IDs, startup state, and failures.\n"
        }
        Some("new-workspace") => {
            "Usage: zmux [OPTIONS] new-workspace NAME\n\nCreates a workspace through the same Rust action used by the native app.\n"
        }
        Some("rename-workspace") => {
            "Usage: zmux [OPTIONS] rename-workspace WORKSPACE_ID NAME\n\nRenames an existing workspace. Names are trimmed and must contain 1–128 non-control characters.\n"
        }
        Some("focus-pane") => {
            "Usage: zmux [OPTIONS] focus-pane PANE_ID\n\nFocuses the pane and its owning workspace.\n"
        }
        Some("split-pane") => {
            "Usage: zmux [OPTIONS] split-pane PANE_ID <horizontal|vertical>\n\nSplits the pane and focuses the newly created pane.\n"
        }
        Some("close-pane") => {
            "Usage: zmux [OPTIONS] close-pane PANE_ID\n\nCloses the pane and cancels all its terminal tabs. Closing the last pane removes its workspace.\n"
        }
        Some("send-text") => {
            "Usage: zmux [OPTIONS] send-text PANE_ID <TEXT|--stdin>\n\nSends exact UTF-8 bytes in order. No newline is added; shell input usually ends with a carriage return. Input is limited to 4096 bytes and is never retried after an uncertain result.\n"
        }
        Some("read-screen") => {
            "Usage: zmux [OPTIONS] read-screen PANE_ID\n\nReads the current bounded visible screen, cursor, size, process state, and exit metadata.\n"
        }
        Some("new-surface") => {
            "Usage: zmux new-surface PANE_ID\n\nCreates and selects an independent local terminal tab without splitting.\n"
        }
        Some("list-pane-surfaces") => {
            "Usage: zmux list-pane-surfaces PANE_ID\n\nLists all tabs, stable IDs, selected state and process readiness, including hidden tabs.\n"
        }
        Some("select-surface") => {
            "Usage: zmux select-surface SURFACE_ID\n\nSelects a live tab and focuses its pane/workspace.\n"
        }
        Some("close-surface") => {
            "Usage: zmux close-surface SURFACE_ID\n\nCloses only this tab and records a restore descriptor. An empty pane/workspace collapses.\n"
        }
        Some("rename-tab") => {
            "Usage: zmux rename-tab SURFACE_ID TITLE\n\nSets a terminal tab title.\n"
        }
        Some("reorder-surface") => {
            "Usage: zmux reorder-surface SURFACE_ID INDEX\n\nMoves a tab to a zero-based final index in its pane.\n"
        }
        Some("move-surface") => {
            "Usage: zmux move-surface SURFACE_ID PANE_ID INDEX\n\nMoves the live session to another pane at a zero-based final index. No shell is respawned.\n"
        }
        Some("close-other-tabs") => {
            "Usage: zmux close-other-tabs SURFACE_ID\n\nCloses sibling tabs as one recoverable group. This explicit CLI command does not prompt.\n"
        }
        Some("close-workspace") => {
            "Usage: zmux close-workspace WORKSPACE_ID\n\nCloses all workspace terminals as one recoverable layout. This explicit CLI command does not prompt.\n"
        }
        Some("reopen-closed") => {
            "Usage: zmux reopen-closed [CLOSED_ID]\n\nStarts restoration of the latest or selected item. Completion is asynchronous; check recently-closed and tab readiness. Local shells start fresh at the saved cwd; commands are never replayed. Failed launches retain history for retry. Removed panes fall back to a new workspace.\n"
        }
        Some("recently-closed") => {
            "Usage: zmux recently-closed\n\nLists up to 32 in-process closed items and their restore state, oldest first. History is cleared when zmux exits.\n"
        }
        Some("prototype") => {
            "Usage: zmux prototype [--timeout SECONDS] -- PROGRAM [ARG ...]\n\nRuns a standalone local Unix PTY and prints its final visible screen as JSON. It does not require the native app.\n"
        }
        Some("help") => {
            "Usage: zmux help [COMMAND]\n\nPrints top-level help or detailed help for one implemented command.\n"
        }
        Some(other) => return Err(CliError(format!("unknown command `{other}`"))),
    };
    print!("{text}");
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
