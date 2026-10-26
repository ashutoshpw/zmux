# Agents, remote sessions, attention, and automation parity audit

Source-only audit on 2026-10-15. Baselines: cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`; zmux
`69342a8f036037736c6eb9a500e479ac41e32d93` (`main`). This extends the bounded
`ssh-transfer-contract.md` trace with the adjacent reconnect, relay, multiplexer,
agent, execution-exchange, notification, metadata, event, and automation owners. It
does not import or translate upstream implementation.

## Result

The machine-readable ledger records 38 feature families: 34 missing, 2 partial,
and 2 verified subsets. No family is classified as fully verified. The two tested
zmux subsets are the bounded in-app notification lifecycle and its focus/read
acknowledgement behavior. Text sending and local surface movement are useful
building blocks, but they do not implement paste/upload or agent-session movement.

| Area | Result | Source-backed implication |
| --- | --- | --- |
| Managed remote | Missing except local primitives | zmux launches local programs through one PTY owner (`crates/session-runtime/src/lib.rs:60`, `:177`) and exposes no `ssh`, `mosh`, remote-tmux, session attach/cleanup, or relay commands in its dispatcher (`apps/cli/src/main.rs:176`). |
| Device registry | Missing | zmux Cloud sign-in is not cmux's `remotes` team device registry; there is no route validation, idempotent add-by-name, or remove-by-name/device-ID owner. |
| Paste and transfer | Partial | Native Cmd-V wraps text in bracketed-paste markers and otherwise translates newlines (`apps/macos/Sources/TerminalView.swift:157`), but CLI paste, upload/remote transfer, target/draft guards, progress/cancel, limits, and generation-bound delivery are absent. |
| Session movement | Partial | `move-surface` moves a live local tab between panes (`apps/cli/src/main.rs:379`; `crates/workspace-core/src/lib.rs:205`). It does not carry agent transcript/history, working-tree state, or reconnect identity between local and SSH endpoints. |
| Portable sessions | Missing | `recently-closed` is process-local shell/layout recovery, not `session save/open` with portable snapshot trust, pruning and collision rules. |
| Agents and hooks | Missing | zmux has no agent command namespace, TextBox composer, managed team/integration launchers, CodeRouter/Feed owner, or typed hook/message/hibernation/vault owner. Its attention records are notifications, not durable agent messages. |
| Attention inbox | Verified subset | zmux bounds and sequences bell/OSC/explicit notifications (`crates/desktop-runtime/src/attention.rs:7`, `:48`), routes open/read/dismiss/clear, and tests focus/generation behavior (`crates/desktop-runtime/tests/desktop.rs:518`). |
| Native notification delivery and actions | Missing | zmux updates an in-app inbox and Dock badge (`apps/macos/Sources/App.swift:765`, `:781`) but has no `UNUserNotificationCenter` authorization/delivery/action/reply owner. |
| Status/PR/progress/log | Missing | `zmux status` is an app/topology snapshot (`crates/desktop-runtime/src/control.rs:472`), not cmux's surface-scoped status, pull-request, progress or log metadata channels. |
| Events/automation/todo | Missing | No dispatcher, persisted rule store, replay cursor, event stream, workspace todo model, or generic integration-hook installer exists in zmux source. |

## Inventory coverage map

This table maps the original/top-level inventory rows in this scope to the normalized
feature IDs in the JSON ledger. It keeps user commands separate from internal or legacy
compatibility helpers.

| Inventory rows / non-CLI entrypoints | Ledger feature | Classification note |
| --- | --- | --- |
| `ssh` | `remote.managed-ssh` | Supported user command; distinct from manually typing `ssh` in a local terminal. |
| `mosh`, `mosh-tmux` | `remote.mosh` | Supported user commands sharing the SSH route and fallback owner. |
| `ssh-tmux` | `remote.tmux` | Supported list/attach/mirror command family. |
| `tmux`, `local-tmux`, `local-zellij` | `remote.local-multiplexers` | Supported managed local multiplexer commands. |
| `ssh-pty-attach`, `ssh-session-list`, `ssh-session-attach`, `ssh-session-cleanup`, `ssh-session-end`, `remote-daemon-status` | `remote.reconnect` | Hidden/internal lifecycle helpers used by managed SSH; not independent parity shortcuts. |
| remote v2 forwarding and relay authorization | `remote.cli-relay` | Non-CLI security boundary reached by managed SSH. |
| `remotes`, `remote list|add|remove` | `remote.device-registry` | Team device routes; distinct from AI accounts and managed SSH. |
| native file/image drop, clipboard image paste, custom upload, `pasteLastScreenshot` (`UX-ACTION-085`) | `remote.file-image-upload` | Non-CLI AppKit/pasteboard entrypoints. |
| `paste`, `send --paste`, Cmd-V | `remote.paste` | User CLI plus native paste entrypoints that must share one target-bound owner. |
| agent/session move | `remote.agent-session-move` | Agent state transfer; distinct from local UI `move-surface`. |
| `session save <path>`, `session open <path>` | `session.file-save-open` | Explicit portable file export/import; distinct from crash restore and recently closed. |
| `agent`, `restore`, `fork`, `surface-resume` | `agent.launch-resume`, `agent.checkpoints` | `surface-resume` is a supported binding-management command; restore/fork consume typed evidence. |
| `focusTextBoxInput`, `cycleTextBoxSubmitAction`, `attachTextBoxFile` (`UX-ACTION-081..083`) | `agent.launch-resume` | Native agent composer/input layer: exact-panel focus, launch action, attachment and failed-submit ownership. |
| `claude-teams`, `codex-teams` | `agent.team-launchers` | Managed provider/team launch and native subagent projection. |
| `omo`, `omx`, `omc` | `agent.integration-launchers` | Managed wrappers with private tmux compatibility routing. |
| `agent message|msg`, `agent messages` | `agent.messages` | Durable agent-directed messaging and settings. |
| `agent inbox`, `agentInbox` shortcut (`UX-ACTION-025`), and inbox hook helpers | `agent.inbox-wakeup` | CLI inspection, feature-gated native quick view/reply, and internal claim/lease/wake delivery. |
| `claude-hook`, `hooks`, agent provider hook subcommands | `agent.hooks` | `claude-hook` is an explicit compatibility entrypoint with direct help; `hooks` is canonical. |
| `codex-hook`, `feed-hook` | `agent.hooks` | Hidden backwards-compatibility entrypoints, not new public families (`CLI/cmux.swift:9741`). |
| `agent-hibernation`, `agent hibernate|wake` | `agent.hibernation` | User policy and exact-surface actions. |
| `ai-accounts` | `agent.ai-accounts` | AI provider accounts; distinct from zmux Cloud user/device auth and the `remotes` device registry. |
| `coderouter status|machines|claude|agent` | `agent.coderouter-control` | cmux-owned team/model-plane control verbs and safe credential intake. |
| `cr`, bare or unowned `coderouter` verbs | `agent.coderouter-passthrough` | External CodeRouter CLI passthrough/bootstrap; distinct from cmux-owned verbs. |
| configured external launcher registry | `agent.external-launchers` | Non-CLI launch/resume routing owner. |
| restore evidence, recovery/checkpoint owners | `agent.checkpoints` | Agent/session evidence, not VM snapshots or recently-closed UI shells. |
| `vault`, `recover`, `session restore` | `agent.vault`, `agent.checkpoints` | Persisted session discovery and typed recovery. |
| `feed tui`, `feed clear` | `agent.feed-console` | Interactive workstream/decision console and its persisted-history lifecycle. |
| `notify`, `list-notifications`, `dismiss-notification`, `mark-notification-read`, `open-notification`, `clear-notifications` | `notification.lifecycle` | Supported human notification lifecycle. |
| `jump-to-unread`, `jump-to-last-prompt`, focus callbacks, Dock/sidebar badges | `notification.attention-focus` | Navigation and acknowledgement layer. |
| desktop delivery permission | `notification.os-delivery-actions` | Non-CLI OS effect. |
| native click/reply/custom actions | `notification.actions` | OS response routing is tracked separately from delivery permission. |
| `set-status`, `clear-status`, `list-status`, `report_pwd`, `report_git_branch`, `report_pr_action` | `sidebar.status` | The `report_*` rows are metadata reporters, not standalone product domains. |
| `pr <url|number>`, `pr clear` | `sidebar.pull-request` | Explicit repository-validated manual PR handoff; distinct from generic status and the internal action hint. |
| `set-progress`, `clear-progress` | `sidebar.progress` | High-frequency metadata channel. |
| `log`, `clear-log`, `list-log` | `sidebar.log` | Bounded target-scoped metadata log. |
| `events` | `events.stream` | Supported authenticated stream command. |
| `glaeda request|observe` | `execution.glaeda-exchange` | Offline caller-neutral execution document exchange with no UI/focus owner. |
| `automation` | `automation.rules` | Supported rule-management and dry-run family. |
| `todo` | `workspace.todo` | Supported workspace todo namespace. |
| `setup-hooks`, `uninstall-hooks`, `codex install-hooks|uninstall-hooks` | `integration.hooks` | Local hook configuration commands; `codex` is only this compatibility namespace (`CLI/cmux.swift:23350`). |

## Remote command and option gaps

cmux treats an SSH connection as a typed, restorable remote workspace. `runSSH`
creates a relay identity/token before parsing (`CLI/cmux.swift:13634`), and
`runSSHWithOptions` resolves ssh_config, route-sensitive connection sharing,
configured remote commands, TTY policy, bootstrap, and focus (`CLI/cmux.swift:13851`).
The parser accepts destination plus `-p/--port`, `-i/--identity`, workspace name,
initial/remote command and trailing argv, window, focus/no-focus, agent forwarding,
repeated `--ssh-option`, transport/profile/session selection, and TTY semantics
(`CLI/cmux.swift:14444`; `CLI/CMUXCLI+SSHCommandOptions.swift:17`). zmux has none
of these options or a persisted remote descriptor.

Mosh is a transport choice with capability probes and explicit SSH fallback,
including local missing/unsupported, remote missing/probe failure, bootstrap failure,
and address fallback (`CLI/CMUXCLI+MoshTerminalTransport.swift:5`, `:65`). Remote
tmux separately supports list/attach/mirror/new-window, transport/broker settings,
bounded interactive-auth retry, progress reporting, caller-window routing, and
focus defaults (`CLI/cmux.swift:13668`, `:13768`). Local tmux and zellij have
managed start/attach/list/status/detach/close/cleanup lifecycles rather than merely
allowing users to type those programs in a shell (`CLI/CMUXCLI+LocalTmux.swift:8`;
`CLI/CMUXCLI+LocalZellij.swift:51`).

Reconnect is identity-bearing. The hidden attach/session commands preserve lifecycle
IDs, distinguish retryable bridge failures from lost sessions, and expose list,
attach, cleanup, end, and daemon status (`CLI/cmux.swift:8584`, `:16965`, `:17383`,
`:17915`). Acceptance must prove replay boundaries, resize/input ownership, no
duplicate session after auth/retry, and no local execution fallback.

The remote CLI relay is an authorization boundary, not a generic socket tunnel.
`RemoteRelayCommandPolicy` denies by default, scopes workspace/surface selectors,
and rejects command-bearing parameters
(`Packages/macOS/CmuxRemoteWorkspace/Sources/CmuxRemoteWorkspace/Relay/RemoteRelayCommandPolicy.swift:9`,
`:20`, `:25`, `:268`). Any zmux implementation needs owned-object alias maps and
policy tests before forwarding a single method; terminal spawn/respawn remains
blocked until live proof shows execution occurs on the remote host.

The device registry is another independent remote domain. `remotes`/`remote`
lists, idempotently adds by name, and removes by name or device ID through one
registry client (`CLI/CMUXCLI+Remotes.swift:4`, `:60`, `:86`, `:127`). Add requires
one or more `--route` values and validates them as Tailscale numeric peers or
authenticated MagicDNS peers before the server-side trust boundary; `--tag` is
optional (`CLI/RemotesArgumentParser.swift:8`). It is neither an AI account store
nor proof that managed SSH exists.

## Transfer, paste, and movement contracts

The prior SSH transfer trace remains the detailed file/image contract. zmux does have
a basic native text-paste subset: Cmd-V reaches `pasteFromClipboard`, which wraps text
in bracketed-paste markers when enabled and otherwise translates newlines
(`apps/macos/Sources/TerminalView.swift:157`; `apps/macos/Sources/App.swift:841`). This
source-only audit did not run that path. CLI paste and native paste/drop still need to
converge on a shared, target-bound write path. cmux resolves
window/workspace/surface identity, checks agent draft/dialog state, preserves bracketed
paste, accepts stdin, enforces the 15 MiB encoded socket limit, and reports the
non-retryable “pasted but submit failed” case
(`CLI/CMUXCLI+Paste.swift:81`, `:129`, `:154`, `:258`, `:318`). Native transfer
classifies local versus detected-SSH/remote targets, prepares content off the UI path,
uploads before inserting shell-quoted paths, and binds completion to the originating
surface (`Sources/TerminalImageTransfer.swift:95`, `:129`, `:242`;
`Sources/GhosttyTerminalView.swift:10229`, `:10463`).

`move-surface` in zmux only changes local topology. cmux's agent move owner refuses
same endpoints, unsafe SSH targets, live source/destination sessions, missing
transcripts, dirty/diverged destinations, and absent mapped working directories before
copying code/session artifacts (`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/AgentMove/AgentSessionMover.swift:40`,
`:64`, `:71`, `:110`). These are separate acceptance gates.

Portable session files are separate from both move and crash recovery. `session
save/open` dispatches to socket methods only for those explicit verbs
(`CLI/CMUXCLI+SessionMove.swift:38`; `Sources/TerminalController.swift:7513`, `:7532`).
Save requires an absolute path, excludes scrollback and refuses the live persistence
file. Open passes through snapshot trust and diagnostic pruning, caps restored windows,
and excludes live stable identities and workspace IDs before creating new windows
(`Sources/AppDelegate.swift:9994`, `:10024`). Acceptance also requires that import not
take focus unless the caller explicitly requests it.

## Agent ownership and delivery

The cmux agent surface is a set of coordinated owners:

- Typed launch/resume identity and external launcher matching live in
  `CMUXAgentLaunch` (`AgentLaunchCommand.swift:12`, `AgentExternalLauncher.swift:244`,
  `AgentSpawnIdentity.swift:46`). The native TextBox is the input side of the same
  ownership boundary: focus toggles only the resolved terminal panel, submit actions
  can launch a provider or send to the active agent, attachments are retained across
  failed submission and cleaned after admitted delivery (`Sources/Panels/TerminalPanel.swift:343`;
  `Sources/TextBoxInput.swift:2540`; `Sources/TextBoxSubmitActions.swift:505`). zmux has
  no agent kind/session identity or equivalent composer beyond a local process ID.
- `claude-teams` validates managed context before creating its private tmux shim and
  exporting launch identity; `codex-teams` binds a localhost app-server, watcher,
  bounded depth and approval bridge to canonical surface identity (`CLI/cmux.swift:27011`,
  `:27028`, `:28179`, `:28216`). OMO, OMX and OMC have separate executable/plugin,
  port and compatibility-shim failures and preserve provider arguments
  (`CLI/cmux.swift:29303`, `:29460`, `:29584`).
- Messages support target or reply identity, sender surface/workspace attribution,
  threads, queued/delivered/read/failed states, per-surface/workspace/global enablement,
  and hook-based claim/lease/ack wake-up (`CLI/CMUXCLI+AgentMessages.swift:138`, `:184`,
  `:236`, `:295`; `CLI/CMUXCLI+AgentMessageSettings.swift:75`). The feature-gated
  Agent Inbox shortcut resolves its window, opens a quick view, preserves reply-field
  input ownership and gates duplicate reply submissions (`Sources/AppDelegate.swift:6191`,
  `:15509`; `Sources/AgentInbox/AgentInboxView.swift:304`).
- Hooks cover install/uninstall/setup plus agent-specific event payloads and failure
  reporting. Hibernation targets a resolved workspace/surface and has explicit on/off,
  hibernate, and wake flows (`CLI/CMUXCLI+AgentHibernation.swift:13`).
- AI accounts/remotes, external launchers, checkpoints/recovery, and the vault are
  distinct stores and commands. A signed-in zmux cloud account is not parity with
  cmux AI account discovery or agent-session recovery.

Agent-message delivery must not be conflated with the existing notification inbox.
Notifications alert a human and acknowledge focus; messages are durable agent-directed
work with target resolution, delivery state, polling/lease ownership, and reply/thread
identity.

`claude-hook` has a direct help contract and command suggestion
(`CLI/cmux.swift:23327`; `CLI/CMUXCLI+CommandSuggestions.swift:72`), while the contract
describes it as a compatibility alias (`docs/cli-contract.md:212`). `codex-hook` and
`feed-hook` are explicitly hidden backward-compatibility dispatch paths
(`CLI/cmux.swift:9741`). The canonical integration family is `hooks`.

## CodeRouter, Glaeda, and Feed

CodeRouter has two owners. The cmux-owned `coderouter status|machines|claude|agent`
verbs expose sign-in/team state, 30-day machine usage, routed VM-agent launch and
Claude OAuth/API-key/Bedrock lifecycle (`CLI/CMUXCLI+Coderouter.swift:4`, `:72`, `:93`,
`:136`, `:180`). Credential add reads environment, bounded stdin or a hidden prompt,
validates credential kind/prefix and never prints raw secrets
(`CLI/CMUXCLI+Coderouter.swift:246`). All `cr` invocations and unowned `coderouter`
verbs instead preserve external CLI behavior (`CLI/cmux.swift:6396`): they strip
`CMUX_*`/`CMUXD_*`, resolve bundled/PATH/install-directory binaries, offer interactive
bootstrap when missing, constrain installer download to HTTPS/TLS 1.2+, and preserve
child exit or signal status (`CLI/CMUXCLI+CoderouterPassthrough.swift:40`, `:57`, `:82`,
`:169`, `:226`). zmux has neither owner.

`glaeda request|observe` is an offline document exchange, not a cloud-session or UI
feature (`CLI/cmux.swift:6501`; `CLI/CMUXCLI+Glaeda.swift:10`, `:41`). Request requires
exact caller/work refs plus repository, commit and tree; observe correlates an exact
request with a bounded receipt (`CLI/CMUXCLI+Glaeda.swift:84`, `:130`). Its parser
rejects duplicate, unknown and missing options, while file reads reject symlinks,
nonregular files and oversized documents (`CLI/CMUXCLI+Glaeda.swift:163`, `:223`). The
JSON deliberately contains no workspace, machine, shell, environment or focus side
effects.

Feed is an agent workstream/decision surface rather than browser content. `feed tui`
selects OpenTUI, legacy, or automatic fallback; the OpenTUI path requires an
interactive terminal, prepares a pinned dependency under cmux-owned state, passes the
authenticated socket, transfers and restores the foreground process group, and reports
child failure (`CLI/cmux.swift:42824`, `:42872`). `feed clear` owns the persisted
workstream history and prompts unless `--yes`/`-y` was explicit (`CLI/cmux.swift:43821`;
`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamPersistence.swift:170`).
zmux's notification inbox is not a workstream store or approval/question console.

## Notifications and sidebar telemetry

zmux's strongest overlap is its in-app attention model. Bell, OSC 9/777, and explicit
notifications are distinct from ordinary output; history is bounded, stale clears
cannot consume newer generations, opening selects the exact surface, active/focused
surfaces auto-read, and the UI exposes inbox, badges, read/dismiss/clear actions
(`crates/terminal-adapter/tests/parsing.rs:93`;
`crates/desktop-runtime/src/attention.rs:48`, `:105`, `:114`, `:119`;
`apps/macos/Sources/App.swift:780`). This is a verified subset from source tests, not
runtime verification of the current macOS artifact.

cmux additionally resolves caller/window/workspace/surface targets, supports title,
subtitle, body, reply shape, desktop effect override, clear scopes, open/jump, all-read
dismissal, and read-by-id/workspace/surface/all (`CLI/cmux.swift:9386`, `:9519`,
`:9542`, `:9556`, `:9585`). Its central store rebinds notifications when surfaces move
and drives native notification authorization/delivery/actions
(`Sources/TerminalNotificationStore.swift:28`, `:1156`, `:1847`, `:2400`;
`Sources/AppDelegate.swift:18913`). zmux lacks native delivery and reply/action handling.

Status, progress, and log are separate surface/workspace metadata channels, with
set/clear/list/append operations routed by the control-socket sidebar coordinator
(`CLI/cmux.swift:9637`; `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Coordinator/Sidebar/ControlCommandCoordinator+SidebarV1.swift:93`). They must not be
implemented as notification bodies or confused with `zmux status`.

Manual pull-request metadata is another explicit sidebar owner. `pr <url|number>` and
`pr clear` resolve an explicit target, caller TTY, ambient workspace or unique registered
worktree without using mutable UI focus (`CLI/CMUXCLI+PullRequest.swift:4`, `:49`). The
lookup bounds `git`/`gh`, accepts only a canonical GitHub pull URL or positive number,
checks the detected repository or parent, and sends one escaped framed mutation
(`CLI/CMUXCLI+PullRequest.swift:282`, `:328`, `:339`). Ambiguous, cross-window, remote,
authentication, timeout, malformed and repository-mismatch outcomes fail without
changing the existing manual link.

## Events, automation, todo, and hooks

cmux events stream supports sequence cursors, cursor files, name/category filters,
reconnect, limit, monotonic timeout, snapshot/ack/heartbeat controls, authenticated
streaming, and transient-only reconnect (`CLI/CMUXCLI+Events.swift:23`, `:196`).
Automation owns list/show/test/enable/disable/logs/reload and an offline dry-run path
through the same rule matcher (`CLI/CMUXCLI+Automation.swift:4`, `:60`). Workspace todo
has a separate target-scoped CRUD/status model (`CLI/CMUXCLI+WorkspaceTodo.swift:50`,
`:109`). Generic hooks install/uninstall agent integrations and translate provider event
schemas; they are not aliases for event streaming or notification creation.

zmux has none of these owners. Acceptance requires bounded persistence, exact event and
target identity, failure-visible rule/action logs, idempotent hook setup/removal, and a
focus policy in which background automation does not select panes or activate the app.

## Evidence limits

This audit read source and source tests only. It did not build or run either app, connect
to an SSH/mosh/tmux/zellij host, exercise a remote relay, transfer bytes, save/open a
session file, launch a team/integration agent, install CodeRouter/OpenTUI, exchange
Glaeda documents, clear Feed history, request macOS notification permission, install
hooks, or validate a packaged artifact. “Verified subset” means the current zmux source
contains focused tests for that narrow behavior; it does not prove installed-app,
two-host, permission, reconnect, stress, or production behavior. Ordinary SSH and SSH
transfer are independent of cloud hosting. Cloud VM and service-provider implementations
remain outside this audit; their command/control contracts are included where listed.
