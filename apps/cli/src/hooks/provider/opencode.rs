//! Invocation-scoped OpenCode plugin setup and normalized local observations.

use super::*;
use std::{collections::VecDeque, os::unix::fs::symlink};
use zmux_remote_runtime::agent_lifecycle::{ProviderKind, ProviderLifecycleObservation};

const MAX_SESSIONS_PER_INVOCATION: usize = 128;
const MAX_CONFIG_ENTRIES: usize = 256;
const MAX_USER_PLUGINS: usize = 128;

pub(super) struct Launch {
    pub(super) args: Vec<OsString>,
    pub(super) config_dir: PathBuf,
    pub(super) expected_session: Option<String>,
}

pub(super) fn prepare_launch(
    executable: &Path,
    args: &[OsString],
    runtime_dir: &Path,
) -> Result<Launch, CliError> {
    if !interactive(args) {
        return Err(CliError(
            "OpenCode lifecycle tracking is available for interactive TUI launches".into(),
        ));
    }
    standalone_supported(executable)?;
    let config_dir = runtime_dir.join("opencode-config");
    fs::DirBuilder::new().mode(0o700).create(&config_dir)?;
    let plugins_dir = config_dir.join("plugins");
    fs::DirBuilder::new().mode(0o700).create(&plugins_dir)?;

    if let Some(existing) = env::var_os("OPENCODE_CONFIG_DIR") {
        mirror_config_directory(Path::new(&existing), &config_dir, &plugins_dir)?;
    }

    let plugin_name = format!("zmux-provider-{}.js", Uuid::new_v4().simple());
    let plugin_path = plugins_dir.join(plugin_name);
    fs::write(
        &plugin_path,
        include_str!("../../../../../crates/session-runtime/src/shell/provider-opencode-plugin.js"),
    )?;
    fs::set_permissions(&plugin_path, fs::Permissions::from_mode(0o600))?;

    let expected_session = expected_session(args)?;
    let mut launch_args = Vec::with_capacity(args.len() + 1);
    if !args.iter().any(|argument| argument == "--standalone") {
        launch_args.push(OsString::from("--standalone"));
    }
    launch_args.extend_from_slice(args);
    Ok(Launch {
        args: launch_args,
        config_dir,
        expected_session,
    })
}

fn standalone_supported(executable: &Path) -> Result<(), CliError> {
    let cancellation = crate::bounded_command::Cancellation::new()?;
    let mut command = Command::new(executable);
    command.arg("--help");
    let output = crate::bounded_command::capture(
        &mut command,
        Instant::now() + Duration::from_secs(2),
        &cancellation,
    )?;
    if output.status != zmux_session_runtime::child::ExitObservation::Exited(0) {
        return Err(CliError(
            "OpenCode --help preflight did not complete successfully".into(),
        ));
    }
    let help = format!("{}\n{}", output.stdout, output.stderr);
    let supported = help.lines().any(|line| {
        let Some(suffix) = line.trim_start().strip_prefix("--standalone") else {
            return false;
        };
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| matches!(character, ' ' | '\t' | '=' | '<'))
    });
    if supported {
        Ok(())
    } else {
        Err(CliError(
            "installed OpenCode does not advertise --standalone".into(),
        ))
    }
}

fn mirror_config_directory(
    original: &Path,
    destination: &Path,
    destination_plugins: &Path,
) -> Result<(), CliError> {
    if !original.is_absolute() || !fs::metadata(original).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(CliError(
            "existing OPENCODE_CONFIG_DIR is not an accessible directory".into(),
        ));
    }
    for (index, entry) in fs::read_dir(original)?.enumerate() {
        if index >= MAX_CONFIG_ENTRIES {
            return Err(CliError(
                "existing OpenCode config directory exceeds its entry bound".into(),
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        if name == "plugins" {
            let source_plugins = entry.path();
            if !fs::metadata(&source_plugins).is_ok_and(|metadata| metadata.is_dir()) {
                return Err(CliError(
                    "existing OpenCode plugins path is not a directory".into(),
                ));
            }
            for (index, plugin) in fs::read_dir(source_plugins)?.enumerate() {
                if index >= MAX_USER_PLUGINS {
                    return Err(CliError(
                        "existing OpenCode plugin directory exceeds its entry bound".into(),
                    ));
                }
                let plugin = plugin?;
                symlink(plugin.path(), destination_plugins.join(plugin.file_name()))?;
            }
        } else {
            symlink(entry.path(), destination.join(name))?;
        }
    }
    Ok(())
}

fn interactive(args: &[OsString]) -> bool {
    let options = args
        .iter()
        .take_while(|argument| argument.as_os_str() != "--");
    if options
        .filter_map(|argument| argument.to_str())
        .any(|argument| matches!(argument, "--help" | "-h" | "--version" | "-v"))
    {
        return false;
    }
    if args
        .iter()
        .filter_map(|argument| argument.to_str())
        .any(|argument| argument == "--server" || argument.starts_with("--server="))
    {
        return false;
    }
    match first_positional(args) {
        Ok(None) => true,
        Err(()) => false,
        Ok(Some(command)) => !matches!(
            command,
            "run"
                | "serve"
                | "acp"
                | "api"
                | "auth"
                | "mcp"
                | "models"
                | "plugin"
                | "session"
                | "service"
                | "debug"
                | "upgrade"
                | "update"
                | "uninstall"
                | "reload"
                | "pair"
                | "stats"
        ),
    }
}

fn first_positional(args: &[OsString]) -> Result<Option<&str>, ()> {
    const VALUE_OPTIONS: &[&str] = &[
        "--session",
        "-s",
        "--prompt",
        "--server",
        "--log-level",
        "--completions",
    ];
    const BOOLEAN_OPTIONS: &[&str] = &[
        "--standalone",
        "--auto",
        "--continue",
        "-c",
        "--print-logs",
        "--wizard",
        "--help",
        "-h",
        "--version",
        "-v",
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
            // Do not inject a plugin into a CLI mode we have not reviewed.
            return Err(());
        }
        index += 1;
    }
    Ok(None)
}

fn expected_session(args: &[OsString]) -> Result<Option<String>, CliError> {
    let mut expected = None;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index]
            .to_str()
            .ok_or_else(|| CliError("OpenCode arguments are not valid UTF-8".into()))?;
        let value = if matches!(argument, "--session" | "-s") {
            index += 1;
            Some(
                args.get(index)
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        CliError("OpenCode session option is missing its opaque ID".into())
                    })?,
            )
        } else {
            argument
                .strip_prefix("--session=")
                .or_else(|| argument.strip_prefix("-s="))
        };
        if let Some(value) = value {
            if expected.is_some() || !valid_id(value) {
                return Err(CliError(
                    "OpenCode session option is ambiguous or invalid; tracking was skipped".into(),
                ));
            }
            expected = Some(value.to_owned());
        }
        index += 1;
    }
    Ok(expected)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

#[derive(Default)]
struct SessionSequence {
    next_sequence: u64,
    ended: bool,
}

pub(super) struct ObservationOwner {
    route: Route,
    runtime: Uuid,
    generation: Uuid,
    sessions: VecDeque<(String, SessionSequence)>,
    lost: bool,
}

impl ObservationOwner {
    pub(super) fn new(
        _options: &ClientOptions,
        route: &Route,
        runtime: Uuid,
        generation: Uuid,
    ) -> Self {
        Self {
            route: route.clone(),
            runtime,
            generation,
            sessions: VecDeque::new(),
            lost: false,
        }
    }

    pub(super) fn observe(
        &mut self,
        observation: ProviderLifecycleObservation,
        options: &ClientOptions,
    ) -> Result<(), CliError> {
        if self.lost {
            return Err(CliError(
                "OpenCode lifecycle event arrived after evidence loss".into(),
            ));
        }
        if observation.provider != ProviderKind::OpenCode
            || observation.scope != zmux_remote_runtime::agent_lifecycle::ProviderScope::Root
        {
            return Err(CliError(
                "OpenCode emitted a different provider kind".into(),
            ));
        }
        observation
            .validate()
            .map_err(|_| CliError("OpenCode observation failed contract validation".into()))?;
        let index = if let Some(index) = self
            .sessions
            .iter()
            .position(|(session_id, _)| session_id == &observation.session_id)
        {
            index
        } else {
            if self.sessions.len() >= MAX_SESSIONS_PER_INVOCATION {
                return Err(CliError(
                    "OpenCode session history reached its bound".into(),
                ));
            }
            self.sessions
                .push_back((observation.session_id.clone(), SessionSequence::default()));
            self.sessions.len() - 1
        };
        if self.sessions[index].1.ended {
            return Err(CliError(
                "OpenCode lifecycle event arrived after session end".into(),
            ));
        }
        let sequence = &mut self.sessions[index].1.next_sequence;
        *sequence = sequence
            .checked_add(1)
            .ok_or_else(|| CliError("OpenCode lifecycle sequence exhausted".into()))?;
        if let Err(error) = super::submit_observation(
            options,
            &self.route,
            self.runtime,
            self.generation,
            *sequence,
            observation,
        ) {
            self.lost = true;
            let _ = self.end_sessions(options, true);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub(super) fn evidence_lost(&mut self, options: &ClientOptions) -> Result<(), CliError> {
        if self.lost {
            return Ok(());
        }
        self.lost = true;
        self.end_sessions(options, true)
    }

    pub(super) fn session_ended_all(&mut self, options: &ClientOptions) -> Result<(), CliError> {
        self.end_sessions(options, false)
    }

    fn end_sessions(&mut self, options: &ClientOptions, lost: bool) -> Result<(), CliError> {
        let mut first_error = None;
        for (session_id, state) in &mut self.sessions {
            if state.ended {
                continue;
            }
            state.next_sequence = state
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| CliError("OpenCode lifecycle sequence exhausted".into()))?;
            if let Err(error) = super::submit_session_end(
                options,
                &self.route,
                super::ProviderSessionEnd {
                    provider: ProviderKind::OpenCode,
                    session_id: session_id.clone(),
                    runtime: self.runtime,
                    generation: self.generation,
                    sequence: state.next_sequence,
                    lost,
                },
            ) {
                first_error.get_or_insert(error);
            } else if !lost {
                state.ended = true;
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_modes_preserve_noninteractive_invocations() {
        assert!(!interactive(&["run".into(), "prompt".into()]));
        assert!(!interactive(&["--help".into()]));
        assert!(interactive(&["--continue".into()]));
        assert!(interactive(&[]));
    }

    #[test]
    fn baseline_and_live_snapshot_remain_distinct_from_outcomes() {
        let baseline = serde_json::from_value::<ProviderLifecycleObservation>(json!({
            "provider":"opencode","scope":"root","session_id":"ses_opaque",
            "turn_id":"msg_opaque","state":"working","transition":"snapshot","marker":"baseline"
        }))
        .unwrap();
        assert!(baseline.validate().is_ok());
        assert_eq!(
            baseline.marker,
            zmux_remote_runtime::agent_lifecycle::ProviderMarker::Baseline
        );
        assert!(valid_id("ses_opaque"));
        assert!(!valid_id("bad\nid"));
    }

    #[test]
    fn custom_config_and_user_plugins_are_merged_into_the_invocation_directory() {
        let root = env::temp_dir().join(format!("zmux-opencode-config-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("create private OpenCode config fixture");
        let original = root.join("user-config");
        fs::create_dir(&original).expect("create existing user config directory");
        fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            original.join("opencode.json"),
            b"{\"theme\":\"user-owned\"}",
        )
        .unwrap();
        let original_plugins = original.join("plugins");
        fs::create_dir(&original_plugins).unwrap();
        fs::write(
            original_plugins.join("user-plugin.js"),
            b"export default () => {}; ",
        )
        .unwrap();

        let destination = root.join("invocation-config");
        let destination_plugins = destination.join("plugins");
        fs::create_dir_all(&destination_plugins).unwrap();
        let zmux_plugin = destination_plugins.join("zmux-provider-opaque.js");
        fs::write(&zmux_plugin, b"invocation-only provider observer").unwrap();
        mirror_config_directory(&original, &destination, &destination_plugins)
            .expect("merge user config without copying or replacing it");

        let config_link = fs::symlink_metadata(destination.join("opencode.json")).unwrap();
        assert!(config_link.file_type().is_symlink());
        assert_eq!(
            fs::read(destination.join("opencode.json")).unwrap(),
            b"{\"theme\":\"user-owned\"}"
        );
        let user_plugin = destination_plugins.join("user-plugin.js");
        assert!(
            fs::symlink_metadata(&user_plugin)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read(&user_plugin).unwrap(),
            b"export default () => {}; "
        );
        assert_eq!(
            fs::read(&zmux_plugin).unwrap(),
            b"invocation-only provider observer"
        );
        assert_eq!(
            fs::read(original.join("opencode.json")).unwrap(),
            b"{\"theme\":\"user-owned\"}"
        );
        assert_eq!(
            fs::read(original_plugins.join("user-plugin.js")).unwrap(),
            b"export default () => {}; "
        );
        fs::remove_dir_all(root).unwrap();
    }
}
