//! One command catalog drives public discovery, offline help, aliases and parser routing.
use super::CliError;
use serde_json::{Value, json};
use std::fmt::Write;
mod contracts;

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
        "hooks",
        "agents",
        false,
        false,
        "hooks claude install --directory DIR --provider PATH | status|uninstall --directory DIR | run --directory DIR --surface UUID -- [PROVIDER_ARGS]",
        "Opt-in session-only local Claude Code2.1.301 plugin. Install writes only private zmux-owned artifacts and never edits provider settings. Run requires the exact Desktop terminal process/session and an interactive terminal. A synchronous root UserPromptSubmit may prepare one bounded context; unknown submission stays queued and never authorizes Ack/ReadTurn. Permission/question/plan holds persist until a fresh explicit launch. No idle wake, Codex, headless, resume or actual-provider acceptance claim. A local hook process binding is not provider authentication. Uninstall disables admissions and retains changed or active artifacts."
    ),
    command!(
        "agent",
        "agents",
        false,
        true,
        "agent message|msg TARGET [--from LABEL] [--thread UUID] [--request-id UUID] [--] TEXT|- | agent message --reply-to UUID TEXT|- | agent inbox [--surface HANDLE] [--state queued|delivered|read|failed] [--limit 1..32] [--cursor N] [--mark-read] | agent messages [on|off|status] [TARGET] [--workspace|--global] | agent retry --surface UUID --accept-duplicate MESSAGE_ID...",
        "Durable local agent messages, replies, inbox and opt-out settings. Bodies preserve UTF-8 whitespace and are limited to 4096 bytes; - reads bounded stdin. Message/msg is an alias. Recipient targets and caller sender attribution resolve independently. A successful send means saved in the local journal, not delivered to a provider; no terminal input is injected. --mark-read marks only the exact listed message IDs. Results are JSON. One client deadline covers persistence and selected-ID marking; timeout never retries a send. Unknown provider submissions remain queued and are excluded from automatic delivery. Explicit retry preserves the original IDs and may duplicate a prior submission. The opt-in local Claude hook wrapper reports unconfirmed submission only. Provider wake, native Agent Inbox and full reference size/output parity remain unavailable."
    ),
    command!(
        "set-buffer",
        "automation",
        false,
        false,
        "set-buffer [--name NAME] [--] [TEXT | -]",
        "Persist exact UTF-8 text in a private named buffer (default: default). Omitted text or - reads bounded stdin before any socket connection. Leading/trailing whitespace is preserved. Limits: 4096 bytes per buffer, 128 names, 512 KiB serialized store. Names contain 1–128 bytes without controls. Uses ~/.config/zmux/buffers; no running app required. Existing corrupt or unsafe state fails without replacement."
    ),
    command!(
        "list-buffers",
        "automation",
        false,
        false,
        "list-buffers [--json]",
        "List persistent named buffers alphabetically as JSON without exposing contents. Size is explicitly UTF-8 bytes; the reference's grapheme-count display remains a compatibility gap. No running app required."
    ),
    command!(
        "paste-buffer",
        "automation",
        false,
        true,
        "paste-buffer [--name NAME] [--bracketed] [--window HANDLE] [--workspace HANDLE] [--pane HANDLE] [--surface HANDLE]",
        "Send one saved buffer to a live terminal using shared target resolution without changing focus. --bracketed uses the same terminal paste owner as native Paste, honoring terminal bracketed-paste mode. Missing buffers, stale/cross-owner targets, browser targets and read-only sockets fail. A timeout never retries input. All results are JSON; full historical output and size-limit compatibility remain unverified."
    ),
    command!(
        "current",
        "inspect",
        false,
        true,
        "current [--limit 1..200] [--json]",
        "Inspect app-wide facts from one cached desktop publication without changing focus or refreshing owners. Default limit 100 bounds surfaces in canonical window/workspace/pane/tab order; totals and truncation are explicit. JSON includes exact identities, publication/model revisions, local child lifecycle, metadata counts and unread observations. A running child does not establish an active task, current command or remote liveness. Browser activity, tasks and human obligations remain unknown. Logs/transcripts, resources, external repositories and Cloud machines are not read. --json is an optional compatibility spelling; all results use JSON. Historical text/schema compatibility remains unresolved."
    ),
    command!(
        "local-zellij",
        "remote",
        false,
        false,
        "local-zellij start NAME --headless [--cwd PATH] [--command SHELL_TEXT] | attach --headless --id UUID | list | status --id UUID | cleanup --id UUID --generation UUID",
        "Run one supported local Zellij0.43.0 owner through an exact process-bound foreground client. Requires terminal stdin/stdout and a committed private reservation. Start is never replayed after uncertainty; attach never creates or adopts. List/status distinguish live identities from finite completion authenticated by the original retained supervisor. Explicit cleanup only retires the selected completed generation after durable consumption and frontend release. Automatic close/prune, detached start and GUI attachment are unavailable."
    ),
    command!(
        "workspace",
        "navigate",
        false,
        true,
        "workspace group create --name NAME --from HANDLE,HANDLE [--external-id KEY | --idempotency-key KEY] | list | rename GROUP --name NAME | collapse|expand GROUP | add --group GROUP --workspace HANDLE | remove --workspace HANDLE | set-anchor --group GROUP --workspace HANDLE | move GROUP --index INTEGER | ungroup|delete GROUP [--window HANDLE] [--expect-revision N] [--json]",
        "Original bounded existing-workspace groups. create requires --name and explicit --from HANDLE,HANDLE; it never creates an anchor shell. Stable workspace_group:N and UUID handles survive order changes. --window is a hard constraint; index:N is one-based within the selected window. Groups remain contiguous, metadata changes preserve focus and live Sessions. External identity is window scoped and retry returns the existing group before resolving stale initial members. delete and ungroup dissolve only the container; destructive close/generated-anchor cleanup, pin/color/icon, focus/new-workspace, native group presentation/navigation and full reference output parity remain unavailable. move --index uses a signed final workspace slot after removing the group block; values clamp and insertion cannot split another group. Results are JSON; --json is optional."
    ),
    command!(
        "workspace-group",
        "navigate",
        false,
        true,
        "workspace-group create --name NAME --from HANDLE,HANDLE [--external-id KEY | --idempotency-key KEY] | list | rename GROUP --name NAME | collapse|expand GROUP | add --group GROUP --workspace HANDLE | remove --workspace HANDLE | set-anchor --group GROUP --workspace HANDLE | move GROUP --index INTEGER | ungroup|delete GROUP [--window HANDLE] [--expect-revision N] [--json]",
        "Original bounded existing-workspace groups. create requires --name and explicit --from HANDLE,HANDLE; it never creates an anchor shell. Stable workspace_group:N and UUID handles survive order changes. --window is a hard constraint; index:N is one-based within the selected window. Groups remain contiguous, metadata changes preserve focus and live Sessions. External identity is window scoped and retry returns the existing group before resolving stale initial members. delete and ungroup dissolve only the container; destructive close/generated-anchor cleanup, pin/color/icon, focus/new-workspace, native group presentation/navigation and full reference output parity remain unavailable. move --index uses a signed final workspace slot after removing the group block; values clamp and insertion cannot split another group. Results are JSON; --json is optional."
    ),
    command!(
        "reorder-workspace",
        "navigate",
        false,
        true,
        "reorder-workspace [--workspace HANDLE | HANDLE] <--index INTEGER | --before HANDLE | --after HANDLE> [--window HANDLE] [--dry-run] [--expect-revision N]",
        "Reorder one workspace without changing focus. Final placement indices are zero-based and signed values clamp; target index:N is one-based within the chosen window. Before/after use the original order; after-self advances one place, clamped at the end. --before-workspace and --after-workspace are aliases. --window is a hard constraint. Dry-run returns the exact plan without mutation."
    ),
    command!(
        "reorder-workspaces",
        "navigate",
        false,
        true,
        "reorder-workspaces --order HANDLE,HANDLE,... [--window HANDLE] [--dry-run] [--expect-revision N]",
        "Place listed workspaces first within one window, preserving the relative order of unlisted workspaces and all focus. Empty, duplicate and cross-window entries reject atomically. Dry-run is read-only. Existing group blocks and anchors must remain intact; pin-tier compatibility is unresolved."
    ),
    command!(
        "move-workspace-to-window",
        "navigate",
        false,
        true,
        "move-workspace-to-window --workspace HANDLE --window DESTINATION [--focus true|false | --no-focus] [--expect-revision N]",
        "Move one existing workspace without restarting its sessions or browsers. --window identifies the destination, not a source constraint; both selectors are required. Focus defaults to false, preserving key window and populated destination selection. --focus true selects the moved workspace and waits for a separate exact native focus receipt; --no-focus is an explicit false alias and conflicts with --focus. JSON reports topology_committed independently from native focus. An empty source stays empty. Same-window moves are bounded no-ops; their full reference parity remains unresolved. Unknown native focus outcomes never replay or roll back committed topology. Split ratios follow workspace identity in native preferences, not portable session exports."
    ),
    command!(
        "list-windows",
        "inspect",
        false,
        true,
        "list-windows",
        "List canonical main windows, workspace membership, logical focus and observed native host/key state as JSON. Failed native opens retain their error and recovery guidance; they are never automatically retried. A logical record alone does not prove a native window exists."
    ),
    command!(
        "current-window",
        "inspect",
        false,
        true,
        "current-window [--window HANDLE]",
        "Resolve the caller's current window without changing focus, or inspect an explicit window root, including an empty window. Surviving caller objects determine ownership after moves; ZMUX_WINDOW_ID is only a fallback hint."
    ),
    command!(
        "new-window",
        "start",
        false,
        true,
        "new-window [--no-focus]",
        "Create a main window with an initial workspace and wait for its exact native acknowledgement. Focus defaults to true; --no-focus preserves current logical and native focus. JSON reports epoch, operation UUID and stable window identity. The default client budget is 35 seconds; an explicit global --timeout wins. A timeout never retries creation."
    ),
    command!(
        "focus-window",
        "navigate",
        false,
        true,
        "focus-window --window HANDLE",
        "Focus an explicit main window, including an empty window, and wait for its exact native receipt. Ambient caller targets are ignored. Native mutations default to a 35-second client budget and are never replayed after uncertainty."
    ),
    command!(
        "close-window",
        "start",
        false,
        true,
        "close-window --window HANDLE",
        "Close an explicit main window and wait for native acknowledgement. Closing the last main window requests application termination; a completed terminate receipt means termination was requested, not that session saving or process exit succeeded. An unavailable final reply remains an unknown outcome. The default client budget is 35 seconds; no automatic retry."
    ),
    command!(
        "local-tmux",
        "remote",
        false,
        false,
        "local-tmux start NAME [--detached] [--cwd PATH] [--command SHELL] [--workspace TARGET] [--no-focus]\n       zmux local-tmux attach <NAME | --id UUID> [--headless | --workspace TARGET] [--new-client] [--no-focus]\n       zmux local-tmux list [--json]\n       zmux local-tmux status <NAME | --id UUID> [--json]\n       zmux local-tmux detach <NAME | --id UUID> [--all]\n       zmux local-tmux close <NAME | --id UUID>\n       zmux local-tmux cleanup [--prune]\n       zmux local-tmux operation-status --id N --epoch UUID",
        "Manage opt-in user-owned local tmux sessions. GUI start/attach use the authenticated app socket; --detached/--headless and lifecycle inspection/mutation operate directly on the private user profile. Detached sessions survive CLI/app exit while their tmux server remains alive; reboot/server termination ends processes. Requires tmux on PATH or an absolute ZMUX_LOCAL_TMUX_BIN. Private state defaults to ~/.config/zmux/local-tmux; ZMUX_LOCAL_TMUX_ROOT selects an absolute profile path whose parent already exists and is user-owned, nonsymlink and unwritable by others. Profile mode is 0700. The app and external CLI must use the same root. ROOT/BIN are installation configuration; caller-only PROFILE is exported by the owned server. Names: 1–128 ASCII letters, digits, underscores or hyphens. Start's --command is explicit shell code; it is not saved in the registry. Headless attach requires terminal stdin/stdout and directly executes a tmux client. List/status return JSON and include stale records; list also reports unmanaged sessions. Detach refuses multiple clients unless --all is explicit. Close terminates only an exact live managed session. Cleanup previews stale records; --prune removes only those records and never kills unknown live sessions. GUI operations return a confirmed client receipt, reuse an exact live client by default, and accept --new-client to attach another surface. A missing client is recreated only in a verified workspace; stale saved workspace bindings require --workspace. --focus true|false and --no-focus control selection. GUI target options cannot accompany --detached/--headless. Closing a surface ends only its client. Reopen/session restore reattach saved immutable identities without replaying startup commands; missing/replaced servers fail visibly. GUI receipts are scoped by runtime epoch; after an unknown timeout inspect operation-status instead of replaying start. Managed tmux callers resolve through their actual pane; multiple matching app surfaces require an explicit target. Individual --client detach and Settings integration remain unavailable."
    ),
    command!(
        "tmux",
        "remote",
        false,
        false,
        "tmux attach <NAME | --id UUID> [--headless | --workspace TARGET] [--new-client] [--no-focus]",
        "Attach-only compatibility spelling of local-tmux attach. Default attachment uses the authenticated app socket; --headless requires terminal stdin/stdout. Other lifecycle verbs use local-tmux."
    ),
    command!(
        "todo",
        "automation",
        true,
        true,
        "todo add TEXT [--state pending|in-progress|completed] [--origin user|agent] [--workspace TARGET]\n       zmux todo list [--workspace TARGET]\n       zmux todo <check|uncheck|start|rm> <INDEX|UUID> [--workspace TARGET]\n       zmux todo clear [--workspace TARGET]",
        "Manage the target workspace's checklist only when the user asks. Add defaults to pending/user. At most 50 items with nonblank text of at most 2048 bytes; all workspace metadata shares a conservative 512 KiB encoded budget. JSON includes ordered todos with stable UUIDs; INDEX is their current one-based array position, resolved atomically at the owner. check/uncheck/start set completed/pending/in-progress; repeated state changes succeed. Unknown or foreign items fail. Clear removes only todos. Requires explicit workspace or proven caller; never borrows foreground focus. List works on read-only sockets; mutations require write access. Todos persist with the workspace through save/export and app reopen; other metadata stays transient. The writer uses session version 6; version 5 and later carry todos and session files have their own 512 KiB bound. Mutation is not a durable-save receipt: inspect persistence status/session-status for failures. Undo-close retains the exact removed owner checklist in bounded process-local history; reopening into a surviving owner preserves its newer checklist. Moving a workspace preserves its checklist. Moving its final surface retires the empty source workspace and its checklist without undo history; the destination checklist stays unchanged. Edit, set, reorder, open and frozen output/schema compatibility are unavailable."
    ),
    command!(
        "pr",
        "automation",
        true,
        true,
        "pr <https://github.com/OWNER/REPO/pull/NUMBER | NUMBER | clear> [--workspace TARGET] [--window TARGET]",
        "Attach a repository-verified manual PR link, or clear only that link. Requires an explicit workspace or live caller; never targets foreground focus. Set runs bounded git/gh from the CLI's local worktree, which must match a local terminal in the target workspace. Origin must be GitHub; gh authentication is required. URL must be canonical and match origin or its verified fork parent; a bare positive number uses the parent when present. No query, fragment, trailing slash, leading zero or alternate host. Workspace/window UUIDs are pinned before lookup; replacement or cross-window moves fail. --timeout covers lookup and socket requests; each child has at most2s additional physical cleanup reserve, never renewed. Lookup output is bounded at64KiB per stream. Clear needs no Git/gh. Results use JSON, including pull_request and stable workspace_uuid. Manual metadata is transient; automatic status refresh, TTY and unique-worktree target inference are unavailable."
    ),
    command!(
        "set-status",
        "automation",
        true,
        true,
        "set-status KEY VALUE [--workspace TARGET] [--icon NAME] [--color #RRGGBB] [--priority N]",
        "Set or replace a workspace status key. Higher priority sorts first, then key. At most 64 keys; names use 1–128 bytes, values at most 2048 bytes. Metadata stays in this app process. All workspaces share a conservative 512 KiB encoded metadata budget; adding data beyond it fails atomically. Clear metadata to release capacity."
    ),
    command!(
        "clear-status",
        "automation",
        true,
        true,
        "clear-status KEY [--workspace TARGET]",
        "Remove a workspace status key. Clearing a missing key succeeds."
    ),
    command!(
        "list-status",
        "automation",
        true,
        true,
        "list-status [--workspace TARGET]",
        "List workspace status entries, progress and bounded log metadata as JSON."
    ),
    command!(
        "set-progress",
        "automation",
        true,
        true,
        "set-progress <0.0-1.0> [--workspace TARGET] [--label TEXT]",
        "Replace workspace progress with a finite value from 0 to 1 and an optional label of at most 2048 bytes. Completion remains until cleared."
    ),
    command!(
        "clear-progress",
        "automation",
        true,
        true,
        "clear-progress [--workspace TARGET]",
        "Clear workspace progress."
    ),
    command!(
        "log",
        "automation",
        true,
        true,
        "log [--workspace TARGET] [--level LEVEL] [--source NAME] [--] MESSAGE",
        "Append a workspace log message (at most 1024 bytes). Levels: info (default), progress, success, warning, error. Retains the newest 256 entries and reports logs_dropped; entries list oldest first."
    ),
    command!(
        "clear-log",
        "automation",
        true,
        true,
        "clear-log [--workspace TARGET]",
        "Clear workspace log entries and their dropped count."
    ),
    command!(
        "list-log",
        "automation",
        true,
        true,
        "list-log [--workspace TARGET] [--limit N]",
        "List workspace metadata with the newest N log entries, in chronological order. N is 1–256; omit it to include all retained logs."
    ),
    command!(
        "restore-session",
        "start",
        false,
        true,
        "restore-session [--from PATH | --export PATH [--force]]",
        "Restore the default saved session or a private session file into an empty desktop. A populated desktop refuses restore; additive windows are unavailable. --export writes the current restorable layout to PATH without replacing an existing file unless --force is set. Relative paths resolve from the CLI's working directory. Operations return a pending operation_id; inspect session-status --id ID for completion or an error. Fresh shells start without replaying commands or recovering running jobs. Caller target variables do not select a session."
    ),
    command!(
        "save-session",
        "start",
        false,
        true,
        "save-session",
        "Save the current restorable layout to the app's default private session file. The response admits asynchronous work; inspect session-status --id ID to confirm completion or an error."
    ),
    command!(
        "session-status",
        "start",
        false,
        true,
        "session-status [--id ID]",
        "Show persistence state or the bounded receipt for an admitted operation. A pending receipt does not establish that a file was saved or a restore completed. Unknown or expired operation IDs fail."
    ),
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
        description: "Select the next workspace in model order, wrapping at the end. next-window remains an alias for workspace navigation; --window constrains workspace ownership.",
    },
    CommandSpec {
        name: "previous-workspace",
        aliases: &["previous-window"],
        group: "navigate",
        targeted: true,
        socket: true,
        usage: "previous-workspace [--workspace TARGET]",
        description: "Select the previous workspace in model order, wrapping at the beginning. previous-window remains an alias for workspace navigation; --window constrains workspace ownership.",
    },
    CommandSpec {
        name: "last-workspace",
        aliases: &["last-window"],
        group: "navigate",
        targeted: true,
        socket: true,
        usage: "last-workspace [--workspace TARGET]",
        description: "Select the most recently focused other workspace. Fails when no live history remains. last-window remains an alias for workspace navigation; --window constrains workspace ownership.",
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
        "reorder-surface [--surface HANDLE | HANDLE] (--index INTEGER | --before HANDLE | --after HANDLE) [--workspace HANDLE] [--window HANDLE] [--focus true|false]",
        "Reorder within one pane without focus by default. Signed final indices clamp; self anchors are no-ops. Legacy [SURFACE] INDEX is retained."
    ),
    command!(
        "move-surface",
        "navigate",
        true,
        true,
        "move-surface [SURFACE] DESTINATION_PANE INDEX",
        "Move a live tab to another pane at a zero-based position without restarting its process. Moving the final surface retires the empty source workspace and its checklist without undo history; the destination checklist stays unchanged."
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
        "notify [--workspace TARGET] [--surface TARGET] [--title TITLE] [--subtitle TEXT] [--body TEXT] [--desktop true|false] [--reply]\n       zmux notify --clear [--workspace TARGET] [--surface TARGET]",
        "Add bounded attention to a live workspace or exact terminal caller. The title defaults to Notification; subtitle and body default to empty. --desktop false suppresses OS delivery; true requests delivery subject to permissions and suppression policy. OS success is not implied by the inbox receipt. --clear removes notifications for the explicit workspace/surface or inherited caller surface, then caller workspace, then focused surface. Surface clear requires workspace context. With only a workspace selector, a live caller surface in that workspace remains the owner; otherwise the notification belongs to the workspace. External callers without surface evidence target the focused workspace. Workspace notifications open without changing the selected tab. Reply is opt-in: --reply requires an exact live terminal caller or --surface, enables a native text action, and never targets a workspace-only or browser notification. A complete reply confirms local PTY acceptance, not remote program consumption; uncertain writes are never resent. Content, --desktop and --reply conflict with --clear."
    ),
    command!(
        "list-notifications",
        "automation",
        false,
        true,
        "list-notifications",
        "List numeric IDs, UUIDs and notification:UUID references, content, targets, read/delivery state, dropped-event count, epoch and cursor. Use an observed cursor with clear-notifications --through to preserve newer arrivals."
    ),
    command!(
        "mark-notification-read",
        "automation",
        true,
        true,
        "mark-notification-read <--id ID | --workspace TARGET [--surface TARGET] | --all>",
        "Mark exactly one notification, a workspace (optionally limited to one surface), or all notifications read. --surface requires an explicit workspace or ZMUX_WORKSPACE_ID. ID and global selectors accept no target scope. ID accepts a positive numeric ID, UUID or notification:UUID reference. This operation preserves focus."
    ),
    command!(
        "open-notification",
        "automation",
        false,
        true,
        "open-notification --id ID",
        "Focus the notification's live target and acknowledge that generation. ID accepts a positive numeric ID, UUID or notification:UUID reference. Closed or stale targets fail without changing focus."
    ),
    command!(
        "jump-to-unread",
        "automation",
        false,
        true,
        "jump-to-unread",
        "Open the newest reachable unread notification and acknowledge only that notification. Closed or stale targets are skipped. Returns opened:false without changing focus when none can open. Caller target environment does not limit this action."
    ),
    command!(
        "dismiss-notification",
        "automation",
        true,
        true,
        "dismiss-notification <--id ID | --all-read>",
        "Remove exactly one notification or every read notification. Unread rows survive --all-read. ID accepts a positive numeric ID, UUID or notification:UUID reference. Target selectors are not accepted and focus is preserved."
    ),
    command!(
        "clear-notifications",
        "automation",
        true,
        true,
        "clear-notifications [--workspace TARGET [--surface TARGET]] [--through CURSOR]",
        "Clear notifications for an explicit workspace or surface. Without selectors, ZMUX_WORKSPACE_ID scopes the clear to that workspace, ignoring the caller surface; without workspace context it clears all notifications. Surface scope requires workspace context. --through accepts an observed list-notifications cursor (including zero) and preserves newer arrivals. Receipts report epoch, cursor, affected_ids and affected_uuids. Focus is preserved."
    ),
    command!(
        "browser",
        "browser",
        true,
        true,
        "browser profiles list|add NAME\n       zmux browser profiles rename NAME_OR_UUID NEW_NAME\n       zmux browser profiles clear|delete NAME_OR_UUID\n       zmux browser profiles status PROFILE_UUID INTENT_UUID\n       zmux browser profiles reconcile PROFILE_UUID INTENT_UUID\n       zmux browser download list|ls [--surface TARGET] [--limit 1..25]\n       zmux browser download [wait] [--surface TARGET] [--path PATH | PATH] [--timeout-ms N | --timeout SECONDS]\n       zmux browser download cancel [--surface TARGET] DOWNLOAD_UUID\n       zmux browser open [PANE] URL [--focus true|false|--no-focus] [--profile NAME_OR_UUID]\n       zmux browser navigate|goto [SURFACE] URL\n       zmux browser back|forward|reload [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser press|key|keydown|keyup [--key KEY | KEY] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser zoom in|out|reset|FACTOR [TARGET_OPTIONS]\n       zmux browser type [--selector SELECTOR | SELECTOR] [--text TEXT | TEXT ...] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser select [--selector SELECTOR | SELECTOR] [--value VALUE | VALUE] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser scroll [--selector SELECTOR] [--dx N] [--dy N | N] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser fill [--selector SELECTOR | SELECTOR] [--text TEXT | TEXT ...] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser click [--selector SELECTOR | SELECTOR] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser dblclick|hover|focus|check|uncheck|scroll-into-view [--selector SELECTOR | SELECTOR] [TARGET_OPTIONS] [--snapshot-after]\n       zmux browser get url|title [TARGET_OPTIONS]\n       zmux browser get text|html|value|count|box|styles [TARGET_OPTIONS] [--selector SELECTOR | SELECTOR] [--property NAME]\n       zmux browser get attr [TARGET_OPTIONS] [--selector SELECTOR | SELECTOR] [--attr NAME | NAME]\n       zmux browser is visible|enabled|checked SELECTOR [TARGET_OPTIONS]\n       zmux browser find role ROLE [--name TEXT] [--exact] [TARGET_OPTIONS]\n       zmux browser find text|label|placeholder|alt|title|testid TEXT [--exact] [TARGET_OPTIONS]\n       zmux browser find first|last --selector SELECTOR [TARGET_OPTIONS]\n       zmux browser find nth --index N --selector SELECTOR [TARGET_OPTIONS]\n       zmux browser wait --selector SELECTOR [--state STATE] [--timeout-ms N] [TARGET_OPTIONS]\n       zmux browser eval [TARGET_OPTIONS] SCRIPT\n       zmux browser eval [TARGET_OPTIONS] --script SCRIPT\n       zmux browser snapshot [TARGET_OPTIONS] [--interactive|-i] [--cursor] [--compact] [--max-depth DEPTH] [--selector SELECTOR]",
        "Control an embedded browser through shared native WebKit execution. Downloads resolve their scoped/caller/current browser once: list is newest-first non-consuming history; no-path wait consumes one terminal event through a repeatable ticket; wait --path only observes a nonempty filesystem path and never chooses a save destination or proves download completion. Wait defaults to 10 seconds, clamps to 1..120000ms and never cancels physical work. Cancel is a mutation; read-only sockets permit list/wait. Native downloads ask for a new destination and refuse overwrite; blob/data downloads and resume remain unavailable. TARGET_OPTIONS: --window, --workspace, --pane, --surface (or --tab), --expect-revision. Automation targets use named options; selector options may precede or follow positional arguments. Browser open/navigate retain the leading-selector and positional-target grammar. Browser open focuses by default; --focus false or --no-focus preserves workspace, pane and tab selection. Use created_surfaces to identify a browser created without focus; focused_* reports the retained selection. HTTP(S) and about:blank are allowed. Automation returns completed JSON results; the global --timeout bounds admission plus result polling. Admission is never retried. Read-only sockets allow get/is/find/wait/snapshot and deny eval/navigation. Eval runs in the page content world; built-in DOM reads use an isolated world. Eval scripts are limited to 8192 UTF-8 bytes, selectors to 2048, depth to 0–32 and results to 64 KiB. --property applies only to get styles. Snapshot is a bounded document tree with document-scoped element refs, not a screenshot. Native execution must be available; a portable desktop alone cannot execute DOM operations. Zoom uses public WebKit pageZoom on the exact live host, with finite numeric factors clamped to 0.25–5.0, 0.1 in/out steps clamped at the limits and reset to 1. Reset to a configured default remains unresolved. It preserves tab selection and document generation; read-only sockets deny it. Zoom is not persisted across host recreation and does not emulate viewport geometry. Fill replaces the first CSS match or exact document ref with text (8192 UTF-8 bytes, no NUL); omitted/empty text clears. Explicit --text overrides positional text; positional words join with spaces. It focuses the DOM element and emits cancelable beforeinput, then native value/text replacement and input/change events. Cancellation reports input_rejected. These are synthetic DOM events, not trusted keyboard input; app selection is retained. Read-only sockets deny fill. Optional --snapshot-after adds a bounded snapshot without repeating the mutation. Click targets the first CSS match or exact document ref; native disabled targets reject. It scrolls nearest, sends synthetic pointer/mouse events, focuses the DOM element and invokes one DOM click activation. Down-event cancellation does not suppress activation; click cancellation controls the default action. Completion reports dispatch, not navigation readiness or guaranteed default action. This is not a trusted OS gesture. Read-only sockets deny click, and uncertain outcomes are never replayed. Dblclick, hover, focus, check, uncheck and scroll-into-view share first CSS match/exact refs, optional snapshot-after and mutation policy: read-only sockets deny all, and uncertain outcomes never replay. Dblclick runs two complete click attempts then one synthetic dblclick with detail 2. Hover scrolls nearest and emits over/enter/move only, allowing disabled targets; it does not activate, focus or guarantee CSS :hover. Focus calls ordinary DOM focus, including default scrolling; disabled/nonfocusable targets can complete without taking focus. Check/uncheck require a checked property and non-disabled target, scroll nearest and focus; same desired state emits no activation/input/change. State changes use one DOM click, except radio uncheck uses the inherited checked setter plus input/change. Final state must match; canceled activation fails not_changed without forcing state. Scroll-into-view (aliases scrollintoview and scrollinto) centers both axes instantly without focus/activation. These synthetic operations retain app selection and do not create trusted OS gestures. Type appends a trimmed nonblank chunk to the entire value/textContent, ignoring caret selection; it uses ordinary focus and cancelable beforeinput insertText, then native replacement and input/change for value fields or input only otherwise. It does not emit keyboard events or enforce disabled/readonly. Select trims a nonblank value, requires any value property, uses its inherited setter and always input/change; it does not focus, reject disabled targets or verify a matching option. Type/select are bounded to 8192 UTF-8 bytes with no NUL; this follows the pinned server trimming behavior, while fill preserves literal whitespace and empty clearing. Scroll accepts exact signed integer pixel dx/dy (default0, magnitude at most9007199254740991), with one positional dy or --dy; omitted selector scrolls window, explicit selector scrolls first element, instantly. Browser geometry may clamp; no focus or wheel-event simulation. All are mutations denied by read-only sockets, retain app selection, support snapshot-after and never replay uncertainty. Keyboard press (key alias), keydown and keyup retain an untrimmed key token (128 UTF-8 bytes, no NUL), including literal space. Supported names and printable ASCII keys use direct native WebKit delivery on the exact live host with scoped held modifiers and native defaults; dispatch completion does not promise a page default or navigation success. Unknown opaque keys receive explicitly synthetic page events only. No native failure silently falls back. App selection and key responder remain unchanged; read-only sockets deny keyboard mutations. Held native state is cleared on owner/document/process replacement or close. Snapshot-after observes without replay. IME, physical input, complete key-map compatibility and lifecycle native acceptance remain separate gates. Main-document semantic locators normalize whitespace; substring matches ignore case, while --exact requires case-sensitive equality. Role names use aria labels, associated HTML labels and common element text/attributes; this is not the complete platform accessibility tree. Hidden elements are discoverable. Semantic find requires exactly one match; first/last/nth select CSS/ref document order explicitly (nth 0–4095). Wait defaults to visible; states: attached, detached, visible, hidden, enabled, disabled, checked, unchecked. Missing CSS matches satisfy only hidden/detached; stale refs always fail. Selectors are strict for wait, which rejects ambiguity. --timeout-ms accepts 1–30000 and adds at most one second of receipt headroom unless an explicit global --timeout is stricter. Queries and selectors are bounded to 2048 UTF-8 bytes, enumeration to 4096 elements. TARGET_OPTIONS may also precede the verb. Convenience aliases: positional wait SELECTOR and get visible|enabled|checked. Explicit persistent profiles: profiles add NAME registers durable metadata; profiles list reports loading/errors and identities. profiles rename NAME_OR_UUID NEW_NAME atomically changes the name while retaining its UUID and website data. --profile resolves an existing name or UUID; omitted profile keeps per-host private storage. Profile names are unique, case-sensitive, trimmed, at most 128 UTF-8 bytes and cannot be UUIDs. Persistent profiles share cookies/localStorage across their own tabs and retain identity on reopen/restore. Unknown profiles fail without fallback. Requires durable runtime state. Clear removes all website data while retaining the profile UUID/name; delete also removes selected catalog membership. Both reject open/hidden tabs, pending constructors, downloads and foreign store owners; explicitly close those owners first. A known WebKit removal callback precedes catalog completion. Missing/uncertain callbacks quarantine the original UUID; query original profile/intent status instead of retrying. Explicit reconcile uses only original captured callback/token evidence for metadata commit or known terminal lock cleanup, including a lost finish acknowledgment; it never repeats WebKit effects. Unknown native callbacks after restart cannot be reconciled and remain quarantined. Data import, frames, shadow traversal, trusted input and screenshots remain unavailable."
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
        "commands [--check]",
        "Print the executable command catalog and bounded argument contracts as JSON without connecting to the app. --check validates parser probes and frozen inventory mappings; it does not establish behavioral compatibility."
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

const GLOBAL: &str = "Options:\n  --socket <PATH>      Select an absolute Unix socket path\n  --password <TEXT>    Authenticate to a password-protected local socket\n  --timeout <MILLIS>   Client response deadline (50–30000 ms; checked after decoding; output rendering may finish later)\n  -h, --help           Read offline help\n  -V, --version        Print version\n";
const TARGETS: &str = "Targets:\n  UUID, positive legacy ID, window:N/workspace:N/pane:N/surface:N, or tab:N.\n  index:N selects a one-based position; reorder/move INDEX is zero-based.\n  Selector options constrain ownership where accepted; consult command usage.\n  --tab aliases --surface. Workspace navigation accepts --window and --workspace.\n  --expect-revision <N> checks an observed model_revision before the operation.\n  Place selector options before positional arguments; -- preserves literal text.\n  new-split, focus-direction and tab-action also allow options after their verb.\n  --window constrains an object's actual owner, including an inherited caller;\n  it never silently retargets an object to another window's selection. Explicit\n  workspace/pane/surface selectors override inherited object defaults.\n  ZMUX_WINDOW_ID is a hint only; malformed/stale hints are ignored. Workspace metadata\n  uses ZMUX_WORKSPACE_ID with caller surface ownership checked, and allows\n  options after values; consult its command help. For other target commands,\n  ZMUX_SURFACE_ID (or ZMUX_TAB_ID), then ZMUX_WORKSPACE_ID, then focus applies.\n  Those surface callers follow tab moves. Closed/stale targets fail.\n  UUIDs remain stable within the live model; reopened terminals receive new UUIDs.\n";
const METADATA_TARGETS: &str = "Workspace metadata targets:\n  --window is a hard ownership constraint for the resolved workspace.\n  --workspace accepts a workspace UUID, positive ID, workspace:N or index:N.\n  --expect-revision <N> checks an observed model_revision before the operation.\n  Explicit --workspace overrides inherited caller defaults. Otherwise,\n  ZMUX_WORKSPACE_ID selects the workspace, and any ZMUX_SURFACE_ID\n  (or ZMUX_TAB_ID) must belong to it. A moved or mismatched caller fails.\n  Without a caller workspace, the caller surface or current focus applies.\n  Closed/stale targets fail. Pane/surface selector flags are unavailable.\n  Options may appear before or after values; -- preserves literal text.\n";
const PR_TARGETS: &str = "PR targets:\n  --workspace accepts a workspace UUID, positive ID, workspace:N or index:N.\n  Otherwise a live ZMUX_WORKSPACE_ID and/or ZMUX_SURFACE_ID (ZMUX_TAB_ID)\n  or managed tmux caller must prove the owner. No foreground-focus fallback.\n  --window constrains ownership; --expect-revision checks the observed model.\n  Options may precede or follow the single value. Resolved workspace/window\n  UUIDs remain pinned through lookup; failures preserve the previous link.\n";
const TODO_TARGETS: &str = "Todo targets:\n  --workspace accepts a workspace UUID, positive ID, workspace:N or index:N.\n  Otherwise a live caller workspace/surface or managed tmux caller must prove ownership.\n  No foreground-focus fallback; --window alone cannot select a workspace.\n  --window constrains ownership; --expect-revision checks the observed model revision.\n  Options may precede or follow operands; -- preserves literal text.\n  Item UUIDs are stable within the live workspace; positive item positions are one-based.\n  --json is accepted explicitly; results use JSON by default.\n";
fn metadata_command(name: &str) -> bool {
    matches!(
        name,
        "pr" | "set-status"
            | "clear-status"
            | "list-status"
            | "set-progress"
            | "clear-progress"
            | "log"
            | "clear-log"
            | "list-log"
    )
}
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
            out.push_str(if spec.name == "todo" {
                TODO_TARGETS
            } else if spec.name == "pr" {
                PR_TARGETS
            } else if metadata_command(spec.name) {
                METADATA_TARGETS
            } else {
                TARGETS
            });
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
pub(super) fn catalog() -> Result<Value, CliError> {
    let (leaves, coverage) = contracts::validated()?;
    Ok(
        json!({"schema_version": 1, "contract_schema_version": 1, "parity_status": "incomplete", "version": env!("CARGO_PKG_VERSION"), "commands": COMMANDS.iter().map(|s| json!({
        "name": s.name, "aliases": s.aliases, "group": s.group, "usage": s.usage, "description": s.description,
        "targeted": s.targeted, "socket_required": s.socket,
        "socket_required_for": match s.name {"local-tmux"=>Some(vec!["gui_start","gui_attach","operation_status"]),"tmux"=>Some(vec!["gui_attach"]),_=>None},
        "availability": "implemented", "availability_scope": "dispatcher_only",
        "argument_contract_status": if leaves.iter().any(|leaf| leaf["dispatcher"] == s.name) {"partial"} else {"unresolved"}
    })).collect::<Vec<_>>(), "global_arguments": contracts::globals(), "contracts": leaves,
      "frozen_coverage": coverage, "error_contract": contracts::errors()}),
    )
}
pub(super) fn check_catalog() -> Result<Value, CliError> {
    let (leaves, coverage) = contracts::validated()?;
    Ok(
        json!({"schema_version": 1, "status": "passed", "parity_status": "incomplete",
        "checks": ["registry_aliases", "actual_parser_probes", "frozen_inventory_ids_and_spellings", "live_contract_references"],
        "contract_count": leaves.len(), "inventory_entries": coverage["entries"].as_array().map(Vec::len),
        "compatibility_verified": false}),
    )
}
pub(super) fn guide() -> String {
    format!(
        "# zmux automation guide\n\nUse `zmux commands` to discover this build's executable commands.\nRead `zmux help <command>` or `zmux help <topic>` before constructing requests.\n\nInspect with list-workspaces, list-panes and list-pane-surfaces. Preserve returned\nUUIDs when later requests must address the same object. Use leading selectors for\nexplicit scope; inherited terminal context applies when selectors are omitted.\nUse --expect-revision for operations that depend on a previously observed index.\n\nSend exact text with send-text; include carriage return only when submission is intended.\nUse --stdin for multiline text. Never retry a timeout whose outcome is unknown.\nObserve readiness after new-workspace, new-surface, new-split or reopen-closed: launch is asynchronous.\nReopening ordinary terminals starts fresh shells at saved working directories and does not replay jobs. Local-tmux terminals reattach their saved immutable owner identity.\n\nSession save, restore and export return an operation_id for asynchronous work.\nPoll session-status --id ID until its receipt completes or fails before using a saved file.\nRestore requires an empty desktop. Ordinary/SSH terminals start fresh shells without replaying commands; local-tmux terminals reattach their saved owner identity.\n\n{}\n{}",
        GLOBAL, TARGETS
    )
}
