# Codex and OpenCode adapters

**Updated:** 2026-11-02

## Adapter boundary

The provider wrappers run only from an integrated interactive local shell or a
managed SSH shell. Local ingestion first verifies the exact existing terminal
route and process ancestry. Managed SSH observations travel over that terminal's
existing nonce- and generation-bound lifecycle stream. Neither adapter selects
a terminal from cwd, title, provider output, or a candidate list.

This binds adapter evidence to the terminal, but does not authenticate the
provider process. Managed SSH frames are sanitized claims from the account and
terminal reached by that exact SSH binding.

Both adapters emit the shared normalized provider contract in
[provider-lifecycle-contract.md](provider-lifecycle-contract.md). Root session
and turn IDs stay opaque UTF-8 values (maximum 256 bytes); the desktop creates
its UUID compatibility projection. Every active observation has an exact turn
ID. Provider prompts, transcripts, tool inputs, environment data, arbitrary
payloads, and filesystem paths are excluded.

## Codex

Codex launch injects one `SessionStart` command into the effective hook list for
that invocation. Existing `SessionStart` commands are retained. User-supplied
hook overrides, explicit `--cd`/`-C` or `--profile`/`-p` selections, unsupported
CLI options, `--no-daemon`, remote modes, and
`--dangerously-bypass-hook-trust` keep the original provider command and run it
once without tracking. The hook only reports the opaque `session_id`; it does
not return an approval decision or use `Stop` as completion evidence. The user
must review Zmux's hook in Codex `/hooks` when Codex asks for trust.

The observer attaches to the already-running shared app-server. It never calls
`thread/start`, sends a prompt, or creates a separate thread. It verifies that
the SessionStart thread has no parent, reads that exact thread and its newest
turn, consumes notifications for that thread only, and polls a bounded status
snapshot every five seconds. Approval/input flags mean Waiting; an in-progress
turn means Working; only a `turn/completed` notification with status `completed`
is successful completion. `failed` and `interrupted` stay distinct. Periodic
source snapshots are Live+Snapshot and can refresh state without creating a
turn transition or completion eligibility. Attach and recovery snapshots are
Baseline/Recovery and remain silent.

The read-only app-server implementation has bounded WebSocket frames (256 KiB),
bounded queued notifications (128), a two-second RPC deadline, and a 100-byte
Unix socket path limit. If hook trust, daemon attachment, exact-thread reads,
or notification delivery fail, Zmux reports Unknown and does not retry the
provider launch. Installed Codex 0.166.0 was probed read-only without starting
a model turn; that daemon had no loaded thread, so it did not prove live
notification delivery.

## OpenCode

Before an automatic local launch, Zmux runs a bounded `opencode --help` probe and
requires an option declaration for `--standalone`. Managed SSH uses the same
check through Python 3. If the option is absent or help cannot be verified, the
original command is launched once without tracking. There is no retry after a
provider process has spawned.

For a tracked invocation, the adapter adds `--standalone` and creates a private
invocation config directory with one uniquely named plugin. When the user has
an explicit `OPENCODE_CONFIG_DIR`, its entries are symlinked into the private
directory and existing plugins are linked into its plugin directory. The
user-owned directory is never written. The temporary directory contains no
`opencode.json` override. OpenCode's documented load order continues to include
global and project configuration; the custom config directory is an additional
source. Random plugin naming avoids replacing an existing plugin.

The plugin listens to the provider event API. It binds only a root session
created/viewed by the standalone invocation, validates it with
`context.session.get`, and enforces an explicit `--session` ID when supplied.
Foreign roots and children are ignored. `session.status` busy/retry/idle,
permission callbacks, forms used for questions, prompt hooks, execution
outcomes, and `experimental.idle` supply lifecycle data. Completion requires
successful execution, matching idle confirmation, empty background work, and
no unresolved permission/form request. Plain idle, deprecated `session.idle`,
failure, interruption, or a second uncorrelatable prompt never reports success.

An ordered queue is capped at 64 observations; OpenCode session history and
pending wait identities are capped at 128. The plugin emits a current Live
Snapshot every five seconds while it has a known state. Queue overflow, an
invalid event, or a failed sidechannel write marks evidence lost; it does not
discard events and continue as if state were current.

The installed OpenCode v2.2.1 plugin was accepted by the real standalone server
and observed an empty root session through the actual plugin event subscription,
using synthetic local server authentication only. No provider credentials,
prompt, or model turn were used. A callback host drives synthetic prompt,
permission, form, idle, success, interruption, and foreign-root cases. Those
fixtures test provider event normalization, not authenticated model behavior.

## Managed SSH producer

The remote shell needs Python 3 for managed Codex and OpenCode tracking. The
shell prints a specific requirement message and launches the original command
once without tracking when Python 3 is unavailable. Codex uses its exact
SessionStart thread ID and attaches only to the existing daemon. OpenCode uses
an invocation config directory and the same exact-root plugin.

The plugin writes sanitized provider observations to the private
`agent-events` FIFO. The existing lifecycle stream emits protocol v3
`provider_state` frames in the same global sequence as Claude hooks and
heartbeats. Each newline-delimited frame stays under 4096 bytes. A queue loss or
sidechannel error writes to `agent-events-loss`; the stream emits `gap` and
ends, so remote activity becomes Unknown. The plugin's five-second Live
Snapshot is provider evidence; the independent stream heartbeat only refreshes
transport connection. This keeps a valid long-running provider state current
without inventing a transition.

## Owner-boundary coverage

| Boundary | Observable risk covered |
| --- | --- |
| Local CLI with a real Desktop terminal | Exact terminal/session binding, root filtering, opaque IDs, waiting/completion/interruption, no Stop decision, user config/plugin preservation, and one launch when standalone preflight fails |
| Managed loopback SSH | Real managed shell/FIFO/frame path, exact root filtering, provider lifecycle sequence, user config/plugin preservation, one fallback launch, and a Working turn that remains Connected for more than 30 seconds through source snapshots |
| Shell integration plugin host | Actual plugin module callback surface, payload privacy, lifecycle transition mapping, and refusal to treat idle/background work as successful completion |
| Installed OpenCode v2.2.1 acceptance | Real plugin load and empty-root observation with no provider credentials or model/API turn |
| Remote lifecycle producer fixture | Ordered provider frames, heartbeat sequence sharing, malformed/drop gap, frame bounds, and absence of transcript/private event data |

Managed SSH tests require `sshd`, `ssh-keygen`, and Python 3. The OpenCode SSH
fixture additionally requires Node.js to host the synthetic plugin callbacks.

## External references

- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [Codex hooks](https://learn.chatgpt.com/docs/hooks)
- [OpenCode plugins](https://opencode.ai/docs/plugins/)
- [OpenCode config](https://opencode.ai/docs/config/)
- OpenCode v2.2.1 plugin implementation: <https://github.com/anomalyco/opencode/tree/v2.2.1>
- [Normalized provider lifecycle contract](provider-lifecycle-contract.md)

## Integration review corrections and regression gates

- Root Codex records may omit parentThreadId or set it null; only a non-null
  parent excludes a subagent. The shared synthetic daemon omits the optional
  field, exercising both actual local and managed-SSH observer paths.
- Context-changing Codex options and mcp pass through unchanged with one
  launch. The read-only config API cannot reproduce a selected profile, so
  the adapter does not replace a hook array resolved in a different context.
- A normally ended local provider session can reopen with a fresh runtime/
  generation pair, preserving bounded outcome histories and retired-pair
  fencing, just as recoverable evidence loss does.
- Local event reads have an absolute 200 ms frame deadline (not one timeout
  per byte); accepted sockets are explicitly blocking on Darwin too. This
  bounds listener join after provider exit despite a trickling peer.
- Python checks fpathconf(PC_PIPE_BUF) on the actual open FIFO descriptor.
  Node probes getconf once per FIFO identity with a 500 ms deadline, falling
  back to the POSIX 512-byte atomic guarantee; frames exceeding the atomic
  bound report loss before any write. This is additional to the 4096-byte
  protocol cap.

Regression authoring: these protect real launch argument/config preservation,
optional provider-schema fields, reopening an ended session and bounded
transport teardown/atomicity. Existing fixtures supplied parent=null, never
selected a profile/directory, never resumed a disconnected session and used
only complete frames on hosts with large pipe limits. Tests extend the existing
production CLI/control/SSH boundaries; the reader test runs its real socket
worker. The Node plugin test supplies a synthetic getconf result for the POSIX
minimum and checks actual FIFO loss versus event bytes. No production export,
test flag or test-only injection API was added.

## macOS transport-path correction

The exact session root remains required and held during setup. The separate
local provider event socket and its invocation-only plugin/config files use a
fresh private directory under canonical /tmp, rather than nesting under the
possibly long TMPDIR-derived session root. This keeps the Unix socket within
Darwin SUN_LEN without changing the user's temporary-directory preference or
provider configuration. Existing actual Desktop/Codex coverage now runs with a
long TMPDIR; it reproduced event-route fallback before this correction and
passes afterward. The fake daemon WebSocket fixture uses the same short-parent
rule. Forced termination can still retain private adapter files for review;
normal teardown removes only the invocation's owned directory.
