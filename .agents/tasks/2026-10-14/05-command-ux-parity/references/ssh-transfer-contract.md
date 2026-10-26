# Managed SSH and transfer contract comparison

Reviewed 2026-10-15 against cmux `3ba653b9e5afb11d0bc578031142950d6136697e`
and zmux `e949e6c`. Behavior research only; no upstream implementation is imported.
This is a traced subsystem comparison, not completion of the whole product audit.

## Source-to-owner comparison

| Contract | cmux source and observed responsibility | zmux owner/gap | Required acceptance |
| --- | --- | --- | --- |
| SSH entrypoint | `CLI/cmux.swift:13634` runSSH parses options then starts a managed operation | `apps/cli/src/main.rs:176` dispatch has no SSH handler | CLI invocation creates a remotely identified workspace/surface and reports its identity; it cannot merely inject ssh text into an arbitrary shell. |
| Options/defaults | `CLI/cmux.swift:14444` parseSSHCommandOptions validates port, identity, name, command, window, focus, forwarding, transport and trailing argv | No parser or persisted remote descriptor | Table covers every supported option, missing values, conflicts, precedence, caller context and `--`; unknown options fail without spawning. |
| Auth/config routes | `CLI/cmux.swift:13851` runSSHWithOptions resolves SSH configuration, route-sensitive sharing and configured remote commands | `crates/session-runtime/src/lib.rs` Launch has only local program/args/cwd | Identity, port, config alias, jump/proxy route and host-key policy remain consistent across terminal and file operations; password auth is interactive and cancellation is bounded. |
| Native remote handoff | `CLI/CMUXCLI+SSHTui.swift:9` sends workspace.ssh.open with operation identity and retries after interactive auth | No corresponding ControlCommand or remote lifecycle | Repeated/failed authentication cannot create duplicate sessions; correct workspace gets focus unless explicitly disabled. |
| Remote ownership/reconnect | `CLI/CMUXCLI+SSHPTYAttachBridge.swift:147` reconciles bridge end; `CLI/CMUXCLI+SSHReconnectPrompt.swift` owns reconnect presentation | Local Session worker only; no remote daemon or attachment identity | Disconnect/reconnect preserves the declared remote session when available; resize/output replay/input ownership are verified on a second host. No local fallback execution. |
| Drop entrypoint | `Sources/GhosttyTerminalView.swift:10463` performDragOperation excludes internal drags then handles pasteboard content | `apps/macos/Sources/WorkspaceViews.swift:399` only moves tabs; TerminalView has no file-drop route | File URLs and image pasteboards work over terminal content without interfering with tab drags. |
| Content/target classification | `Sources/TerminalImageTransfer.swift:129`, `:242`; TerminalImageTransferTarget and TerminalImageTransferPlan distinguish text, local, workspace remote, detected SSH and cloud | Only text clipboard paste exists | Plain text remains text; local files insert local paths; remote files upload first; image bytes become bounded temporary files; unavailable remote targets never insert local paths. |
| Responsive preparation | `Sources/TerminalImageTransferPreparationService.swift:214`, `:384`, `:388` queues preparation with cancellation/deadlines | No preparation service | Image decoding and file IO do not block AppKit; oversized/malformed images and canceled preparation clean up their temporary resources. |
| Transfer routing | `Sources/Workspace.swift:7158` routes remote upload; `Sources/RemoteTui/DetectedSSHSession+Workspace.swift:6` constructs endpoint from remote configuration | No transfer worker/remote route | Transfer uses the same authenticated endpoint/options as the terminal, including password-authenticated shared connections. |
| File-operation connection reuse | `Sources/RemoteTui/DetectedSSHSession+Workspace.swift:28` selects file-operation SSH options | No connection-sharing identity | Files reach the actual remote destination through configured proxy/jump/identity; simultaneous hosts cannot share the wrong credentials or connection. |
| Progress/cancel/error | `Sources/GhosttyTerminalView.swift:10229` executeImageTransferPlan; `Sources/TerminalImageTransfer.swift:95` cancellation; TerminalUploadFailureNotification | No transfer presentation/model | UI shows pending/progress/completion or failure; cancel stops the worker and suppresses insertion; partial/failure state is observable. |
| Completion delivery | `Sources/GhosttyNSView+TerminalCustomUpload.swift:11` deliverUploadResultText captures originating surface; transfer plan finishes with remote paths | No operation/surface generation binding | Switching tabs, closing, moving or reconnecting during upload cannot insert paths into another terminal. Successful paths are shell-quoted; no command is submitted implicitly. |
| Clipboard/custom/cloud routes | `Sources/TerminalImageTransfer.swift:294` execute; TerminalCustomUploadRunner; GhosttyApp+TerminalCustomUpload | No equivalents | Audit paste/drop variants, custom upload rules and cloud behavior separately; a working file drop does not close image/clipboard/custom/cloud rows. |

## Delivery order correction

Managed SSH and file/image transfer are a single user journey, with independently validated
commits underneath it. Implement the typed remote descriptor and shared action first,
then connection/PTY lifecycle, then transfer/preparation workers, then native drop/paste,
progress/cancellation and origin-bound insertion. Preserve remote behavior for new tabs,
splits, closure and reconnect; a one-command SSH wrapper does not establish parity.

Ordinary SSH and SSH file transfer do **not** depend on zmux Cloud deployment. The cloud
provider/database discussion blocks hosted cloud journeys only. Full transport parity
still includes mosh, remote tmux, attach/list/cleanup and session move; none is waived.

## Required independent verification

- Real SSH server with host-key acceptance/rejection, key and password auth, config aliases,
  explicit ports/identities, proxy/jump routes and forwarding options.
- Drop multiple files, Unicode/space/quote filenames, directories according to the source
  contract, actual image files, raw clipboard images, unsupported/oversized content.
- Verify remote file bytes and resulting inserted paths, not just a successful subprocess.
- Slow transfer while typing/resizing other terminals; cancel, disconnect, permission denied,
  target closed/moved/reconnected, partial transfer and cleanup.
- Native packaged app tests plus two-host reconnect/stress evidence. Local mocks and a
  downloadable app alone do not establish this journey.

## Audit process correction

The earlier 428-command and 171-shortcut lists remain discovery inventories. They must not
be cited as an exhaustive behavior review. Each family needs this level of tracing: entry,
shared owner, side effects, focus/identity, persistence, errors, platform dependencies and
behavioral evidence. Non-CLI paths (menus, pasteboards, drag/drop, settings, background
callbacks and recovery) need their own rows linked to the same owners.

No family is complete because one command or shortcut works. Partial delivery is allowed
for the requested incremental commits, but full parity remains an unmet acceptance gate.
Only explicit user scope changes can remove requirements. A missing implementation is
work to do, not a scope exception.
