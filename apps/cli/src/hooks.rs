//! Explicit opt-in local provider integration. Hook output is never a receipt.
use super::*;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::UnixListener,
        },
    },
    path::Path,
    process::{Child, Command, Stdio},
};
use uuid::Uuid;
use zmux_desktop_runtime::agent_lifecycle::{
    Action as LifecycleAction, ClaudeHookEvent, Event as LifecycleEvent, HookDisposition,
    Provider as LifecycleProvider, Request as LifecycleRequest, Target as LifecycleTarget,
};
use zmux_desktop_runtime::agent_messages::{MessageAction, MessageRequest, ProviderExposure};
use zmux_session_runtime::child::{ProcessIdentity, process_identity};
use zmux_workspace_core::{SessionId, SurfaceId};
mod plugin;
mod provider;
const EVENT_BUDGET: Duration = Duration::from_millis(2700);
const LIFECYCLE_BUDGET: Duration = Duration::from_millis(150);
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
        notice_plugin: Option<PathBuf>,
        args: Vec<OsString>,
    },
    Auto {
        root: PathBuf,
        surface: Uuid,
        notice_plugin: PathBuf,
        args: Vec<OsString>,
    },
    Notice {
        event: String,
    },
    CombinedEvent {
        event: String,
    },
    Cleanup {
        root: PathBuf,
    },
    Event {
        event: String,
    },
    Provider(provider::Invocation),
}
pub(super) fn parse(tail: &[OsString]) -> Result<Parsed, CliError> {
    if help_requested(tail) {
        return Ok(Parsed::Offline(Offline::Help(Some("hooks".into()))));
    }
    let separator = tail.iter().position(|argument| argument == "--");
    let prefix = &tail[..separator.unwrap_or(tail.len())];
    let args = utf8_args(prefix)?;
    if args.first().map(String::as_str) == Some("provider") {
        return provider::parse(tail)
            .map(|invocation| Parsed::Hooks(Invocation::Provider(invocation)));
    }
    if args.first().map(String::as_str) != Some("claude") {
        return Err(CliError(
            "hooks supports claude and the session-scoped provider auto adapters".into(),
        ));
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
    if args.get(1).map(String::as_str) == Some("notice") {
        return match args.as_slice() {
            [_, _, event] if matches!(event.as_str(), "Notification" | "Stop") => {
                Ok(Parsed::Hooks(Invocation::Notice {
                    event: event.clone(),
                }))
            }
            _ => Err(CliError("unsupported internal Claude notice event".into())),
        };
    }
    if args.get(1).map(String::as_str) == Some("combined-event") {
        return match args.as_slice() {
            [_, _, event] if matches!(event.as_str(), "Notification" | "Stop") => {
                Ok(Parsed::Hooks(Invocation::CombinedEvent {
                    event: event.clone(),
                }))
            }
            _ => Err(CliError(
                "unsupported internal combined Claude hook event".into(),
            )),
        };
    }
    let verb = args.get(1).map(String::as_str).ok_or_else(|| {
        CliError("hooks claude requires install, status, uninstall, run, auto or cleanup".into())
    })?;
    let (mut directory, mut provider, mut surface, mut root, mut notice_plugin) =
        (None, None, None, None, None);
    let mut cursor = 2;
    while cursor < args.len() {
        let slot = match args[cursor].as_str() {
            "--directory" if matches!(verb, "install" | "status" | "uninstall" | "run") => {
                &mut directory
            }
            "--provider" if verb == "install" => &mut provider,
            "--surface" if matches!(verb, "run" | "auto") => &mut surface,
            "--root" if matches!(verb, "auto" | "cleanup") => &mut root,
            "--notice-plugin" if matches!(verb, "run" | "auto") => &mut notice_plugin,
            _ => return Err(CliError(
                "hooks requires explicit command-specific options; provider arguments follow --"
                    .into(),
            )),
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
    let invocation =
        match verb {
            "install" if cursor == args.len() && separator.is_none() => Invocation::Install {
                directory: PathBuf::from(directory.ok_or_else(|| {
                    CliError(
                        "hooks requires --directory pointing to private zmux-owned plugin storage"
                            .into(),
                    )
                })?),
                provider: PathBuf::from(provider.ok_or_else(|| {
                    CliError("install requires absolute --provider executable".into())
                })?),
            },
            "status" if cursor == args.len() && separator.is_none() => Invocation::Status {
                directory: PathBuf::from(directory.ok_or_else(|| {
                    CliError(
                        "hooks requires --directory pointing to private zmux-owned plugin storage"
                            .into(),
                    )
                })?),
            },
            "uninstall" if cursor == args.len() && separator.is_none() => Invocation::Uninstall {
                directory: PathBuf::from(directory.ok_or_else(|| {
                    CliError(
                        "hooks requires --directory pointing to private zmux-owned plugin storage"
                            .into(),
                    )
                })?),
            },
            "run" => {
                let surface = surface
                    .ok_or_else(|| CliError("run requires exact --surface UUID".into()))?
                    .parse::<Uuid>()
                    .map_err(|_| CliError("run surface must be a canonical UUID".into()))?;
                if surface.is_nil() {
                    return Err(CliError("run surface cannot be nil".into()));
                }
                let provider_args = separator
                    .map(|index| tail[index + 1..].to_vec())
                    .unwrap_or_default();
                interactive_args(&provider_args)?;
                Invocation::Run {
                    directory: PathBuf::from(directory.ok_or_else(|| {
                        CliError("run requires private zmux-owned --directory".into())
                    })?),
                    surface,
                    notice_plugin: notice_plugin.map(PathBuf::from),
                    args: provider_args,
                }
            }
            "auto" if separator.is_some() && cursor == args.len() => {
                let surface = surface
                    .ok_or_else(|| CliError("auto requires exact --surface UUID".into()))?
                    .parse::<Uuid>()
                    .map_err(|_| CliError("auto surface must be a canonical UUID".into()))?;
                if surface.is_nil() {
                    return Err(CliError("auto surface cannot be nil".into()));
                }
                Invocation::Auto {
                    root: PathBuf::from(root.ok_or_else(|| {
                        CliError("auto requires the private session --root".into())
                    })?),
                    surface,
                    notice_plugin: PathBuf::from(notice_plugin.ok_or_else(|| {
                        CliError("auto requires the existing session --notice-plugin".into())
                    })?),
                    args: separator
                        .map(|index| tail[index + 1..].to_vec())
                        .unwrap_or_default(),
                }
            }
            "cleanup" if cursor == args.len() && separator.is_none() => Invocation::Cleanup {
                root: PathBuf::from(root.ok_or_else(|| {
                    CliError("cleanup requires the private session --root".into())
                })?),
            },
            _ => return Err(CliError("invalid hooks claude command".into())),
        };
    Ok(Parsed::Hooks(invocation))
}
fn events() -> [&'static str; 12] {
    [
        "SessionStart",
        "UserPromptSubmit",
        "PermissionRequest",
        "Elicitation",
        "ElicitationResult",
        "Notification",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "StopFailure",
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

const PROVIDER_HELP_LIMIT: usize = 65_536;

struct TemporaryProviderHelpOutput {
    directory: PathBuf,
    path: PathBuf,
}

impl Drop for TemporaryProviderHelpOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

fn temporary_provider_help_output() -> Result<TemporaryProviderHelpOutput, CliError> {
    let parent = env::temp_dir().canonicalize()?;
    for _ in 0..8 {
        let directory = parent.join(format!("zmux-claude-help-{}", Uuid::new_v4().simple()));
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {
                let output = TemporaryProviderHelpOutput {
                    path: directory.join("help.txt"),
                    directory,
                };
                let file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&output.path)?;
                drop(file);
                return Ok(output);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(CliError(
        "cannot allocate bounded Claude help output storage".into(),
    ))
}

fn provider_help(
    provider: &Path,
    cancellation: &bounded_command::Cancellation,
) -> Result<String, CliError> {
    let output_file = temporary_provider_help_output()?;
    let command_output = bounded_command::capture(
        Command::new("/bin/sh")
            .args([
                "-c",
                "ulimit -f 128 || exit 125; \"$1\" --help >\"$2\" 2>&1",
                "zmux-claude-help",
            ])
            .arg(provider)
            .arg(&output_file.path),
        Instant::now() + DEFAULT_TIMEOUT,
        cancellation,
    )?;
    if command_output.status != zmux_session_runtime::child::ExitObservation::Exited(0) {
        return Err(CliError(
            "Claude --help preflight did not exit successfully within its bounds".into(),
        ));
    }

    let metadata = fs::symlink_metadata(&output_file.path)?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() > PROVIDER_HELP_LIMIT as u64
    {
        return Err(CliError(
            "Claude --help output exceeded its private capture bounds".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut file = fs::File::open(&output_file.path)?;
    Read::by_ref(&mut file)
        .take(PROVIDER_HELP_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > PROVIDER_HELP_LIMIT {
        return Err(CliError(
            "Claude --help output exceeded its private capture bounds".into(),
        ));
    }
    String::from_utf8(bytes).map_err(|_| CliError("Claude --help returned invalid UTF-8".into()))
}

fn repeatable_plugin_dir(help: &str) -> bool {
    let lines = help.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let option = line.trim_start();
        let Some(rest) = option.strip_prefix("--plugin-dir") else {
            continue;
        };
        let argument = rest.trim_start();
        // Help examples can begin with `--plugin-dir A`; only inspect the
        // option declaration, which includes the argument placeholder.
        if !argument.starts_with('<') && !argument.starts_with('=') {
            continue;
        }
        let mut section = option.to_ascii_lowercase();
        for continuation in lines.iter().skip(index + 1) {
            let continuation = continuation.trim_start();
            if continuation.starts_with("--") {
                break;
            }
            section.push(' ');
            section.push_str(continuation);
        }
        return section.contains("repeatable") || section.contains("can be used multiple times");
    }
    false
}

fn supported_provider(path: &Path) -> Result<plugin::VerifiedProvider, CliError> {
    if !path.is_absolute() {
        return Err(CliError("provider path must be absolute".into()));
    }
    let expected = plugin::binary(path)?;
    let cancellation = bounded_command::Cancellation::new()?;
    let version_output = bounded_command::capture(
        Command::new(&expected.path).arg("--version"),
        Instant::now() + DEFAULT_TIMEOUT,
        &cancellation,
    )?;
    let version = version_output
        .stdout
        .trim()
        .strip_suffix(" (Claude Code)")
        .filter(|version| plugin::supports_provider_version(version))
        .map(str::to_owned);
    if version_output.status != zmux_session_runtime::child::ExitObservation::Exited(0)
        || version.is_none()
    {
        return Err(CliError(format!(
            "Claude Code version is unsupported; the reviewed patch range is {} through {}",
            plugin::MIN_SUPPORTED_VERSION,
            plugin::MAX_SUPPORTED_VERSION
        )));
    }
    if !plugin::metadata_matches(&expected) {
        return Err(CliError(
            "Claude executable changed during --version preflight".into(),
        ));
    }

    let help = provider_help(&expected.path, &cancellation)?;
    if !repeatable_plugin_dir(&help) {
        return Err(CliError(format!(
            "Claude Code {} does not advertise repeatable --plugin-dir support",
            version.as_deref().expect("validated Claude version")
        )));
    }
    if plugin::binary(&expected.path)? != expected {
        return Err(CliError(
            "Claude executable changed during --help preflight".into(),
        ));
    }
    Ok(plugin::VerifiedProvider {
        binary: expected,
        version: version.expect("validated Claude version"),
    })
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
        Invocation::CombinedEvent { event } => {
            combined_event(&event);
            Ok(0)
        }
        Invocation::Notice { event } => {
            notice_event(&event)?;
            Ok(0)
        }
        Invocation::Install {
            directory,
            provider,
        } => {
            let expected = supported_provider(&provider)?;
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
            notice_plugin,
            args,
        } => {
            let options = client_options(socket, timeout.min(EVENT_BUDGET), password)?;
            let held = interactive_args(&args)?;
            launch(
                &options,
                &directory,
                surface,
                notice_plugin.as_deref(),
                args,
                held,
            )
            .map_err(Into::into)
        }
        Invocation::Auto {
            root,
            surface,
            notice_plugin,
            args,
        } => automatic_launch(
            socket,
            timeout,
            password,
            &root,
            surface,
            &notice_plugin,
            args,
        ),
        Invocation::Cleanup { root } => {
            println!("{}", cleanup_auto_plugins(&root)?);
            Ok(0)
        }
        Invocation::Provider(invocation) => provider::run(invocation, socket, timeout, password),
    }
}

fn find_provider() -> Result<PathBuf, CliError> {
    let search_path = env::var_os("PATH").ok_or_else(|| CliError("PATH is unavailable".into()))?;
    for directory in env::split_paths(&search_path) {
        let candidate = directory.join("claude");
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if metadata.is_file() && metadata.mode() & 0o111 != 0 {
            return candidate.canonicalize().map_err(Into::into);
        }
    }
    Err(CliError("Claude executable was not found on PATH".into()))
}

fn launch_plain(
    provider: &Path,
    notice_plugin: &Path,
    args: Vec<OsString>,
) -> Result<u8, CliError> {
    let mut command = Command::new(provider);
    command.env_remove("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE");
    if notice_plugin.is_absolute()
        && fs::metadata(notice_plugin).is_ok_and(|metadata| metadata.is_dir())
    {
        command.arg("--plugin-dir").arg(notice_plugin);
    }
    let status = command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(exit_code(status))
}

fn auto_fallback(
    provider: &Path,
    notice_plugin: &Path,
    args: Vec<OsString>,
    reason: &str,
) -> Result<u8, Box<dyn Error>> {
    eprintln!(
        "zmux: automatic Claude lifecycle unavailable ({reason}); launching this request once with the session notice plugin"
    );
    launch_plain(provider, notice_plugin, args).map_err(Into::into)
}

fn create_auto_plugin_directory(root: &Path) -> Result<PathBuf, CliError> {
    if !root.is_absolute() {
        return Err(CliError(
            "automatic Claude session root must be absolute".into(),
        ));
    }
    plugin::Directory::open(root, false)?;
    for _ in 0..8 {
        let path = root.join(format!("claude-auto-{}", Uuid::new_v4().simple()));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(CliError(
        "cannot allocate a unique automatic Claude plugin directory".into(),
    ))
}

fn cleanup_auto_directory(root: &Path, directory: &Path) -> Result<bool, CliError> {
    let _lock = plugin::auto_lock(root)?;
    clear_launch_marker(directory)?;
    let result = plugin::uninstall(directory)?;
    if result["cleanup_pending"] == true {
        return Ok(false);
    }
    plugin::remove_empty_auto_directory(directory)?;
    Ok(true)
}

fn create_launch_marker(directory: &Path) -> Result<(), CliError> {
    let path = directory.join("launch-unknown");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(b"provider spawn outcome pending\n")?;
    file.sync_all()?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn clear_launch_marker(directory: &Path) -> Result<(), CliError> {
    let path = directory.join("launch-unknown");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(CliError(
            "automatic Claude launch marker changed; retained".into(),
        ));
    }
    fs::remove_file(path)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn cleanup_auto_plugins(root: &Path) -> Result<Value, CliError> {
    let _lock = plugin::auto_lock(root)?;
    let mut removed = 0usize;
    let mut retained = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(identifier) = name.strip_prefix("claude-auto-") else {
            continue;
        };
        if Uuid::parse_str(identifier).is_err() {
            continue;
        }
        let directory = entry.path();
        if !fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            retained.push(name.to_owned());
            continue;
        }
        if fs::symlink_metadata(directory.join("launch-unknown")).is_ok() {
            retained.push(name.to_owned());
            continue;
        }
        match plugin::uninstall_if_idle(&directory) {
            Ok(result) if result["cleanup_pending"] != true => {
                if plugin::remove_empty_auto_directory(&directory).is_ok() {
                    removed += 1;
                } else {
                    retained.push(name.to_owned());
                }
            }
            _ => retained.push(name.to_owned()),
        }
    }
    if !retained.is_empty() {
        eprintln!(
            "zmux: automatic Claude plugin directories retained for review: {}",
            retained.join(", ")
        );
    }
    Ok(json!({"removed":removed,"retained":retained}))
}

fn automatic_launch(
    socket: Option<PathBuf>,
    timeout: Duration,
    password: Option<SocketPassword>,
    root: &Path,
    surface: Uuid,
    notice_plugin: &Path,
    args: Vec<OsString>,
) -> Result<u8, Box<dyn Error>> {
    use std::io::IsTerminal;
    let provider = find_provider()?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return auto_fallback(
            &provider,
            notice_plugin,
            args,
            "this invocation has no interactive terminal",
        );
    }
    let held = match interactive_args(&args) {
        Ok(held) => held,
        Err(_) => {
            return auto_fallback(
                &provider,
                notice_plugin,
                args,
                "this Claude invocation uses unsupported adapter options",
            );
        }
    };
    let expected = match supported_provider(&provider) {
        Ok(provider) => provider,
        Err(_) => {
            return auto_fallback(
                &provider,
                notice_plugin,
                args,
                "this Claude version is outside the supported adapter range",
            );
        }
    };
    let options = match client_options(socket, timeout.min(EVENT_BUDGET), password) {
        Ok(options) => options,
        Err(_) => {
            return auto_fallback(
                &provider,
                notice_plugin,
                args,
                "the exact Desktop lifecycle route is unavailable",
            );
        }
    };
    let route = match route(&options, surface, Instant::now() + options.timeout).and_then(|route| {
        check_terminal_ancestor(&route)?;
        Ok(route)
    }) {
        Ok(route) => route,
        Err(_) => {
            return auto_fallback(
                &provider,
                notice_plugin,
                args,
                "the exact Desktop terminal could not be verified",
            );
        }
    };

    let mut auto_directory = None;
    let setup = (|| -> Result<PreparedPlugin, CliError> {
        let _lock = plugin::auto_lock(root)?;
        let directory = create_auto_plugin_directory(root)?;
        auto_directory = Some(directory.clone());
        if let Err(error) = plugin::install(&directory, expected) {
            let _ = plugin::uninstall(&directory);
            let _ = plugin::remove_empty_auto_directory(&directory);
            return Err(CliError(format!(
                "session-only lifecycle plugin installation failed: {error}"
            )));
        }
        let prepared = prepare_plugin(&directory)
            .map_err(|error| CliError(format!("prepare installed lifecycle plugin: {error}")))?;
        clear_launch_marker(&directory)
            .map_err(|error| CliError(format!("clear stale launch marker: {error}")))?;
        create_launch_marker(&directory)
            .map_err(|error| CliError(format!("create launch uncertainty marker: {error}")))?;
        Ok(prepared)
    })();
    let prepared = match setup {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("zmux: automatic Claude lifecycle setup failed ({error})");
            if let Some(directory) = &auto_directory {
                let _ = cleanup_auto_directory(root, directory);
            }
            return auto_fallback(
                &provider,
                notice_plugin,
                args,
                "the session-only lifecycle plugin could not be prepared",
            );
        }
    };
    let directory = auto_directory.expect("successful setup created a private plugin directory");

    // Once this call reaches provider spawn, failures never start a second copy.
    let result = launch_prepared(LaunchRequest {
        options: &options,
        directory: &directory,
        route,
        prepared,
        notice_plugin: Some(notice_plugin),
        args: args.clone(),
        held,
        inbox_enabled: false,
        automatic_root: Some(root),
    });
    match result {
        Ok(code) => {
            match cleanup_auto_directory(root, &directory) {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    eprintln!("zmux: automatic Claude plugin cleanup remains pending")
                }
            }
            Ok(code)
        }
        Err(LaunchFailure::BeforeStart(_)) => {
            let _ = cleanup_auto_directory(root, &directory);
            auto_fallback(
                &provider,
                notice_plugin,
                args,
                "adapter setup failed before the provider was started",
            )
        }
        Err(LaunchFailure::AfterStart(error)) => {
            eprintln!("zmux: provider launch outcome is uncertain; adapter files retained");
            Err(error.into())
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
fn hook_input(deadline: Instant) -> Result<Value, CliError> {
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
    serde_json::from_slice(&bytes).map_err(|_| CliError("malformed hook input".into()))
}

fn event_client(event: &str) -> Result<HookDisposition, CliError> {
    let deadline = Instant::now() + EVENT_BUDGET;
    let payload = hook_input(deadline)?;
    event_client_payload(event, payload, deadline)
}

fn event_client_payload(
    event_name: &str,
    payload: Value,
    deadline: Instant,
) -> Result<HookDisposition, CliError> {
    let event: ClaudeHookEvent = serde_json::from_value(payload.clone())
        .map_err(|_| CliError("invalid typed Claude hook input".into()))?;
    if event.name() != event_name {
        return Err(CliError(
            "hook event name does not match its typed input".into(),
        ));
    }
    let hook_id = Uuid::new_v4();
    let expires = monotonic_ms().saturating_add(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u64,
    );
    let Some(path) = env::var_os(ROUTE_ENV) else {
        return Err(CliError("hook has no owned route".into()));
    };
    let input = minimal_local_hook_input(&event);
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.connect_timeout(
        &socket2::SockAddr::unix(PathBuf::from(path))?,
        socket_budget(deadline)?,
    )?;
    socket.set_nonblocking(true)?;
    let mut stream: UnixStream = socket.into();
    send_frame(
        &mut stream,
        &json!({"event":event_name,"hook_id":hook_id,"input":input,"expires_monotonic_ms":expires}),
        deadline,
    )?;
    let result = read_frame(&mut stream, deadline)?;
    let disposition = match result["hook_disposition"].as_str() {
        Some("alert_covered") => HookDisposition::AlertCovered,
        Some("handled_no_alert") => HookDisposition::HandledNoAlert,
        Some("not_handled") => HookDisposition::NotHandled,
        _ => return Err(CliError("invalid lifecycle hook receipt".into())),
    };
    let output = result
        .get("output")
        .filter(|v| v.is_object())
        .ok_or_else(|| CliError("invalid hook response".into()))?;
    let encoded =
        serde_json::to_vec(output).map_err(|_| CliError("invalid hook response".into()))?;
    // There is deliberately no Ack/ReadTurn, even after complete stdout.
    let _ = stdout_before(&encoded, deadline);
    if result["attempt_id"].is_string() {
        // One IPC exchange, with an optional final local transport report. Loss
        // of this report leaves the pre-existing durable unknown fence intact.
        let _ = send_frame(&mut stream, &json!({"submission_reported":true}), deadline);
    }
    Ok(disposition)
}

/// Send only the typed facts used by the local adapter. Provider prompt text,
/// tool arguments/results, error bodies, task details, and unknown fields stay
/// out of the local IPC frame.
fn minimal_local_hook_input(event: &ClaudeHookEvent) -> Value {
    let mut input = json!({
        "hook_event_name": event.name(),
        "session_id": event.session_id(),
    });
    match event {
        ClaudeHookEvent::SessionStart { source, .. } => {
            input["source"] = json!(if source == "startup" {
                "startup"
            } else {
                "other"
            });
        }
        ClaudeHookEvent::UserPromptSubmit {
            prompt_id,
            permission_mode,
            ..
        } => {
            input["prompt_id"] = json!(prompt_id);
            input["permission_mode"] = json!(match permission_mode.as_str() {
                "default" | "acceptEdits" | "plan" => permission_mode.as_str(),
                _ => "other",
            });
        }
        ClaudeHookEvent::ElicitationResult { action, .. } => {
            input["action"] = json!(match action.as_str() {
                "accept" | "decline" | "cancel" => action.as_str(),
                _ => "other",
            });
        }
        ClaudeHookEvent::Notification {
            notification_type, ..
        } => input["notification_type"] = json!(notification_type),
        ClaudeHookEvent::PreToolUse { tool_name, .. } => {
            input["tool_name"] = json!(if tool_name == "AskUserQuestion" {
                "AskUserQuestion"
            } else if tool_name == "EnterPlanMode" {
                "EnterPlanMode"
            } else {
                "Other"
            });
        }
        ClaudeHookEvent::PostToolUse { .. } | ClaudeHookEvent::PostToolUseFailure { .. } => {
            input["tool_name"] = json!("Other");
        }
        ClaudeHookEvent::Stop {
            stop_hook_active,
            background_tasks,
            ..
        } => {
            if let Some(stop_hook_active) = stop_hook_active {
                input["stop_hook_active"] = json!(stop_hook_active);
            }
            if let Some(background_tasks) = background_tasks {
                input["background_tasks"] = if background_tasks.is_empty() {
                    json!([])
                } else {
                    json!([true])
                };
            }
        }
        ClaudeHookEvent::SessionEnd { reason, .. } => {
            if let Some(reason) = reason {
                input["reason"] = json!(match reason.as_str() {
                    "logout" | "prompt_input_exit" => reason.as_str(),
                    _ => "other",
                });
            }
        }
        ClaudeHookEvent::PermissionRequest { .. }
        | ClaudeHookEvent::Elicitation { .. }
        | ClaudeHookEvent::StopFailure { .. } => {}
    }
    if !event.is_main_session() {
        input["agent_id"] = json!("redacted");
    }
    input
}

fn combined_event(event_name: &str) {
    if env::var_os("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE").as_deref() != Some(OsStr::new("1")) {
        let _ = stdout_before(b"{}", Instant::now() + Duration::from_millis(50));
        return;
    }
    let deadline = Instant::now() + EVENT_BUDGET;
    let input = hook_input(deadline);
    let parsed_input = input.as_ref().ok();
    let idle_prompt = event_name == "Notification"
        && parsed_input.is_some_and(|payload| payload["notification_type"] == "idle_prompt");
    let fallback_on_not_handled =
        parsed_input.and_then(|payload| silent_fallback_kind(payload, event_name));
    let fallback_on_error = uncertain_fallback_kind(parsed_input, event_name);
    let disposition = input.and_then(|payload| event_client_payload(event_name, payload, deadline));
    let fallback = match disposition {
        Ok(HookDisposition::AlertCovered | HookDisposition::HandledNoAlert) => None,
        // A valid idle_prompt follows Stop. Stop owns the NotHandled fallback;
        // repeating it here would create a second completion notice.
        Ok(HookDisposition::NotHandled) if idle_prompt => None,
        Ok(HookDisposition::NotHandled) => fallback_on_not_handled,
        Err(_) => fallback_on_error,
    };
    if let Some(fallback) = fallback
        && let Some(root) = env::var_os("ZMUX_SHELL_INTEGRATION_DIRECTORY")
    {
        // The owner receipt is exact to this Zmux hook invocation. A missing,
        // rejected or ambiguous receipt keeps a visual-only terminal fallback;
        // the generic OS notification path must not add a second sound.
        let _ = write_notice_event(&PathBuf::from(root), fallback);
    }
    if disposition.is_err() {
        let _ = stdout_before(b"{}", Instant::now() + Duration::from_millis(50));
    }
}

fn silent_fallback_kind(payload: &Value, event_name: &str) -> Option<&'static str> {
    if payload.get("hook_event_name").and_then(Value::as_str) != Some(event_name) {
        return None;
    }
    match event_name {
        "Stop" => Some("stop_silent"),
        "Notification"
            if matches!(
                payload.get("notification_type").and_then(Value::as_str),
                Some("permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog")
            ) =>
        {
            Some("attention_silent")
        }
        _ => None,
    }
}

fn uncertain_fallback_kind(payload: Option<&Value>, event_name: &str) -> Option<&'static str> {
    match event_name {
        "Stop" => Some("stop_silent"),
        "Notification" => match payload.and_then(|payload| payload["notification_type"].as_str()) {
            Some("idle_prompt") => Some("stop_silent"),
            Some("permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog") => {
                Some("attention_silent")
            }
            _ => None,
        },
        _ => None,
    }
}

fn notice_event(event_name: &str) -> Result<(), CliError> {
    if env::var_os("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE").as_deref() != Some(OsStr::new("1")) {
        return Err(CliError("lifecycle notice routing is not active".into()));
    }
    combined_event(event_name);
    Ok(())
}

fn write_notice_event(root: &Path, event: &str) -> Result<(), CliError> {
    if !matches!(
        event,
        "attention" | "stop" | "failure" | "attention_silent" | "stop_silent"
    ) {
        return Err(CliError("unsupported session notice event".into()));
    }
    let directory = plugin::Directory::open(root, false)?;
    let fifo = std::fs::File::from(
        rustix::fs::openat(
            &directory.0,
            "attention",
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    let metadata = fifo.metadata()?;
    if !metadata.file_type().is_fifo()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(CliError("unsafe session notice FIFO".into()));
    }
    let mut fifo = fifo;
    fifo.write_all(event.as_bytes())?;
    fifo.write_all(b"\n")?;
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
fn lifecycle_operation(
    options: &ClientOptions,
    deadline: Instant,
    lifecycle_request: LifecycleRequest,
) -> Result<Value, CliError> {
    let deadline = deadline.min(Instant::now() + LIFECYCLE_BUDGET);
    let budget = ClientOptions {
        socket: options.socket.clone(),
        password: options.password.clone(),
        timeout: socket_budget(deadline)?,
    };
    response_result(request(
        &budget,
        ControlCommand::AgentLifecycle {
            request: lifecycle_request,
        },
    )?)
}
#[derive(Clone, PartialEq, Eq)]
struct Route {
    namespace: Uuid,
    surface: Uuid,
    surface_id: SurfaceId,
    session: u64,
    terminal: ProcessIdentity,
}

struct PreparedPlugin {
    installation: plugin::Installation,
    _lease: fs::File,
}

struct LaunchRequest<'a> {
    options: &'a ClientOptions,
    directory: &'a Path,
    route: Route,
    prepared: PreparedPlugin,
    notice_plugin: Option<&'a Path>,
    args: Vec<OsString>,
    held: bool,
    inbox_enabled: bool,
    automatic_root: Option<&'a Path>,
}

fn prepare_plugin(directory: &Path) -> Result<PreparedPlugin, CliError> {
    let installation = plugin::active(directory)?;
    if plugin::binary(&installation.provider.path)? != installation.provider
        || plugin::binary(&env::current_exe()?)? != installation.adapter
    {
        return Err(CliError(
            "provider or adapter executable changed; install a fresh owned generation".into(),
        ));
    }
    let lease = plugin::lease(directory, &installation)?;
    Ok(PreparedPlugin {
        installation,
        _lease: lease,
    })
}

enum LaunchFailure {
    BeforeStart(CliError),
    AfterStart(CliError),
}

impl LaunchFailure {
    fn into_error(self) -> CliError {
        match self {
            Self::BeforeStart(error) | Self::AfterStart(error) => error,
        }
    }
}

impl From<CliError> for LaunchFailure {
    fn from(error: CliError) -> Self {
        Self::AfterStart(error)
    }
}

impl From<io::Error> for LaunchFailure {
    fn from(error: io::Error) -> Self {
        Self::AfterStart(error.into())
    }
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
    let surface_id = item["surface"]["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .map(SurfaceId)
        .ok_or_else(|| CliError("missing exact surface identity".into()))?;
    let terminal_pid = item["session"]["published_local_child_pid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| CliError("missing terminal child identity".into()))?;
    let terminal = process_identity(terminal_pid)?;
    Ok(Route {
        namespace,
        surface,
        surface_id,
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

fn system_hook_shell(executable: &Path) -> bool {
    let supported_name = executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| matches!(name, "sh" | "dash" | "bash" | "zsh"));
    supported_name
        && matches!(
            executable.parent(),
            Some(parent) if parent == Path::new("/bin") || parent == Path::new("/usr/bin")
        )
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
    lifecycle: Option<LifecycleCapability>,
    cleanup_deadline: Option<Instant>,
    held: bool,
    disabled: bool,
    startup_warning_emitted: bool,
    prompts: HashSet<String>,
    inbox_enabled: bool,
}
struct LifecycleCapability {
    token: Uuid,
    sequence: u64,
    terminal: Option<(u64, LifecycleEvent)>,
}
impl Adapter<'_> {
    fn hook_client_parent(
        &self,
        client: &ProcessIdentity,
    ) -> Result<Option<ProcessIdentity>, CliError> {
        if client.executable != self.installation.adapter.path {
            return Err(CliError("hook client executable binding rejected".into()));
        }
        if client.parent_pid == self.provider.pid {
            return Ok(None);
        }
        let parent = process_identity(client.parent_pid)?;
        if parent.parent_pid != self.provider.pid
            || parent.uid != self.provider.uid
            || !system_hook_shell(&parent.executable)
        {
            return Err(CliError(
                "hook client must be a direct provider child or use its exact system shell".into(),
            ));
        }
        Ok(Some(parent))
    }

    fn hook_client_is_current(
        &self,
        client: &ProcessIdentity,
        parent_shell: Option<&ProcessIdentity>,
    ) -> bool {
        process_identity(client.pid).is_ok_and(|current| current == *client)
            && process_identity(self.provider.pid).is_ok_and(|current| current == self.provider)
            && parent_shell.is_none_or(|parent| {
                process_identity(parent.pid).is_ok_and(|current| current == *parent)
            })
    }

    fn retire(&mut self, deadline: Instant) -> Result<(), CliError> {
        self.disabled = true;
        let lifecycle_result = self.lifecycle_event(LifecycleEvent::EvidenceLost, None, deadline);
        let mut first_error = lifecycle_result.err();
        self.retire_inbox(deadline, &mut first_error);
        first_error.map_or(Ok(()), Err)
    }

    fn retire_for_session_end(
        &mut self,
        event: LifecycleEvent,
        deadline: Instant,
    ) -> Result<(), CliError> {
        self.disabled = true;
        let lifecycle_result = self.lifecycle_event(event, None, deadline);
        let mut first_error = lifecycle_result.err();
        self.retire_inbox(deadline, &mut first_error);
        first_error.map_or(Ok(()), Err)
    }

    fn retire_inbox(&mut self, deadline: Instant, first_error: &mut Option<CliError>) {
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
            match agent_messages::completed(self.options, deadline, request) {
                Ok(_) => {
                    self.registration = None;
                    self.retirement = None;
                }
                Err(error) if first_error.is_none() => *first_error = Some(error),
                Err(_) => (),
            }
        }
    }
    fn cleanup(&mut self, provider_exited: bool) -> Result<(), CliError> {
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
        if !provider_exited {
            return self.retire(deadline);
        }
        self.disabled = true;
        let lifecycle_result = self.lifecycle_event(LifecycleEvent::SessionEnded, None, deadline);
        let mut first_error = lifecycle_result.err();
        self.retire_inbox(deadline, &mut first_error);
        first_error.map_or(Ok(()), Err)
    }
    fn current(&self) -> bool {
        plugin::metadata_matches(&self.installation.provider)
            && plugin::metadata_matches(&self.installation.adapter)
            && plugin::active(self.directory)
                .is_ok_and(|i| i.generation == self.installation.generation)
    }

    fn warn_startup_rejection(&mut self, reason: &str) {
        if self.startup_warning_emitted {
            return;
        }
        self.startup_warning_emitted = true;
        eprintln!("zmux: local Claude lifecycle SessionStart ignored: {reason}");
    }

    fn register_lifecycle(&mut self, deadline: Instant) -> Result<(), CliError> {
        if self.lifecycle.is_some() {
            return Ok(());
        }
        let provider_session_id = self
            .root_session
            .parse::<Uuid>()
            .map_err(|_| CliError("invalid provider session identity".into()))?;
        let request = LifecycleRequest {
            request_id: Uuid::new_v4(),
            action: LifecycleAction::Register {
                target: LifecycleTarget {
                    identity_namespace: self.route.namespace,
                    surface_id: self.route.surface_id,
                    surface_uuid: self.route.surface,
                    session_id: SessionId(self.route.session),
                },
                provider: LifecycleProvider::ClaudeLocal,
                runtime_id: self.runtime,
                generation: self.generation,
                provider_session_id,
            },
        };
        let result = lifecycle_operation(self.options, deadline, request)?;
        if result["accepted"] != true {
            return Err(CliError(
                "desktop did not accept provider lifecycle registration".into(),
            ));
        }
        let token = result["capability"]
            .as_str()
            .and_then(|value| value.parse::<Uuid>().ok())
            .filter(|value| !value.is_nil())
            .ok_or_else(|| CliError("desktop returned no lifecycle capability".into()))?;
        self.lifecycle = Some(LifecycleCapability {
            token,
            sequence: 0,
            terminal: None,
        });
        Ok(())
    }

    fn lifecycle_event(
        &mut self,
        event: LifecycleEvent,
        hook_id: Option<Uuid>,
        deadline: Instant,
    ) -> Result<HookDisposition, CliError> {
        let Some(capability) = self.lifecycle.as_mut() else {
            return Ok(HookDisposition::NotHandled);
        };
        let is_terminal = matches!(
            &event,
            LifecycleEvent::SessionEnded | LifecycleEvent::EvidenceLost
        );
        let (sequence, event) = if let Some((sequence, terminal_event)) = &capability.terminal {
            if !is_terminal {
                return Err(CliError("lifecycle capability is already terminal".into()));
            }
            (*sequence, terminal_event.clone())
        } else if is_terminal {
            capability.sequence = capability
                .sequence
                .checked_add(1)
                .ok_or_else(|| CliError("lifecycle event sequence exhausted".into()))?;
            let terminal = (capability.sequence, event);
            capability.terminal = Some(terminal.clone());
            terminal
        } else {
            capability.sequence = capability
                .sequence
                .checked_add(1)
                .ok_or_else(|| CliError("lifecycle event sequence exhausted".into()))?;
            (capability.sequence, event)
        };
        let request = LifecycleRequest {
            request_id: Uuid::new_v4(),
            action: LifecycleAction::Event {
                capability: capability.token,
                runtime_id: self.runtime,
                generation: self.generation,
                sequence,
                hook_id,
                event,
            },
        };
        let result = lifecycle_operation(self.options, deadline, request)?;
        if result["accepted"] != true {
            return Err(CliError(
                "desktop did not accept provider lifecycle event".into(),
            ));
        }
        let disposition = match result["hook_disposition"].as_str() {
            Some("alert_covered") => HookDisposition::AlertCovered,
            Some("handled_no_alert") => HookDisposition::HandledNoAlert,
            Some("not_handled") => HookDisposition::NotHandled,
            _ => return Err(CliError("desktop returned an invalid hook receipt".into())),
        };
        if is_terminal {
            self.lifecycle = None;
        }
        Ok(disposition)
    }

    fn try_lifecycle_event(
        &mut self,
        event: &ClaudeHookEvent,
        hook_id: Uuid,
        deadline: Instant,
    ) -> Result<HookDisposition, CliError> {
        let result = match event {
            ClaudeHookEvent::SessionEnd { .. } => Ok(HookDisposition::NotHandled),
            _ => {
                // Register is exact-identity idempotent. If the bounded startup
                // request timed out, a later matching hook can recover the cap.
                self.register_lifecycle(deadline).and_then(|()| {
                    event
                        .lifecycle_event()
                        .map_or(Ok(HookDisposition::NotHandled), |event| {
                            self.lifecycle_event(event, Some(hook_id), deadline)
                        })
                })
            }
        };
        if let Err(error) = &result {
            if matches!(event, ClaudeHookEvent::SessionStart { .. }) {
                self.warn_startup_rejection(&format!("lifecycle admission failed: {error}"));
            } else {
                eprintln!(
                    "zmux: local Claude lifecycle evidence unavailable; status may be unknown: {error}"
                );
            }
        }
        result
    }

    fn hook(
        &mut self,
        event: &ClaudeHookEvent,
        hook_id: Uuid,
        deadline: Instant,
    ) -> Result<(Value, Option<Uuid>, HookDisposition), CliError> {
        let noop = || (json!({}), None, HookDisposition::NotHandled);
        let startup = matches!(event, ClaudeHookEvent::SessionStart { .. });
        if self.disabled {
            if startup && event.is_main_session() {
                self.warn_startup_rejection("adapter is disabled");
            }
            return Ok(noop());
        }
        if !self.current() {
            if startup && event.is_main_session() {
                self.warn_startup_rejection("owned plugin generation is no longer current");
            }
            return Ok(noop());
        }
        if event.session_id() != self.root_session {
            if startup && event.is_main_session() {
                self.warn_startup_rejection("provider session does not match this adapter run");
            }
            return Ok(noop());
        }
        if !event.is_main_session() {
            return Ok(noop());
        }
        if route(self.options, self.route.surface, deadline)? != self.route {
            if startup {
                self.warn_startup_rejection("exact terminal route changed during startup");
            }
            self.retire(deadline)?;
            return Ok(noop());
        }
        let disposition = self
            .try_lifecycle_event(event, hook_id, deadline)
            .unwrap_or(HookDisposition::NotHandled);
        match event {
            ClaudeHookEvent::SessionStart { source, .. } => {
                if !self.inbox_enabled {
                    return Ok(noop());
                }
                if source != "startup" || self.registration.is_some() {
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
            ClaudeHookEvent::SessionEnd { .. } => {
                let terminal = event
                    .lifecycle_event()
                    .unwrap_or(LifecycleEvent::EvidenceLost);
                self.retire_for_session_end(terminal, deadline)?;
                Ok(noop())
            }
            ClaudeHookEvent::Notification { .. } => Ok((json!({}), None, disposition)),
            ClaudeHookEvent::PermissionRequest { .. } | ClaudeHookEvent::Elicitation { .. } => {
                self.held = true;
                self.hold(deadline)?;
                Ok(noop())
            }
            ClaudeHookEvent::PreToolUse { tool_name, .. }
                if matches!(tool_name.as_str(), "AskUserQuestion" | "EnterPlanMode") =>
            {
                self.held = true;
                self.hold(deadline)?;
                Ok(noop())
            }
            ClaudeHookEvent::UserPromptSubmit {
                prompt_id,
                permission_mode,
                ..
            } => {
                if !self.inbox_enabled {
                    return Ok(noop());
                }
                if permission_mode == "plan" {
                    self.held = true;
                    self.hold(deadline)?;
                }
                if self.held || !matches!(permission_mode.as_str(), "default" | "acceptEdits") {
                    return Ok(noop());
                }
                let prompt = prompt_id.to_string();
                let prompt = prompt.as_str();
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
                    disposition,
                ))
            }
            // Stop updates provider lifecycle state separately. It does not claim,
            // continue, Ack or ReadTurn, and is not proof that context was accepted.
            ClaudeHookEvent::Stop { .. } => Ok((json!({}), None, disposition)),
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
        // The lifecycle plugin runs as an exec-form direct child. The session
        // notice plugin is a generated shell conditional whose exact system
        // shell is a direct provider child. Neither path authenticates Claude.
        let parent_shell = self.hook_client_parent(&client)?;
        if !self.hook_client_is_current(&client, parent_shell.as_ref()) {
            return Err(CliError("hook process binding changed".into()));
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
        let event_name = frame["event"]
            .as_str()
            .filter(|e| events().contains(e))
            .ok_or_else(|| CliError("unknown hook event".into()))?;
        let input = frame
            .get("input")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| CliError("invalid hook input".into()))?;
        let hook_id = frame["hook_id"]
            .as_str()
            .and_then(|value| value.parse::<Uuid>().ok())
            .filter(|hook_id| !hook_id.is_nil())
            .ok_or_else(|| CliError("missing Zmux hook invocation ID".into()))?;
        let event: ClaudeHookEvent = serde_json::from_value(input.clone())
            .map_err(|_| CliError("invalid typed Claude hook input".into()))?;
        if event.name() != event_name {
            return Err(CliError(
                "hook event name does not match its typed input".into(),
            ));
        }
        if !self.hook_client_is_current(&client, parent_shell.as_ref()) {
            return Err(CliError("hook process changed before admission".into()));
        }
        let (output, attempt, hook_disposition) = self.hook(&event, hook_id, deadline)?;
        if !self.hook_client_is_current(&client, parent_shell.as_ref()) {
            return Err(CliError("hook process changed before response".into()));
        }
        send_frame(
            &mut stream,
            &json!({"output":output,"attempt_id":attempt,"hook_disposition":hook_disposition}),
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
        if self.cleanup(false).is_err() {
            eprintln!("zmux: adapter cleanup could not confirm lifecycle or inbox retirement");
        }
    }
}

fn launch(
    options: &ClientOptions,
    directory: &Path,
    surface: Uuid,
    notice_plugin: Option<&Path>,
    args: Vec<OsString>,
    held: bool,
) -> Result<u8, CliError> {
    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError(
            "Claude adapter run requires an interactive terminal; headless is unsupported".into(),
        ));
    }
    let route = route(options, surface, Instant::now() + options.timeout)?;
    check_terminal_ancestor(&route)?;
    let prepared = prepare_plugin(directory)?;
    launch_prepared(LaunchRequest {
        options,
        directory,
        route,
        prepared,
        notice_plugin,
        args,
        held,
        inbox_enabled: true,
        automatic_root: None,
    })
    .map_err(LaunchFailure::into_error)
}

fn check_terminal_ancestor(route: &Route) -> Result<(), CliError> {
    let mut ancestor = process_identity(std::process::id())?;
    for _ in 0..32 {
        if ancestor == route.terminal {
            return Ok(());
        }
        if ancestor.parent_pid <= 1 {
            break;
        }
        ancestor = process_identity(ancestor.parent_pid)?;
    }
    Err(CliError(
        "wrapper is not running inside the exact Desktop terminal process lifetime".into(),
    ))
}

fn provider_arguments(
    lifecycle_plugin: &Path,
    notice_plugin: Option<&Path>,
    root_session: &str,
    user_arguments: Vec<OsString>,
) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--plugin-dir"),
        lifecycle_plugin.as_os_str().to_owned(),
    ];
    if let Some(notice_plugin) = notice_plugin {
        arguments.extend([
            OsString::from("--plugin-dir"),
            notice_plugin.as_os_str().to_owned(),
        ]);
    }
    arguments.extend([OsString::from("--session-id"), OsString::from(root_session)]);
    arguments.extend(user_arguments);
    arguments
}

fn launch_prepared(request: LaunchRequest<'_>) -> Result<u8, LaunchFailure> {
    let LaunchRequest {
        options,
        directory,
        route,
        prepared,
        notice_plugin,
        args,
        held,
        inbox_enabled,
        automatic_root,
    } = request;
    check_terminal_ancestor(&route).map_err(LaunchFailure::BeforeStart)?;
    if let Some(root) = automatic_root {
        let expected_notice_plugin = root.join("plugin");
        if notice_plugin != Some(expected_notice_plugin.as_path()) {
            return Err(LaunchFailure::BeforeStart(CliError(
                "automatic launch notice plugin is outside its exact session root".into(),
            )));
        }
        plugin::Directory::open(root, false).map_err(LaunchFailure::BeforeStart)?;
        plugin::Directory::open(&expected_notice_plugin, false)
            .map_err(LaunchFailure::BeforeStart)?;
    }
    let PreparedPlugin {
        installation: installed,
        _lease,
    } = prepared;
    let root_session = Uuid::new_v4().to_string();
    let runtime = Uuid::new_v4();
    // Darwin permits at most 103 pathname bytes. TMPDIR commonly lives under
    // a long per-user /var/folders path; choose a bounded path before creating
    // anything. The random directory remains private even in shared /tmp.
    let name = format!("zmux-hook-{}", runtime.simple());
    let mut path = env::temp_dir()
        .canonicalize()
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?
        .join(&name);
    if path.join("event.sock").as_os_str().as_bytes().len() > 103 {
        path = Path::new("/tmp").canonicalize()?.join(&name);
    }
    if path.join("event.sock").as_os_str().as_bytes().len() > 103 {
        return Err(LaunchFailure::BeforeStart(CliError(
            "hook socket path exceeds the Unix pathname limit".into(),
        )));
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
        .map_err(io::Error::from)
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?,
    );
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
    let owned_directory =
        plugin::Directory::open(&path, false).map_err(LaunchFailure::BeforeStart)?;
    let socket_path = path.join("event.sock");
    let mut socket_dir = SocketDirectory {
        path,
        directory: owned_directory,
        parent,
        socket: None,
    };
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
    let metadata = fs::symlink_metadata(&socket_path)
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
    socket_dir.socket = Some((metadata.dev(), metadata.ino()));
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
    let signals = Signals::new().map_err(LaunchFailure::BeforeStart)?;
    let mut command = Command::new(&installed.provider.path);
    let notice_plugin = notice_plugin.filter(|path| {
        path.is_absolute() && fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
    });
    command.args(provider_arguments(
        &directory.join(installed.generation.to_string()),
        notice_plugin,
        &root_session,
        args,
    ));
    for (name, _) in env::vars_os() {
        if name.to_string_lossy().starts_with("ZMUX_") {
            command.env_remove(name);
        }
    }
    command.env(ROUTE_ENV, &socket_path);
    if let Some(root) = automatic_root {
        // The session-owned OSC notice plugin keeps attention and completion
        // visible even when lifecycle transport cannot admit a particular event.
        // Restore no caller identity, credential, or unrelated ZMUX variables.
        command.env("ZMUX_SHELL_INTEGRATION_DIRECTORY", root);
        command.env("ZMUX_CLI_PATH", &installed.adapter.path);
        command.env("ZMUX_CLAUDE_LIFECYCLE_ALERTS_ACTIVE", "1");
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child: Child = command
        .spawn()
        .map_err(|error| LaunchFailure::BeforeStart(error.into()))?;
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
    // The spawn outcome is now known and the runtime lease is still held.
    // Clear the uncertainty marker so concurrent session cleanup can prove
    // retention through the generation lease instead of retaining forever.
    if automatic_root.is_some() {
        clear_launch_marker(directory).map_err(LaunchFailure::AfterStart)?;
    }
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
        lifecycle: None,
        cleanup_deadline: None,
        held,
        disabled: false,
        startup_warning_emitted: false,
        prompts: HashSet::new(),
        inbox_enabled,
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
                && adapter.cleanup(false).is_err()
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
                        if adapter.cleanup(false).is_err() {
                            eprintln!("zmux: hook poll retirement unconfirmed");
                        }
                        eprintln!("zmux: hook listener poll failed; provider retained: {error}");
                        break child.wait()?;
                    }
                }
            }
            Err(e) => {
                if adapter.cleanup(false).is_err() {
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
    if adapter.cleanup(true).is_err() {
        eprintln!("zmux: provider exited; exact adapter retirement unconfirmed");
    }
    Ok(exit_code(status))
}
fn exit_code(status: std::process::ExitStatus) -> u8 {
    status.code().unwrap_or(1).clamp(0, 255) as u8
}
