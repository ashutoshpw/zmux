//! Session-only shell/provider integration, shared by local and SSH launches.
//! Never edits user startup files, provider settings or global hooks.
use crate::Launch;
use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const FILES: &[&str] = &[
    "claude.sh",
    "claude.zsh",
    "claude.fish",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    "env",
    "attention",
    "plugin/.claude-plugin/plugin.json",
    "plugin/hooks/hooks.json",
];
const DIRECTORIES: &[&str] = &["plugin/.claude-plugin", "plugin/hooks", "plugin"];

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
pub fn prepare(mut launch: Launch) -> io::Result<(Launch, Option<ShellIntegrationLease>)> {
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
    let mut directory = None;
    for _ in 0..128 {
        let path = std::env::temp_dir().join(format!(
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
    let mut args = vec![
        "-c".into(),
        bootstrap_script(),
        "zmux-shell".into(),
        root,
        launch.program,
    ];
    args.extend(launch.args);
    launch.program = "/bin/sh".into();
    launch.args = args;
    Ok((launch, Some(lease)))
}

/// Original remote shell code, carried in the already authenticated PTY. No
/// local socket/password is forwarded; OSC notices are bound to that exact PTY.
pub fn remote_command() -> String {
    let script = bootstrap_script().replace('\'', "'\\''");
    format!("/bin/sh -c '{script}' zmux-shell '' \"${{SHELL:-/bin/sh}}\" -l")
}

fn bootstrap_script() -> String {
    let mut generated = String::new();
    let files = [
        ("claude.sh", include_str!("shell/claude.sh")),
        ("claude.fish", include_str!("shell/claude.fish")),
        (
            "plugin/.claude-plugin/plugin.json",
            r#"{"name":"zmux-attention","version":"0.1.0","description":"Terminal-local completion and attention notifications"}"#,
        ),
        (
            "plugin/hooks/hooks.json",
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"{ [ -p \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\" ] && printf 'stop\\n' > \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\"; } 2>/dev/null || :","timeout":3}]}],"StopFailure":[{"hooks":[{"type":"command","command":"{ [ -p \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\" ] && printf 'failure\\n' > \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\"; } 2>/dev/null || :","timeout":3}]}],"Notification":[{"matcher":"permission_prompt|elicitation_dialog","hooks":[{"type":"command","command":"{ [ -p \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\" ] && printf 'attention\\n' > \"$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention\"; } 2>/dev/null || :","timeout":3}]}]}}"#,
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
            r#"if [[ "$ZMUX_USER_ZDOTDIR_SET" == x ]]; then ZDOTDIR=$ZMUX_USER_ZDOTDIR; else unset ZDOTDIR; fi
if [[ -r "$ZMUX_USER_ZDOTDIR/.zshrc" ]]; then source "$ZMUX_USER_ZDOTDIR/.zshrc"; fi
source "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.zsh"
"#,
        ),
        (
            "env",
            r#"ENV=$ZMUX_USER_ENV
if [ -n "$ENV" ] && [ -r "$ENV" ]; then . "$ENV"; fi
. "$ZMUX_SHELL_INTEGRATION_DIRECTORY/claude.sh"
"#,
        ),
    ];
    for (name, contents) in files {
        generated.push_str(&format!("cat >\"$zmux_private_root/{name}\" <<'ZMUX_SHELL_FILE_END'\n{contents}\nZMUX_SHELL_FILE_END\n"));
    }
    generated.push_str(&format!(
        "cat >\"$zmux_private_root/claude.zsh\" <<'ZMUX_SHELL_FILE_END'\n{}\nZMUX_SHELL_FILE_END\n",
        include_str!("shell/claude.sh").replace("claude() {", "function claude {")
    ));
    include_str!("shell/bootstrap.sh").replace("# ZMUX_GENERATED_FILES", &generated)
}
