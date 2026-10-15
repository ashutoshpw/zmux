//! One command catalog drives public discovery, offline help, aliases and parser routing.
use super::CliError;
use serde_json::{Value, json};
use std::fmt::Write;

#[derive(Clone, Copy)]
pub(super) struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub group: &'static str,
    pub targeted: bool,
    pub socket: bool,
    pub usage: &'static str,
    pub description: &'static str,
}
macro_rules! command {
    ($name:literal, $group:literal, $targeted:literal, $socket:literal, $usage:literal, $description:literal) => {
        CommandSpec {
            name: $name,
            aliases: &[],
            group: $group,
            targeted: $targeted,
            socket: $socket,
            usage: $usage,
            description: $description,
        }
    };
}
pub(super) const GROUPS: &[(&str, &str)] = &[
    ("start", "Start and recover"),
    ("navigate", "Navigate and arrange"),
    ("inspect", "Inspect terminals"),
    ("automation", "Input and attention"),
    ("browser", "Embedded browser"),
    ("customize", "Help and configuration"),
    ("agents", "Agents"),
    ("remote", "Remote sessions"),
    ("diagnostics", "Diagnostics"),
];
pub(super) const COMMANDS: &[CommandSpec] = &[
    command!(
        "paste-files",
        "automation",
        true,
        true,
        "paste-files [--workspace TARGET] [--pane TARGET] [--surface TARGET] -- PATH ...",
        "Insert quoted local paths, or upload regular files through the target SSH terminal's authenticated connection and insert remote paths. Never submits a command. Up to 16 files, 64 MiB each and 256 MiB total. CLI input is file paths. The native app also prepares clipboard images for paste/drop. Directories remain unavailable."
    ),
    command!(
        "list-uploads",
        "inspect",
        false,
        true,
        "list-uploads",
        "Show bounded upload progress, remote paths, errors and origin-bound insertion results."
    ),
    command!(
        "cancel-upload",
        "automation",
        false,
        true,
        "cancel-upload ID",
        "Cancel an upload and suppress path insertion. Already completed remote files remain reported; cancellation does not remove them."
    ),
    command!(
        "ssh",
        "remote",
        false,
        true,
        "ssh DESTINATION [--name NAME] [--port PORT] [--identity PATH] [--ssh-config PATH] [-A|-a] [--jump HOST] [--ssh-option KEY=VALUE] [--command TEXT] [--focus true|false|--no-focus] [-- REMOTE_ARG ...]",
        "Create an SSH terminal workspace. Connection arguments are validated and retained on the surface; reopening starts a fresh remote login without replaying commands. Regular file/image uploads reuse this terminal's private authenticated connection. The native app supports clipboard image paste/drop; this CLI accepts image file paths through paste-files. Cross-tab connection sharing and reconnect remain unavailable. Extra SSH options use a bounded allowlist; mosh is unavailable."
    ),
    command!(
        "new-workspace",
        "start",
        false,
        true,
        "new-workspace NAME",
        "Create and focus a workspace with a local terminal."
    ),
    command!(
        "new-surface",
        "start",
        true,
        true,
        "new-surface [PANE]",
        "Create an independent terminal tab in a pane."
    ),
    command!(
        "reopen-closed",
        "start",
        false,
        true,
        "reopen-closed [CLOSED_ID]",
        "Restore a closed item with fresh shells at saved working directories. Launch failures remain retryable; commands are never replayed."
    ),
    command!(
        "recently-closed",
        "start",
        false,
        true,
        "recently-closed",
        "List bounded in-process close history and restoration state."
    ),
    command!(
        "select-workspace",
        "navigate",
        true,
        true,
        "select-workspace [WORKSPACE]",
        "Select a workspace and retain its selected pane and tab."
    ),
    command!(
        "new-split",
        "navigate",
        true,
        true,
        "new-split <left|right|up|down> [--workspace TARGET] [--pane TARGET] [--surface TARGET] [--focus true|false|--no-focus]",
        "Create a terminal pane beside the target. Focus stays unchanged by default; --focus true selects the new pane. Use created_panes and created_surfaces in the result to identify new objects; focused_* describes selection. Options may precede or follow the direction."
    ),
    CommandSpec {
        name: "next-workspace",
        aliases: &["next-window"],
        group: "navigate",
        targeted: true,
        socket: true,
        usage: "next-workspace [--workspace TARGET]",
        description: "Select the next workspace in model order, wrapping at the end. next-window is an alias for workspace navigation; --window is unavailable.",
    },
    CommandSpec {
        name: "previous-workspace",
        aliases: &["previous-window"],
        group: "navigate",
        targeted: true,
        socket: true,
        usage: "previous-workspace [--workspace TARGET]",
        description: "Select the previous workspace in model order, wrapping at the beginning. previous-window is an alias for workspace navigation; --window is unavailable.",
    },
    CommandSpec {
        name: "last-workspace",
        aliases: &["last-window"],
        group: "navigate",
        targeted: true,
        socket: true,
        usage: "last-workspace [--workspace TARGET]",
        description: "Select the most recently focused other workspace. Fails when no live history remains. last-window is an alias for workspace navigation; --window is unavailable.",
    },
    command!(
        "next-tab",
        "navigate",
        true,
        true,
        "next-tab [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Select the next tab in the target pane, wrapping at the end."
    ),
    command!(
        "previous-tab",
        "navigate",
        true,
        true,
        "previous-tab [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Select the previous tab in the target pane, wrapping at the beginning."
    ),
    command!(
        "last-tab",
        "navigate",
        true,
        true,
        "last-tab [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Select the most recently focused other tab in the target pane."
    ),
    command!(
        "next-pane",
        "navigate",
        true,
        true,
        "next-pane [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Focus the next pane in split-tree order, wrapping at the end."
    ),
    command!(
        "previous-pane",
        "navigate",
        true,
        true,
        "previous-pane [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Focus the previous pane in split-tree order, wrapping at the beginning."
    ),
    command!(
        "last-pane",
        "navigate",
        true,
        true,
        "last-pane [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Focus the most recently focused other pane within the target workspace."
    ),
    command!(
        "focus-direction",
        "navigate",
        true,
        true,
        "focus-direction <left|right|up|down> [--workspace TARGET] [--pane TARGET] [--surface TARGET]",
        "Focus an adjacent pane using equal-share split topology. Fails when no adjacent pane exists; native divider ratios do not affect this selection."
    ),
    command!(
        "tab-action",
        "navigate",
        true,
        true,
        "tab-action <rename|close-others> [--tab TARGET] [--workspace TARGET] [--pane TARGET] [--title TITLE]",
        "Apply a supported tab action. --action may supply the action instead of a positional verb. Rename accepts --title or trailing title words; close-others closes sibling tabs as one recoverable group. Other actions and flags are rejected."
    ),
    command!(
        "focus-history",
        "navigate",
        false,
        true,
        "focus-history <back|forward>",
        "Navigate global live focus history across workspaces. Accepts no target selectors or inherited caller scope."
    ),
    command!(
        "rename-workspace",
        "navigate",
        true,
        true,
        "rename-workspace [WORKSPACE] NAME",
        "Rename a workspace without changing focus. Names contain 1–128 non-control characters."
    ),
    command!(
        "close-workspace",
        "navigate",
        true,
        true,
        "close-workspace [WORKSPACE]",
        "Close all workspace tabs as one recoverable layout. This explicit CLI operation does not prompt."
    ),
    command!(
        "focus-pane",
        "navigate",
        true,
        true,
        "focus-pane [PANE]",
        "Focus a pane and its owning workspace."
    ),
    command!(
        "split-pane",
        "navigate",
        true,
        true,
        "split-pane [PANE] <horizontal|vertical>",
        "Split a pane and focus the new terminal."
    ),
    command!(
        "close-pane",
        "navigate",
        true,
        true,
        "close-pane [PANE]",
        "Close a pane and its tabs; remove the workspace when its last pane closes."
    ),
    command!(
        "select-surface",
        "navigate",
        true,
        true,
        "select-surface [SURFACE]",
        "Select a live tab and focus its pane and workspace."
    ),
    command!(
        "close-surface",
        "navigate",
        true,
        true,
        "close-surface [SURFACE]",
        "Close one tab, preserving sibling sessions and a restore descriptor."
    ),
    command!(
        "close-other-tabs",
        "navigate",
        true,
        true,
        "close-other-tabs [SURFACE]",
        "Close sibling tabs as one recoverable group. This explicit CLI operation does not prompt."
    ),
    command!(
        "rename-tab",
        "navigate",
        true,
        true,
        "rename-tab [SURFACE] TITLE",
        "Set a tab's title."
    ),
    command!(
        "reorder-surface",
        "navigate",
        true,
        true,
        "reorder-surface [SURFACE] INDEX",
        "Move a tab to a zero-based final position in its pane."
    ),
    command!(
        "move-surface",
        "navigate",
        true,
        true,
        "move-surface [SURFACE] DESTINATION_PANE INDEX",
        "Move a live tab to another pane at a zero-based position without restarting its process."
    ),
    command!(
        "list-workspaces",
        "inspect",
        false,
        true,
        "list-workspaces",
        "List workspace trees and a live UUID/ref/index target catalog."
    ),
    command!(
        "list-panes",
        "inspect",
        true,
        true,
        "list-panes [--workspace TARGET]",
        "List panes, selected sessions, process readiness and failures. Outside a caller context, an unscoped request lists every workspace."
    ),
    command!(
        "list-pane-surfaces",
        "inspect",
        true,
        true,
        "list-pane-surfaces [PANE]",
        "List every tab in a pane, including hidden tabs, UUIDs and selected state."
    ),
    command!(
        "read-screen",
        "inspect",
        true,
        true,
        "read-screen [PANE]",
        "Read visible text, cursor, geometry and exit metadata. Use --surface to read an exact hidden terminal."
    ),
    command!(
        "send-text",
        "automation",
        true,
        true,
        "send-text [PANE] <TEXT|--stdin>",
        "Send exact UTF-8 bytes; no newline is added. Input is limited to 4096 bytes. Do not retry an uncertain mutation automatically."
    ),
    command!(
        "notify",
        "automation",
        true,
        true,
        "notify [--surface TARGET] --title TITLE [--body TEXT]",
        "Add bounded in-app attention to the exact live surface."
    ),
    command!(
        "list-notifications",
        "automation",
        false,
        true,
        "list-notifications",
        "List notification IDs, targets, unread state and dropped-event count."
    ),
    command!(
        "mark-notification-read",
        "automation",
        false,
        true,
        "mark-notification-read --id ID",
        "Mark one notification generation read."
    ),
    command!(
        "open-notification",
        "automation",
        false,
        true,
        "open-notification --id ID",
        "Focus the notification's live target and acknowledge that generation."
    ),
    command!(
        "dismiss-notification",
        "automation",
        false,
        true,
        "dismiss-notification --id ID",
        "Remove one notification."
    ),
    command!(
        "clear-notifications",
        "automation",
        false,
        true,
        "clear-notifications --through ID",
        "Clear history through an observed ID, preserving newer arrivals."
    ),
    command!(
        "browser",
        "browser",
        true,
        true,
        "browser open [PANE] URL\n       zmux browser navigate|goto [SURFACE] URL",
        "Open or navigate an embedded browser tab. Allowed URLs: HTTP(S) and about:blank. Browser surfaces allocate no PTY."
    ),
    command!(
        "doctor",
        "diagnostics",
        false,
        true,
        "doctor",
        "Check the selected socket, protocol and running application."
    ),
    command!(
        "status",
        "diagnostics",
        false,
        true,
        "status",
        "Show running app, workspace, pane and process counts."
    ),
    command!(
        "capabilities",
        "diagnostics",
        false,
        true,
        "capabilities",
        "Discover the running app's actual protocol, targeting and resource limits."
    ),
    command!(
        "prototype",
        "diagnostics",
        false,
        false,
        "prototype [--timeout SECONDS] -- PROGRAM [ARG ...]",
        "Run a standalone local PTY and print its final visible screen as JSON. Does not require the app."
    ),
    CommandSpec {
        name: "version",
        aliases: &["--version", "-V"],
        group: "diagnostics",
        targeted: false,
        socket: false,
        usage: "version",
        description: "Print the CLI version without connecting to the app.",
    },
    command!(
        "commands",
        "customize",
        false,
        false,
        "commands",
        "Print the executable command catalog as JSON without connecting to the app."
    ),
    CommandSpec {
        name: "guide",
        aliases: &["--skill"],
        group: "customize",
        targeted: false,
        socket: false,
        usage: "guide | --skill",
        description: "Print an automation guide for this build's implemented commands.",
    },
    CommandSpec {
        name: "help",
        aliases: &["--help", "-h"],
        group: "customize",
        targeted: false,
        socket: false,
        usage: "help [COMMAND|TOPIC]",
        description: "Read offline help for one command or a task topic.",
    },
];

const GLOBAL: &str = "Options:\n  --socket <PATH>      Select an absolute Unix socket path\n  --password <TEXT>    Authenticate to a password-protected local socket\n  --timeout <MILLIS>   Client deadline (50–30000 milliseconds)\n  -h, --help           Read offline help\n  -V, --version        Print version\n";
const TARGETS: &str = "Targets:\n  UUID, positive legacy ID, workspace:N/pane:N/surface:N, or tab:N.\n  index:N selects a one-based position; reorder/move INDEX is zero-based.\n  Selector options constrain ownership where accepted; consult command usage.\n  --tab aliases --surface. Workspace navigation accepts --workspace only.\n  --expect-revision <N> checks an observed model_revision before the operation.\n  Place selector options before positional arguments; -- preserves literal text.\n  new-split, focus-direction and tab-action also allow options after their verb.\n  Explicit selectors override inherited caller defaults. Without them,\n  ZMUX_SURFACE_ID (or ZMUX_TAB_ID), then ZMUX_WORKSPACE_ID, then focus applies.\n  Surface caller identity follows tab moves. Closed/stale targets fail.\n  UUIDs remain stable within the live model; reopened terminals receive new UUIDs.\n";
pub(super) fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|s| s.name == name || s.aliases.contains(&name))
}
pub(super) fn top_help() -> String {
    let mut out = String::from(
        "zmux — control terminal workspaces through the local socket\n\nUsage: zmux [OPTIONS] <COMMAND> [ARGS]\n\n",
    );
    out.push_str(GLOBAL);
    out.push('\n');
    out.push_str(TARGETS);
    out.push_str("\nSocket authentication:\n  Same-user peer checks apply in every mode. Password precedence:\n  --password, ZMUX_SOCKET_PASSWORD, then ~/.config/zmux/socket-password.\n  ZMUX_SOCKET_PASSWORD_FILE selects another private credential file.\n  ZMUX_SOCKET_PATH overrides the default socket (ZMUX_SOCKET remains an alias).\n");
    out.push_str("\nCommands:\n");
    for (group, title) in GROUPS {
        let rows: Vec<_> = COMMANDS.iter().filter(|s| s.group == *group).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(out, "  {title}:");
        for spec in rows {
            let _ = writeln!(out, "    {}", spec.usage);
        }
    }
    out.push_str("\nLocal-control results use JSON. Read: zmux help <command|topic>.\nAutomation guide: zmux guide. Machine catalog: zmux commands.\n");
    out
}
pub(super) fn help(name: Option<&str>) -> Result<String, CliError> {
    let Some(name) = name else {
        return Ok(top_help());
    };
    if let Some(spec) = find(name) {
        let mut out = format!(
            "Usage: zmux [OPTIONS] {}\n\n{}\n",
            spec.usage, spec.description
        );
        if !spec.aliases.is_empty() {
            let _ = writeln!(out, "\nAliases: {}", spec.aliases.join(", "));
        }
        if spec.targeted {
            out.push('\n');
            out.push_str(TARGETS);
        }
        out.push('\n');
        out.push_str(GLOBAL);
        return Ok(out);
    }
    let topic = match name {
        "resume" => "start",
        "arrange" => "navigate",
        "advanced" => "diagnostics",
        other => other,
    };
    let Some((_, title)) = GROUPS.iter().find(|(group, _)| *group == topic) else {
        return Err(CliError(format!("unknown command or help topic `{name}`")));
    };
    let mut out = format!("zmux — {title}\n\n");
    let mut any = false;
    for spec in COMMANDS.iter().filter(|s| s.group == topic) {
        any = true;
        let _ = writeln!(out, "  zmux {}\n    {}\n", spec.usage, spec.description);
    }
    if !any {
        out.push_str("No executable commands in this topic are available in this build.\n");
    }
    Ok(out)
}
pub(super) fn catalog() -> Value {
    json!({"schema_version": 1, "parity_status": "incomplete", "version": env!("CARGO_PKG_VERSION"), "commands": COMMANDS.iter().map(|s| json!({
        "name": s.name, "aliases": s.aliases, "group": s.group, "usage": s.usage, "description": s.description,
        "targeted": s.targeted, "socket_required": s.socket, "availability": "implemented"
    })).collect::<Vec<_>>()})
}
pub(super) fn guide() -> String {
    format!(
        "# zmux automation guide\n\nUse `zmux commands` to discover this build's executable commands.\nRead `zmux help <command>` or `zmux help <topic>` before constructing requests.\n\nInspect with list-workspaces, list-panes and list-pane-surfaces. Preserve returned\nUUIDs when later requests must address the same object. Use leading selectors for\nexplicit scope; inherited terminal context applies when selectors are omitted.\nUse --expect-revision for operations that depend on a previously observed index.\n\nSend exact text with send-text; include carriage return only when submission is intended.\nUse --stdin for multiline text. Never retry a timeout whose outcome is unknown.\nObserve readiness after new-workspace, new-surface, new-split or reopen-closed: launch is asynchronous.\nReopening starts fresh shells at saved working directories and does not replay jobs.\n\n{}\n{}",
        GLOBAL, TARGETS
    )
}
