//! Explicit opt-in local provider integration. Hook output is never a receipt.
use super::*;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::Path,
    process::{Child, Command, Stdio},
};
use uuid::Uuid;
use zmux_desktop_runtime::agent_messages::{MessageAction, MessageRequest, ProviderExposure};
use zmux_session_runtime::child::{ProcessIdentity, process_identity};
mod plugin;
const EVENT_BUDGET: Duration = Duration::from_millis(2700);
const FRAME_LIMIT: usize = 65536;
const ROUTE_ENV: &str = "ZMUX_CLAUDE_EVENT_SOCKET";

pub(super) enum Invocation {
    Install {
        directory: PathBuf,
        provider: PathBuf,
    },
    Status {
        directory: PathBuf,
    },
    Uninstall {
        directory: PathBuf,
    },
    Run {
        directory: PathBuf,
        surface: Uuid,
        args: Vec<OsString>,
    },
    Event {
        event: String,
    },
}
pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("hooks".into()))));
    }
    let args = utf8_args(tail)?;
    if args.first().map(String::as_str) != Some("claude") {
        return Err(CliError("hooks currently supports claude only".into()));
    }
    if args.get(1).map(String::as_str) == Some("event") {
        return match args.as_slice() {
            [_, _, event] if events().contains(&event.as_str()) => {
                Ok(Parsed::Hooks(Invocation::Event {
                    event: event.clone(),
                }))
            }
            _ => Err(CliError("unsupported internal Claude hook event".into())),
        };
    }
    let verb = args.get(1).map(String::as_str).ok_or_else(|| {
        CliError("hooks claude requires install, status, uninstall or run".into())
    })?;
    let (mut directory, mut provider, mut surface) = (None, None, None);
    let mut cursor = 2;
    while cursor < args.len() && args[cursor] != "--" {
        let slot=match args[cursor].as_str() {
            "--directory"=>&mut directory,"--provider" if verb=="install"=>&mut provider,
            "--surface" if verb=="run"=>&mut surface,
            _=>return Err(CliError("hooks requires explicit --directory, install --provider, or run --surface; provider arguments follow --".into())),
        };
        if slot.is_some() {
            return Err(CliError("repeated hook option".into()));
        }
        *slot = Some(
            args.get(cursor + 1)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CliError("hook option requires a value".into()))?
                .clone(),
        );
        cursor += 2;
    }
    let directory = PathBuf::from(directory.ok_or_else(|| {
        CliError("hooks requires --directory pointing to private zmux-owned plugin storage".into())
    })?);
    let invocation = match verb {
        "install" if cursor == args.len() => Invocation::Install {
            directory,
            provider: PathBuf::from(provider.ok_or_else(|| {
                CliError("install requires absolute --provider executable".into())
            })?),
        },
        "status" if cursor == args.len() => Invocation::Status { directory },
        "uninstall" if cursor == args.len() => Invocation::Uninstall { directory },
        "run" => {
            let surface = surface
                .ok_or_else(|| CliError("run requires exact --surface UUID".into()))?
                .parse::<Uuid>()
                .map_err(|_| CliError("run surface must be a canonical UUID".into()))?;
            if surface.is_nil() {
                return Err(CliError("run surface cannot be nil".into()));
            }
            let provider_args = if cursor < tail.len() {
                tail[cursor + 1..].to_vec()
            } else {
                vec![]
            };
            interactive_args(&provider_args)?;
            Invocation::Run {
                directory,
                surface,
                args: provider_args,
            }
        }
        _ => return Err(CliError("invalid hooks claude command".into())),
    };
    Ok(Parsed::Hooks(invocation))
}
fn events() -> [&'static str; 7] {
    [
        "SessionStart",
        "UserPromptSubmit",
        "PermissionRequest",
        "Elicitation",
        "PreToolUse",
        "Stop",
        "SessionEnd",
    ]
}
fn interactive_args(args: &[OsString]) -> Result<bool, CliError> {
    let args = utf8_args(args)?;
    let (mut cursor, mut positional, mut plan) = (0, 0, false);
    let mut seen = HashSet::new();
    while cursor < args.len() {
        let arg = args[cursor].as_str();
        match arg {
            "--"=> {positional+=args.len()-cursor-1;break;}
            "--model"|"--effort"|"--permission-mode"=> {
                if !seen.insert(arg) {return Err(CliError("repeated supported provider option".into()));}
                let value=args.get(cursor+1).filter(|v|!v.is_empty()&&!v.starts_with('-')).ok_or_else(||CliError("provider option requires a value".into()))?;
                if arg=="--permission-mode" {
                    if !["default","acceptEdits","plan"].contains(&value.as_str()) {return Err(CliError("unsupported provider permission mode".into()));}
                    plan=value=="plan";
                }
                cursor+=1;
            }
            _ if arg.starts_with('-')=>return Err(CliError("unsupported provider option: this wrapper only supports interactive --model, --effort and --permission-mode; no headless/resume/custom plugin/settings modes".into())),
            "resume"|"update"|"install"|"mcp"|"auth"|"agents"|"doctor"|"plugin"=>return Err(CliError("provider subcommands are not interactive adapter launches".into())),
            _=>positional+=1,
        }
        cursor += 1;
    }
    if positional > 1 {
        return Err(CliError(
            "interactive provider launch accepts at most one initial prompt".into(),
        ));
    }
    Ok(plan)
}
pub(super) fn run(
    invocation: Invocation,
    socket: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
) -> Result<u8, Box<dyn Error>> {
    match invocation {
        Invocation::Event { event } => {
            // Every failure is a nonblocking provider no-op; do not print input,
            // body, capability or provider diagnostics to stderr.
            if event_client(&event).is_err() {
                let _ = stdout_before(b"{}", Instant::now() + Duration::from_millis(50));
            }
            Ok(0)
        }
        Invocation::Install {
            directory,
            provider,
        } => {
            if !provider.is_absolute() {
                return Err(CliError("provider path must be absolute".into()).into());
            }
            let expected = plugin::binary(&provider)?;
            let cancellation = bounded_command::Cancellation::new()?;
            let output = bounded_command::capture(
                Command::new(&expected.path).arg("--version"),
                Instant::now() + DEFAULT_TIMEOUT,
                &cancellation,
            )?;
            if output.status != zmux_session_runtime::child::ExitObservation::Exited(0)
                || output.stdout.trim() != format!("{} (Claude Code)", plugin::SUPPORTED_VERSION)
                || plugin::binary(&expected.path)? != expected
            {
                return Err(CliError("only the unchanged Claude Code 2.1.301 executable is supported; no plugin published".into()).into());
            }
            println!("{}", plugin::install(&directory, expected)?);
            Ok(0)
        }
        Invocation::Status { directory } => {
            println!("{}", plugin::status(&directory)?);
            Ok(0)
        }
        Invocation::Uninstall { directory } => {
            println!("{}", plugin::uninstall(&directory)?);
            Ok(0)
        }
        Invocation::Run {
            directory,
            surface,
            args,
        } => {
            let options = client_options(socket, timeout.min(EVENT_BUDGET), password)?;
            launch(&options, &directory, surface, args).map_err(Into::into)
        }
    }
}
fn monotonic_ms() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (t.tv_sec as u64).saturating_mul(1000) + (t.tv_nsec as u64) / 1_000_000
}
fn send_frame(stream: &mut UnixStream, value: &Value, deadline: Instant) -> Result<(), CliError> {
    let bytes = serde_json::to_vec(value).map_err(|_| CliError("invalid hook frame".into()))?;
    if bytes.len() > FRAME_LIMIT {
        return Err(CliError("hook frame exceeds64KiB".into()));
    }
    write_before(stream, &(bytes.len() as u32).to_be_bytes(), deadline)?;
    write_before(stream, &bytes, deadline)?;
    Ok(())
}
fn read_frame(stream: &mut UnixStream, deadline: Instant) -> Result<Value, CliError> {
    let mut length = [0; 4];
    read_before(stream, &mut length, deadline)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > FRAME_LIMIT {
        return Err(CliError("invalid hook frame length".into()));
    }
    let mut bytes = vec![0; length];
    read_before(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| CliError("invalid hook frame JSON".into()))
}
fn pipe_ready<T: AsFd>(pipe: &T, event: PollFlags, deadline: Instant) -> Result<(), CliError> {
    loop {
        let millis = socket_budget(deadline)?
            .as_millis()
            .clamp(1, i32::MAX as u128) as u32;
        let mut descriptors = [PollFd::new(pipe.as_fd(), event)];
        match poll(
            &mut descriptors,
            PollTimeout::try_from(millis).unwrap_or(PollTimeout::MAX),
        ) {
            Ok(0) => (),
            Ok(_) => return Ok(()),
            Err(nix::errno::Errno::EINTR) => (),
            Err(e) => return Err(io::Error::from(e).into()),
        }
    }
}
fn stdout_before(bytes: &[u8], deadline: Instant) -> Result<(), CliError> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let mut out = io::stdout();
    let flags = fcntl(out.as_raw_fd(), FcntlArg::F_GETFL).map_err(io::Error::from)?;
    fcntl(
        out.as_raw_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(io::Error::from)?;
    let mut remaining = bytes;
    let result = (|| -> Result<(), CliError> {
        while !remaining.is_empty() {
            socket_budget(deadline)?;
            match out.write(remaining) {
                Ok(0) => return Err(CliError("hook stdout closed".into())),
                Ok(n) => remaining = &remaining[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    pipe_ready(&out, PollFlags::POLLOUT, deadline)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e.into()),
            }
        }
        out.flush()?;
        Ok(())
    })();
    fcntl(
        out.as_raw_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags)),
    )
    .map_err(io::Error::from)?;
    result
}
fn event_client(event: &str) -> Result<(), CliError> {
    let deadline = Instant::now() + EVENT_BUDGET;
    let expires = monotonic_ms() + EVENT_BUDGET.as_millis() as u64;
    let mut input = io::stdin();
    let mut bytes = Vec::new();
    loop {
        pipe_ready(&input, PollFlags::POLLIN, deadline)?;
        let mut chunk = [0; 4096];
        let n = input.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > FRAME_LIMIT - 1024 {
            return Err(CliError("oversized hook input".into()));
        }
    }
    let payload: Value =
        serde_json::from_slice(&bytes).map_err(|_| CliError("malformed hook input".into()))?;
    let Some(path) = env::var_os(ROUTE_ENV) else {
        return Err(CliError("hook has no owned route".into()));
    };
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.connect_timeout(
        &socket2::SockAddr::unix(PathBuf::from(path))?,
        socket_budget(deadline)?,
    )?;
    socket.set_nonblocking(true)?;
    let mut stream: UnixStream = socket.into();
    send_frame(
        &mut stream,
        &json!({"event":event,"input":payload,"expires_monotonic_ms":expires}),
        deadline,
    )?;
    let result = read_frame(&mut stream, deadline)?;
    let output = result
        .get("output")
        .filter(|v| v.is_object())
        .ok_or_else(|| CliError("invalid hook response".into()))?;
    let encoded =
        serde_json::to_vec(output).map_err(|_| CliError("invalid hook response".into()))?;
    // There is deliberately no Ack/ReadTurn, even after complete stdout.
    if stdout_before(&encoded, deadline).is_err() {
        return Ok(());
    }
    if result["attempt_id"].is_string() {
        // One IPC exchange, with an optional final local transport report. Loss
        // of this report leaves the pre-existing durable unknown fence intact.
        let _ = send_frame(&mut stream, &json!({"submission_reported":true}), deadline);
    }
    Ok(())
}
fn operation(
    options: &ClientOptions,
    deadline: Instant,
    action: MessageAction,
) -> Result<Value, CliError> {
    agent_messages::completed(
        options,
        deadline,
        MessageRequest {
            request_id: Uuid::new_v4(),
            caller: Default::default(),
            action,
        },
    )
}
#[derive(Clone, PartialEq, Eq)]
struct Route {
    namespace: Uuid,
    surface: Uuid,
    session: u64,
    terminal: ProcessIdentity,
}
fn route(options: &ClientOptions, surface: Uuid, deadline: Instant) -> Result<Route, CliError> {
    let budget = ClientOptions {
        socket: options.socket.clone(),
        password: options.password.clone(),
        timeout: socket_budget(deadline)?,
    };
    let report = response_result(request(&budget, ControlCommand::Current { limit: 200 })?)?;
    let namespace = report["identity_namespace"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .filter(|id: &Uuid| !id.is_nil())
        .ok_or_else(|| CliError("invalid route namespace".into()))?;
    let item = report["items"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|v| v["surface"]["uuid"] == surface.to_string())
        })
        .ok_or_else(|| CliError("exact provider surface absent from Desktop".into()))?;
    if item["kind"] != "terminal"
        || item["session"]["scope"] != "local_session_child"
        || item["session"]["state"] != "running"
    {
        return Err(CliError(
            "adapter requires a running local terminal session".into(),
        ));
    }
    let session = item["session_id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| CliError("missing exact SessionId".into()))?;
    let terminal_pid = item["session"]["published_local_child_pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| CliError("missing terminal child identity".into()))?;
    let terminal = process_identity(terminal_pid)?;
    Ok(Route {
        namespace,
        surface,
        session,
        terminal,
    })
}
fn peer(stream: &UnixStream) -> Result<ProcessIdentity, CliError> {
    use nix::sys::socket::{getsockopt, sockopt};
    #[cfg(target_os = "linux")]
    let (pid, uid) = {
        let c = getsockopt(stream, sockopt::PeerCredentials).map_err(io::Error::from)?;
        (c.pid(), c.uid())
    };
    #[cfg(target_os = "macos")]
    let (pid, uid) = {
        let p = getsockopt(stream, sockopt::LocalPeerPid).map_err(io::Error::from)?;
        let (u, _) = nix::unistd::getpeereid(stream).map_err(io::Error::from)?;
        (p, u.as_raw())
    };
    if pid <= 0 || uid != nix::unistd::geteuid().as_raw() {
        return Err(CliError("hook peer credentials rejected".into()));
    }
    let identity = process_identity(pid as u32)?;
    if identity.uid != uid {
        return Err(CliError("hook peer credentials changed".into()));
    }
    Ok(identity)
}
struct Adapter<'a> {
    options: &'a ClientOptions,
    directory: &'a Path,
    installation: plugin::Installation,
    route: Route,
    root_session: String,
    runtime: Uuid,
    generation: Uuid,
    provider: ProcessIdentity,
    registration: Option<(Uuid, String)>,
    retirement: Option<MessageRequest>,
    cleanup_deadline: Option<Instant>,
    held: bool,
    disabled: bool,
    prompts: HashSet<String>,
}
impl Adapter<'_> {
    fn retire(&mut self, deadline: Instant) -> Result<(), CliError> {
        self.disabled = true;
        if let Some((consumer, token)) = &self.registration {
            // Keep exactly the original capability and request UUID after an
            // uncertain result. Context admissions stay disabled. Only standalone
            // shutdown cleanup may retry, under its own single stored deadline.
            let request = self
                .retirement
                .get_or_insert_with(|| MessageRequest {
                    request_id: Uuid::new_v4(),
                    caller: Default::default(),
                    action: MessageAction::Retire {
                        surface: self.route.surface.to_string(),
                        consumer: *consumer,
                        token: token.clone(),
                    },
                })
                .clone();
            agent_messages::completed(self.options, deadline, request)?;
            self.registration = None;
            self.retirement = None;
        }
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), CliError> {
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
        self.retire(deadline)
    }
    fn current(&self) -> bool {
        plugin::metadata_matches(&self.installation.provider)
            && plugin::metadata_matches(&self.installation.adapter)
            && plugin::active(self.directory)
                .is_ok_and(|i| i.generation == self.installation.generation)
    }
    fn hook(
        &mut self,
        event: &str,
        input: &Value,
        deadline: Instant,
    ) -> Result<(Value, Option<Uuid>), CliError> {
        let noop = || (json!({}), None);
        if self.disabled
            || !self.current()
            || input["hook_event_name"] != event
            || input["session_id"] != self.root_session
            || input.get("agent_id").is_some_and(|v| !v.is_null())
            || input.get("agent_type").is_some_and(|v| !v.is_null())
        {
            return Ok(noop());
        }
        if route(self.options, self.route.surface, deadline)? != self.route {
            self.retire(deadline)?;
            return Ok(noop());
        }
        match event {
            "SessionStart" => {
                if input["source"] != "startup" || self.registration.is_some() {
                    return Ok(noop());
                }
                let value = operation(
                    self.options,
                    deadline,
                    MessageAction::Register {
                        surface: self.route.surface.to_string(),
                        provider: "claude-local-hook".into(),
                        interactive: true,
                    },
                )?;
                let consumer = value["consumer"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| CliError("invalid consumer registration".into()))?;
                let token = value["token"]
                    .as_str()
                    .ok_or_else(|| CliError("missing consumer capability".into()))?
                    .to_owned();
                self.registration = Some((consumer, token));
                if value["namespace"] != self.route.namespace.to_string()
                    || value["session"] != self.route.session
                    || value["surface_id"] != self.route.surface.to_string()
                {
                    self.retire(deadline)?;
                    return Ok(noop());
                }
                if self.held {
                    self.hold(deadline)?;
                }
                Ok(noop())
            }
            "SessionEnd" => {
                self.retire(deadline)?;
                Ok(noop())
            }
            "PermissionRequest" | "Elicitation" => {
                self.held = true;
                self.hold(deadline)?;
                Ok(noop())
            }
            "PreToolUse"
                if ["AskUserQuestion", "EnterPlanMode"]
                    .iter()
                    .any(|n| input["tool_name"] == *n) =>
            {
                self.held = true;
                self.hold(deadline)?;
                Ok(noop())
            }
            "UserPromptSubmit" => {
                if input["permission_mode"] == "plan" {
                    self.held = true;
                    self.hold(deadline)?;
                }
                if self.held
                    || !["default", "acceptEdits"]
                        .iter()
                        .any(|mode| input["permission_mode"] == *mode)
                {
                    return Ok(noop());
                }
                let Some(prompt) = input["prompt_id"].as_str().filter(|s| {
                    !s.is_empty()
                        && s.len() <= 128
                        && !s.chars().any(char::is_control)
                        && s.parse::<Uuid>().is_ok_and(|id| !id.is_nil())
                }) else {
                    return Ok(noop());
                };
                if self.prompts.contains(prompt) {
                    return Ok(noop());
                }
                if self.prompts.len() >= 256 {
                    self.retire(deadline)?;
                    return Ok(noop());
                }
                let Some((consumer, token)) = &self.registration else {
                    return Ok(noop());
                };
                self.prompts.insert(prompt.into());
                let result = operation(
                    self.options,
                    deadline,
                    MessageAction::Claim {
                        surface: self.route.surface.to_string(),
                        consumer: *consumer,
                        token: token.clone(),
                        max_messages: Some(1),
                        exposure: Some(ProviderExposure {
                            provider: "claude".into(),
                            provider_version: self.installation.provider_version.clone(),
                            runtime_id: self.runtime,
                            adapter_generation: self.generation,
                            session_id: self.root_session.clone(),
                            prompt_id: prompt.into(),
                        }),
                    },
                )?;
                let Some(context) = result["additional_context"].as_str() else {
                    return Ok(noop());
                };
                if context.chars().count() >= 9000
                    || !self.current()
                    || route(self.options, self.route.surface, deadline)? != self.route
                {
                    return Ok(noop());
                }
                let attempt = result["messages"][0]["delivery_attempts"]
                    .as_array()
                    .and_then(|a| a.last())
                    .and_then(|a| a["id"].as_str())
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| CliError("missing durable exposure attempt".into()))?;
                Ok((
                    json!({"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":context}}),
                    Some(attempt),
                ))
            }
            // Stop does not claim, continue, Ack or ReadTurn. Generic lifecycle
            // completion cannot prove that any context was accepted.
            _ => Ok(noop()),
        }
    }
    fn hold(&self, deadline: Instant) -> Result<(), CliError> {
        if let Some((consumer, token)) = &self.registration {
            operation(
                self.options,
                deadline,
                MessageAction::Poll {
                    surface: self.route.surface.to_string(),
                    consumer: *consumer,
                    token: token.clone(),
                    held: Some(true),
                },
            )?;
        }
        Ok(())
    }
    fn serve(&mut self, mut stream: UnixStream) -> Result<(), CliError> {
        stream.set_nonblocking(true)?;
        let initial = Instant::now() + EVENT_BUDGET;
        let client = peer(&stream)?;
        // Exec-form hooks are direct children. This is local process binding,
        // not cryptographic provider authentication or a provider receipt.
        if client.executable != self.installation.adapter.path
            || client.parent_pid != self.provider.pid
            || process_identity(self.provider.pid)? != self.provider
        {
            return Err(CliError("hook process binding rejected".into()));
        }
        let frame = read_frame(&mut stream, initial)?;
        let expires = frame["expires_monotonic_ms"]
            .as_u64()
            .ok_or_else(|| CliError("missing original hook deadline".into()))?;
        let remaining = expires
            .saturating_sub(monotonic_ms())
            .min(EVENT_BUDGET.as_millis() as u64);
        if remaining == 0 {
            return Err(CliError("hook expired before admission".into()));
        }
        let deadline = initial.min(Instant::now() + Duration::from_millis(remaining));
        let event = frame["event"]
            .as_str()
            .filter(|e| events().contains(e))
            .ok_or_else(|| CliError("unknown hook event".into()))?;
        if process_identity(client.pid)? != client
            || process_identity(self.provider.pid)? != self.provider
        {
            return Err(CliError("hook process changed before admission".into()));
        }
        let (output, attempt) = self.hook(event, &frame["input"], deadline)?;
        if process_identity(client.pid)? != client
            || process_identity(self.provider.pid)? != self.provider
        {
            return Err(CliError("hook process changed before response".into()));
        }
        send_frame(
            &mut stream,
            &json!({"output":output,"attempt_id":attempt}),
            deadline,
        )?;
        if let Some(attempt_id) = attempt {
            let report = read_frame(&mut stream, deadline)?;
            if report["submission_reported"] == true
                && let Some((consumer, token)) = &self.registration
            {
                operation(
                    self.options,
                    deadline,
                    MessageAction::ReportSubmission {
                        surface: self.route.surface.to_string(),
                        consumer: *consumer,
                        token: token.clone(),
                        attempt_id,
                    },
                )?;
            }
        }
        Ok(())
    }
}
// Match std::os::unix::fs::MetadataExt's unsigned identity representation.
// Darwin dev_t is signed i32; Linux rustix already exposes a u64 device ID.
fn stat_identity(stat: &rustix::fs::Stat) -> (u64, u64) {
    #[cfg(target_os = "macos")]
    let device = stat.st_dev as u64;
    #[cfg(not(target_os = "macos"))]
    let device = stat.st_dev;
    (device, stat.st_ino)
}
struct SocketDirectory {
    path: PathBuf,
    directory: plugin::Directory,
    parent: std::fs::File,
    socket: Option<(u64, u64)>,
}
impl Drop for SocketDirectory {
    fn drop(&mut self) {
        let result = (|| -> Result<(), CliError> {
            use rustix::fs::{self as at, AtFlags};
            let name = self
                .path
                .file_name()
                .ok_or_else(|| CliError("missing owned socket directory name".into()))?;
            let current = at::statat(&self.parent, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(io::Error::from)?;
            let pinned = self.directory.0.metadata()?;
            if stat_identity(&current) != (pinned.dev(), pinned.ino()) {
                return Err(CliError("owned socket directory replaced".into()));
            }
            if let Some(expected) = self.socket {
                let socket = at::statat(&self.directory.0, "event.sock", AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(io::Error::from)?;
                if stat_identity(&socket) != expected {
                    return Err(CliError("owned hook socket replaced".into()));
                }
                at::unlinkat(&self.directory.0, "event.sock", AtFlags::empty())
                    .map_err(io::Error::from)?;
            }
            at::unlinkat(&self.parent, name, AtFlags::REMOVEDIR).map_err(io::Error::from)?;
            Ok(())
        })();
        if result.is_err() {
            eprintln!("zmux: exact hook socket cleanup incomplete; unknown replacements retained");
        }
    }
}
struct Signals {
    ids: Vec<signal_hook::SigId>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Signals {
    fn new() -> Result<Self, CliError> {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut signals = Self { ids: vec![], stop };
        for signal in [signal_hook::consts::SIGHUP, signal_hook::consts::SIGTERM] {
            signals.ids.push(signal_hook::flag::register(
                signal,
                std::sync::Arc::clone(&signals.stop),
            )?);
        }
        // Terminal SIGINT already reaches the provider in the same foreground
        // group. Keep the wrapper alive for ordinary provider turn cancellation.
        signals.ids.push(signal_hook::flag::register(
            signal_hook::consts::SIGINT,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?);
        Ok(signals)
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for id in &self.ids {
            signal_hook::low_level::unregister(*id);
        }
    }
}
impl Drop for Adapter<'_> {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            eprintln!("zmux: adapter cleanup could not confirm exact capability retirement");
        }
    }
}

fn launch(
    options: &ClientOptions,
    directory: &Path,
    surface: Uuid,
    args: Vec<OsString>,
) -> Result<u8, CliError> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError(
            "Claude adapter run requires an interactive terminal; headless is unsupported".into(),
        ));
    }
    let held = interactive_args(&args)?;
    let installed = plugin::active(directory)?;
    if plugin::binary(&installed.provider.path)? != installed.provider
        || plugin::binary(&env::current_exe()?)? != installed.adapter
    {
        return Err(CliError(
            "provider or adapter executable changed; install a fresh owned generation".into(),
        ));
    }
    let _lease = plugin::lease(directory, &installed)?;
    let route = route(options, surface, Instant::now() + options.timeout)?;
    let mut ancestor = process_identity(std::process::id())?;
    let mut bound = false;
    for _ in 0..32 {
        if ancestor == route.terminal {
            bound = true;
            break;
        }
        if ancestor.parent_pid <= 1 {
            break;
        }
        ancestor = process_identity(ancestor.parent_pid)?;
    }
    if !bound {
        return Err(CliError(
            "wrapper is not running inside the exact Desktop terminal process lifetime".into(),
        ));
    }
    let root_session = Uuid::new_v4().to_string();
    let runtime = Uuid::new_v4();
    // Darwin permits at most 103 pathname bytes. TMPDIR commonly lives under
    // a long per-user /var/folders path; choose a bounded path before creating
    // anything. The random directory remains private even in shared /tmp.
    let name = format!("zmux-hook-{}", runtime.simple());
    let mut path = env::temp_dir().canonicalize()?.join(&name);
    if path.join("event.sock").as_os_str().as_bytes().len() > 103 {
        path = Path::new("/tmp").canonicalize()?.join(&name);
    }
    if path.join("event.sock").as_os_str().as_bytes().len() > 103 {
        return Err(CliError(
            "hook socket path exceeds the Unix pathname limit".into(),
        ));
    }
    let parent = std::fs::File::from(
        rustix::fs::open(
            path.parent()
                .ok_or_else(|| CliError("missing hook socket parent".into()))?,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    fs::DirBuilder::new().mode(0o700).create(&path)?;
    let owned_directory = plugin::Directory::open(&path, false)?;
    let socket_path = path.join("event.sock");
    let mut socket_dir = SocketDirectory {
        path,
        directory: owned_directory,
        parent,
        socket: None,
    };
    let listener = UnixListener::bind(&socket_path)?;
    let metadata = fs::symlink_metadata(&socket_path)?;
    socket_dir.socket = Some((metadata.dev(), metadata.ino()));
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let signals = Signals::new()?;
    let mut command = Command::new(&installed.provider.path);
    command
        .args([
            OsString::from("--plugin-dir"),
            directory
                .join(installed.generation.to_string())
                .into_os_string(),
            OsString::from("--session-id"),
            root_session.clone().into(),
        ])
        .args(args);
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("ZMUX_") {
            command.env_remove(name);
        }
    }
    command
        .env(ROUTE_ENV, &socket_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child: Child = command.spawn()?;
    let identity_deadline = Instant::now() + Duration::from_secs(1);
    let provider = loop {
        match process_identity(child.id()) {
            Ok(identity)
                if identity.executable == installed.provider.path
                    && identity.parent_pid == std::process::id()
                    && plugin::metadata_matches(&installed.provider)
                    && plugin::metadata_matches(&installed.adapter) =>
            {
                break identity;
            }
            _ if Instant::now() < identity_deadline => thread::sleep(Duration::from_millis(2)),
            _ => {
                // Retain the directly owned provider. It is never killed or
                // relaunched to repair messaging; no registration was created.
                eprintln!("zmux: provider process binding unavailable; adapter disabled");
                return Ok(exit_code(child.wait()?));
            }
        }
    };
    let mut adapter = Adapter {
        options,
        directory,
        installation: installed,
        route,
        root_session,
        runtime,
        generation: Uuid::new_v4(),
        provider,
        registration: None,
        retirement: None,
        cleanup_deadline: None,
        held,
        disabled: false,
        prompts: HashSet::new(),
    };
    let mut next_idle_check = Instant::now();
    let status = loop {
        if Instant::now() >= next_idle_check {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if !adapter.disabled
                && (!plugin::publication_matches(directory, adapter.installation.generation)
                    || signals.stop.load(std::sync::atomic::Ordering::Acquire))
                && adapter.cleanup().is_err()
            {
                eprintln!("zmux: adapter disabled; exact retirement unconfirmed");
            }
            next_idle_check = Instant::now() + Duration::from_millis(250);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if adapter.serve(stream).is_err() {
                    eprintln!("zmux: local hook unavailable; no acknowledgement inferred");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                let wait = next_idle_check.saturating_duration_since(Instant::now());
                let timeout = PollTimeout::try_from(wait.as_millis().clamp(1, 250) as u32)
                    .unwrap_or(PollTimeout::MAX);
                let mut descriptors = [PollFd::new(listener.as_fd(), PollFlags::POLLIN)];
                match poll(&mut descriptors, timeout) {
                    Ok(_) | Err(nix::errno::Errno::EINTR) => (),
                    Err(error) => {
                        if adapter.cleanup().is_err() {
                            eprintln!("zmux: hook poll retirement unconfirmed");
                        }
                        eprintln!("zmux: hook listener poll failed; provider retained: {error}");
                        break child.wait()?;
                    }
                }
            }
            Err(e) => {
                if adapter.cleanup().is_err() {
                    eprintln!("zmux: listener failure retirement unconfirmed");
                }
                eprintln!(
                    "zmux: hook listener failed; provider retained: {}",
                    e.kind()
                );
                break child.wait()?;
            }
        }
    };
    if adapter.cleanup().is_err() {
        eprintln!("zmux: provider exited; exact adapter retirement unconfirmed");
    }
    Ok(exit_code(status))
}
fn exit_code(status: std::process::ExitStatus) -> u8 {
    status.code().unwrap_or(1).clamp(0, 255) as u8
}
