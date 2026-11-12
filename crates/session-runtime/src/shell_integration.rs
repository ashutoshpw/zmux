//! Session-only shell/provider integration, shared by local and SSH launches.
//! Never edits user startup files, provider settings or global hooks.
use crate::Launch;
use data_encoding::BASE64;
use flate2::{Compression, write::GzEncoder};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use zmux_terminal_adapter::ProgramOpenRequest;

static NEXT: AtomicU64 = AtomicU64::new(0);
const MAX_BOOTSTRAP_BYTES: usize = 256 * 1024;
const MAX_BOOTSTRAP_BASE64_BYTES: usize = 48 * 1024;
static BOOTSTRAP_PAYLOADS: [OnceLock<Result<String, String>>; 4] = [const { OnceLock::new() }; 4];
const FILES: &[&str] = &[
    "claude.sh",
    "bashrc",
    "claude.zsh",
    "claude.fish",
    "provider.sh",
    "provider.fish",
    "provider-launch.py",
    "provider-opencode-plugin.js",
    "claude-lifecycle-hook.py",
    "claude-lifecycle-notice.py",
    "claude-lifecycle-stream.py",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    "env",
    "attention",
    "agent-events",
    "agent-events-loss",
    "agent-lifecycle-generation",
    "claude-auto.lock",
    "agent-lifecycle-active.lock",
    "plugin/.claude-plugin/plugin.json",
    "plugin/hooks/hooks.json",
    "bin/zmux-open-url",
    "bin/open",
    "bin/xdg-open",
    "bin/sensible-browser",
];
const DIRECTORIES: &[&str] = &["plugin/.claude-plugin", "plugin/hooks", "plugin", "bin"];

/// Fresh per-PTY correlation token. It is deliberately neither printable via
/// Debug nor serializable; only the local shell bootstrap and SSH command encode it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenRequestNonce([u8; 16]);

impl OpenRequestNonce {
    /// Generate on a launch worker, never on the UI/core executor.
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().into_bytes())
    }

    pub(crate) fn protocol_value(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = String::with_capacity(32);
        for byte in self.0 {
            value.push(HEX[(byte >> 4) as usize] as char);
            value.push(HEX[(byte & 0x0f) as usize] as char);
        }
        value
    }

    pub(crate) fn adapter_value(self) -> [u8; 16] {
        self.0
    }

    /// Private deterministic directory used only by the managed SSH bootstrap.
    /// The token is correlation, not authentication; the transport is the
    /// existing private OpenSSH ControlMaster.
    pub fn remote_integration_directory(&self) -> String {
        format!("/tmp/zmux-shell-{}", self.protocol_value())
    }

    pub fn remote_lifecycle_token(&self) -> String {
        self.protocol_value()
    }

    pub(crate) fn matches_request(&self, request: &ProgramOpenRequest) -> bool {
        request.matches_nonce(&self.0)
    }
}

/// Whether a request originated in a local shell or the managed SSH shell.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OpenRequestSource {
    Local,
    ManagedSsh,
}

/// Private, in-memory owner binding for one shell integration lease.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenRequestRegistration {
    nonce: OpenRequestNonce,
    source: OpenRequestSource,
}

impl OpenRequestRegistration {
    pub fn new(nonce: OpenRequestNonce, source: OpenRequestSource) -> Self {
        Self { nonce, source }
    }

    pub fn nonce(self) -> OpenRequestNonce {
        self.nonce
    }

    pub fn source(self) -> OpenRequestSource {
        self.source
    }
}

/// Accepted URL-only notice. It never enters a serialized snapshot or log value.
#[derive(Clone, PartialEq, Eq)]
pub struct OpenUrl(String);

impl OpenUrl {
    pub(crate) fn new(url: String) -> Self {
        Self(url)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Owned by the actual session worker; cleanup never recursively removes unknown
/// files and also runs when the shell is killed without executing its EXIT trap.
pub struct ShellIntegrationLease(PathBuf);
impl Drop for ShellIntegrationLease {
    fn drop(&mut self) {
        for name in FILES {
            if let Err(error) = fs::remove_file(self.0.join(name))
                && error.kind() != io::ErrorKind::NotFound
            {
                eprintln!("zmux shell integration file cleanup: {error}");
            }
        }
        for path in DIRECTORIES
            .iter()
            .map(|p| self.0.join(p))
            .chain([self.0.clone()])
        {
            if let Err(error) = fs::remove_dir(path)
                && error.kind() != io::ErrorKind::NotFound
            {
                eprintln!("zmux shell integration directory retained: {error}");
            }
        }
    }
}

/// Recognized interactive shells only. Commands, scripts and application PTYs
/// retain their exact argv. Run on the launch worker, before child creation.
pub fn prepare(launch: Launch) -> io::Result<(Launch, Option<ShellIntegrationLease>)> {
    prepare_inner(launch, false)
}

/// Install the opener helper into an eligible interactive shell. The nonce is
/// injected by Session from the matching private registration, not placed in argv.
pub fn prepare_with_nonce(
    launch: Launch,
    _nonce: &OpenRequestNonce,
) -> io::Result<(Launch, Option<ShellIntegrationLease>)> {
    prepare_inner(launch, true)
}

fn prepare_inner(
    mut launch: Launch,
    install_openers: bool,
) -> io::Result<(Launch, Option<ShellIntegrationLease>)> {
    let shell = Path::new(&launch.program)
        .file_name()
        .and_then(|n| n.to_str());
    if !matches!(shell, Some("bash" | "zsh" | "sh" | "dash" | "fish"))
        || !launch.args.iter().any(|a| {
            matches!(
                a.as_str(),
                "-i" | "-l" | "-il" | "-li" | "--login" | "--interactive"
            )
        })
        || launch.args.iter().any(|a| a == "-c" || a.starts_with("-c"))
        || std::env::var_os("ZMUX_SHELL_INTEGRATION_DISABLED").as_deref()
            == Some(std::ffi::OsStr::new("1"))
    {
        return Ok((launch, None));
    }
    let (bash_rc_mode, bash_rc_path) = if shell == Some("bash") {
        prepare_bash_rcfile(&mut launch.args)
    } else {
        ("default".to_owned(), String::new())
    };
    let temporary_directory = std::env::temp_dir().canonicalize()?;
    let mut directory = None;
    for _ in 0..128 {
        let path = temporary_directory.join(format!(
            "zmux-shell-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                directory = Some(path);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let lease =
        ShellIntegrationLease(directory.ok_or_else(|| {
            io::Error::other("cannot allocate private shell integration directory")
        })?);
    let root = lease
        .0
        .to_str()
        .ok_or_else(|| io::Error::other("shell integration path is not UTF-8"))?
        .to_string();
    let payload = bootstrap_payload(install_openers, false)?;
    let mut args = vec![
        "-c".into(),
        bootstrap_decoder(payload),
        "zmux-shell".into(),
        root,
        bash_rc_mode,
        bash_rc_path,
        launch.program,
    ];
    args.extend(launch.args);
    launch.program = "/bin/sh".into();
    launch.args = args;
    Ok((launch, Some(lease)))
}

fn prepare_bash_rcfile(args: &mut Vec<String>) -> (String, String) {
    let mut mode = "default";
    let mut path = String::new();
    let mut filtered = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--norc" {
            mode = "none";
            index += 1;
        } else if argument == "--rcfile" || argument == "--init-file" {
            if let Some(value) = args.get(index + 1) {
                mode = if value.is_empty() { "none" } else { "file" };
                path.clear();
                path.push_str(value);
                index += 2;
            } else {
                filtered.push(argument.clone());
                index += 1;
            }
        } else if let Some(value) = argument
            .strip_prefix("--rcfile=")
            .or_else(|| argument.strip_prefix("--init-file="))
        {
            mode = if value.is_empty() { "none" } else { "file" };
            path.clear();
            path.push_str(value);
            index += 1;
        } else {
            filtered.push(argument.clone());
            index += 1;
        }
    }
    *args = filtered;
    (mode.to_owned(), path)
}

/// Original remote shell code, carried in the already authenticated PTY. No
/// local socket/password is forwarded; OSC notices are bound to that exact PTY.
pub fn remote_command(nonce: &OpenRequestNonce) -> String {
    let payload = match bootstrap_payload(true, true) {
        Ok(payload) => payload,
        Err(_) => {
            return "printf '%s\\n' 'zmux: managed shell integration bootstrap exceeds its configured size limit' >&2; exit 70".into();
        }
    };
    let script = bootstrap_decoder(payload).replace('\'', "'\\''");
    let token = nonce.remote_lifecycle_token();
    let root = nonce.remote_integration_directory();
    let shell = ["$", "{SHELL:-/bin/sh}"].concat();
    format!(
        "ZMUX_OPEN_URL_NONCE={token} ZMUX_AGENT_LIFECYCLE_NONCE={token} ZMUX_REMOTE_LIFECYCLE=1 /bin/sh -c '{script}' zmux-shell '{root}' default '' \"{shell}\" -l",
    )
}

fn bootstrap_payload(
    install_openers: bool,
    install_remote_lifecycle: bool,
) -> io::Result<&'static str> {
    let cache_index = usize::from(install_openers) | (usize::from(install_remote_lifecycle) << 1);
    let result = BOOTSTRAP_PAYLOADS[cache_index].get_or_init(|| {
        let bootstrap = bootstrap_script(install_openers, install_remote_lifecycle);
        if bootstrap.len() > MAX_BOOTSTRAP_BYTES {
            return Err(format!(
                "expanded shell bootstrap is {} bytes; limit is {MAX_BOOTSTRAP_BYTES}",
                bootstrap.len()
            ));
        }
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(bootstrap.as_bytes())
            .map_err(|error| error.to_string())?;
        let compressed = encoder.finish().map_err(|error| error.to_string())?;
        let encoded = BASE64.encode(&compressed);
        if encoded.len() > MAX_BOOTSTRAP_BASE64_BYTES {
            return Err(format!(
                "compressed shell bootstrap is {} bytes; limit is {MAX_BOOTSTRAP_BASE64_BYTES}",
                encoded.len()
            ));
        }
        Ok(encoded)
    });
    result
        .as_deref()
        .map_err(|message| io::Error::other(message.clone()))
}

/// Decode the trusted compressed payload into a shell variable, retaining
/// trailing newlines with a sentinel before invoking the shell builtin `eval`.
/// The fallback path preserves the user's shell argv when base64 or gzip is
/// unavailable on the target host.
fn bootstrap_decoder(payload: &str) -> String {
    let decoder = r#"zmux_bootstrap_payload='@PAYLOAD@'
if ! command -v base64 >/dev/null 2>&1 || ! command -v gzip >/dev/null 2>&1; then
    printf '%s\n' 'zmux: shell integration requires the base64 and gzip utilities' >&2
    shift 3
    exec "$@"
fi
if printf '%s\n' 'WA==' | base64 -d >/dev/null 2>&1; then
    zmux_base64_flag=-d
elif printf '%s\n' 'WA==' | base64 -D >/dev/null 2>&1; then
    zmux_base64_flag=-D
else
    printf '%s\n' 'zmux: shell integration needs a compatible base64 decoder' >&2
    shift 3
    exec "$@"
fi
if zmux_bootstrap=$(printf '%s' "$zmux_bootstrap_payload" | base64 "$zmux_base64_flag" 2>/dev/null | gzip -dc 2>/dev/null || exit 1
    printf .); then
    :
else
    printf '%s\n' 'zmux: shell integration bootstrap could not be decoded' >&2
    shift 3
    exec "$@"
fi
zmux_bootstrap=@NEW@
eval "$zmux_bootstrap"
"#;
    decoder
        .replace("@PAYLOAD@", payload)
        .replace("@NEW@", &["$", "{zmux_bootstrap%.}"].concat())
}

fn bootstrap_script(install_openers: bool, install_remote_lifecycle: bool) -> String {
    let mut generated = String::new();
    let files = [
        ("claude.sh", include_str!("shell/claude.sh")),
        (
            "bashrc",
            r#". "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.sh"
case "${ZMUX_USER_BASHRC_MODE-default}" in
    file) [ -r "$ZMUX_USER_BASHRC" ] && . "$ZMUX_USER_BASHRC" ;;
    default) [ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc" ;;
esac
zmux_install_program_status_prompt
"#,
        ),
        ("claude.fish", include_str!("shell/claude.fish")),
        ("provider.sh", include_str!("shell/provider.sh")),
        ("provider.fish", include_str!("shell/provider.fish")),
        (
            "provider-launch.py",
            include_str!("shell/provider-launch.py"),
        ),
        (
            "provider-opencode-plugin.js",
            include_str!("shell/provider-opencode-plugin.js"),
        ),
        (
            "plugin/.claude-plugin/plugin.json",
            r#"{"name":"zmux-attention","version":"0.1.0","description":"Terminal-local completion and attention notifications"}"#,
        ),
        (
            ".zshenv",
            r#"if [[ -r "$ZMUX_USER_ZDOTDIR/.zshenv" ]]; then
    if [[ "$ZMUX_USER_ZDOTDIR_SET" == x ]]; then ZDOTDIR=$ZMUX_USER_ZDOTDIR; else unset ZDOTDIR; fi
    source "$ZMUX_USER_ZDOTDIR/.zshenv"
    ZMUX_USER_ZDOTDIR=${ZDOTDIR-$HOME}
    ZMUX_USER_ZDOTDIR_SET=${ZDOTDIR+x}
    export ZMUX_USER_ZDOTDIR ZMUX_USER_ZDOTDIR_SET
fi
ZDOTDIR=$ZMUX_SHELL_INTEGRATION_DIRECTORY
"#,
        ),
        (
            ".zprofile",
            r#"if [[ -r "$ZMUX_USER_ZDOTDIR/.zprofile" ]]; then
    if [[ "$ZMUX_USER_ZDOTDIR_SET" == x ]]; then ZDOTDIR=$ZMUX_USER_ZDOTDIR; else unset ZDOTDIR; fi
    source "$ZMUX_USER_ZDOTDIR/.zprofile"
    ZMUX_USER_ZDOTDIR=${ZDOTDIR-$HOME}
    ZMUX_USER_ZDOTDIR_SET=${ZDOTDIR+x}
    export ZMUX_USER_ZDOTDIR ZMUX_USER_ZDOTDIR_SET
fi
ZDOTDIR=$ZMUX_SHELL_INTEGRATION_DIRECTORY
"#,
        ),
        (
            ".zshrc",
            r#"source "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.zsh"
if [[ "$ZMUX_USER_ZDOTDIR_SET" == x ]]; then ZDOTDIR=$ZMUX_USER_ZDOTDIR; else unset ZDOTDIR; fi
if [[ -r "$ZMUX_USER_ZDOTDIR/.zshrc" ]]; then source "$ZMUX_USER_ZDOTDIR/.zshrc"; fi
"#,
        ),
        (
            "env",
            r#". "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.sh"
ENV=$ZMUX_USER_ENV
if [ -n "$ENV" ] && [ -r "$ENV" ]; then . "$ENV"; fi
"#,
        ),
    ];
    let hook_config = if install_remote_lifecycle {
        include_str!("shell/claude-hooks-remote.json")
    } else {
        include_str!("shell/claude-hooks-local.json")
    };
    generated.push_str(&format!(
        "cat >\"$zmux_private_root/plugin/hooks/hooks.json\" <<'ZMUX_SHELL_FILE_END'\n{hook_config}\nZMUX_SHELL_FILE_END\n"
    ));
    if install_remote_lifecycle {
        for (name, contents) in [
            (
                "claude-lifecycle-hook.py",
                include_str!("shell/claude-lifecycle-hook.py"),
            ),
            (
                "claude-lifecycle-notice.py",
                include_str!("shell/claude-lifecycle-notice.py"),
            ),
            (
                "claude-lifecycle-stream.py",
                include_str!("shell/claude-lifecycle-stream.py"),
            ),
        ] {
            generated.push_str(&format!(
                "cat >\"$zmux_private_root/{name}\" <<'ZMUX_SHELL_FILE_END'\n{contents}\nZMUX_SHELL_FILE_END\n"
            ));
        }
    }
    if install_openers {
        generated.push_str("mkdir \"$zmux_private_root/bin\" || exit 70\n");
        for name in ["zmux-open-url", "open", "xdg-open", "sensible-browser"] {
            let contents = if name == "zmux-open-url" {
                include_str!("shell/open-url.sh")
            } else {
                include_str!("shell/open-wrapper.sh")
            };
            generated.push_str(&format!(
                "cat >\"$zmux_private_root/bin/{name}\" <<'ZMUX_SHELL_FILE_END'\n{contents}\nZMUX_SHELL_FILE_END\n"
            ));
            generated.push_str(&format!(
                "chmod 700 \"$zmux_private_root/bin/{name}\" || exit 70\n"
            ));
        }
    }
    for (name, contents) in files {
        generated.push_str(&format!("cat >\"$zmux_private_root/{name}\" <<'ZMUX_SHELL_FILE_END'\n{contents}\nZMUX_SHELL_FILE_END\n"));
    }
    generated.push_str(&format!(
        "cat >\"$zmux_private_root/claude.zsh\" <<'ZMUX_SHELL_FILE_END'\n{}\nZMUX_SHELL_FILE_END\n",
        include_str!("shell/claude.sh").replace("claude() {", "function claude {")
    ));
    include_str!("shell/bootstrap.sh").replace("# ZMUX_GENERATED_FILES", &generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    #[test]
    fn compressed_remote_bootstrap_round_trips_exact_bytes() {
        let expected = bootstrap_script(true, true);
        let payload = bootstrap_payload(true, true).unwrap();
        let compressed = BASE64.decode(payload.as_bytes()).unwrap();
        let decoder = GzDecoder::new(compressed.as_slice());
        let mut decoded = Vec::new();
        decoder
            .take((MAX_BOOTSTRAP_BYTES + 1) as u64)
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, expected.as_bytes());
        assert!(payload.len() <= MAX_BOOTSTRAP_BASE64_BYTES);
    }
}
