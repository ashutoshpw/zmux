# Local and managed SSH provider lifecycle

Zmux can follow interactive Codex and OpenCode runs started by name in a local
or managed SSH terminal. It attaches lifecycle state to that exact existing
terminal. It does not create a terminal, send prompts, approve permissions, or
install a global provider hook or plugin.

OSC 7501 program status is a separate, self-reported source and does not
authenticate or create provider identity. Per-command alert source arbitration
does not match provider and OSC messages across protocols; generic OSC 9/777
notices retain their existing path. See the [OSC 7501 guide](program-status.md).

Managed shell startup transfers its trusted bootstrap as a bounded gzip and
Base64 payload. The target host needs `gzip` and `base64` (`-d` or BSD `-D`) to
enable integration; if either utility is unavailable, Zmux prints a diagnostic
and starts the original shell arguments without integration. Decoding does not
add a Python 3 requirement.

Tracking starts only after the shell integration verifies the terminal's exact
Zmux route. This associates evidence with the terminal, but does not
authenticate the provider process. Unsupported commands and failed preflight keep their original
arguments and launch once without tracking. Missing, stale, ambiguous, or lost
provider evidence is shown as Unknown. Completion sounds require a live start
and provider-confirmed successful completion; waiting sounds require a live
needs-user event. Sound and notification preferences remain on the local Mac,
including for managed SSH agents.

## Codex

The adapter attaches to Codex's already-running shared app-server daemon. A
session-scoped `SessionStart` hook supplies the opaque thread ID; Zmux reads
that exact thread and its turns and then follows its daemon notifications. It
does not start or resume a thread. A thread must be a root thread, and its
working directory or terminal output is never used as a substitute for its ID.

The adapter adds one `SessionStart` command to the effective hook list for that
launch. Existing Codex hooks are retained. Codex may require review of the new
hook in `/hooks`; tracking remains Unknown until the reviewed hook reports the
thread ID. Zmux does not return a permission decision from this hook, replace
the user's approval policy, or use `Stop` hooks as completion evidence. A
command that overrides hooks or bypasses hook trust runs once without
tracking. Launches selecting `--cd`/`-C` or `--profile`/`-p` also run
unchanged without tracking, because the read-only config interface cannot
reproduce the invocation's selected context reliably. Start Codex from the
chosen directory without a profile override to use automatic tracking.

An existing daemon and a supported read-only app-server interface are required
for tracking. If the daemon is absent or cannot read the exact thread, Codex
still launches once and Zmux reports that tracking is unavailable. The
installed Codex 0.166.0 daemon was checked without starting a model turn; this
does not prove notification delivery for an authenticated live turn.

## OpenCode

Before tracking, Zmux checks that the installed CLI advertises `--standalone`.
It then launches that invocation with a private config directory containing a
uniquely named lifecycle plugin. If the user set `OPENCODE_CONFIG_DIR`, Zmux
links its existing config entries and plugins into that private directory;
OpenCode's documented config precedence continues to load global and project
configuration as well. Zmux does not create an `opencode.json` override or
change permissions, approvals, user plugins, or other provider settings.

The plugin binds to a root session created or viewed by that invocation and
checks the provider's session record. Explicit `--session` values must match
that session exactly. Events from other sessions and child sessions are
ignored. Session status, permission and form requests/replies, successful
execution, failure/interruption, and the experimental idle callback provide
the lifecycle evidence. A successful completion also requires no pending
permission/form request and no reported background work. Plain idle and the
deprecated `session.idle` event do not prove success.

If `--standalone` is unavailable, the plugin cannot be installed for the exact
invocation, or no root-session callback arrives, OpenCode runs once without
tracking. The installed OpenCode v2.2.1 plugin was loaded and observed an empty
root session against an isolated local server without provider credentials or
a model/API turn. Synthetic callback tests cover the live transitions; they do
not substitute for an authenticated provider run.

## Managed SSH

Provider state crosses only the managed terminal's existing SSH lifecycle
stream. Remote Codex and OpenCode helpers require Python 3. If it is missing,
the shell prints a setup message and launches the original command once without
tracking. OpenCode also needs the remote CLI to advertise `--standalone`.
Explicit remote commands and unmanaged SSH sessions are not wrapped.

The stream keeps its nonce, generation, ordering, 4 KiB frame limit, and
heartbeat sequence. Provider frames contain only provider kind, root scope,
opaque session/turn IDs, normalized state and transition, and an attachment
marker. Prompts, transcripts, tool inputs, environment data, and paths are not
sent. FIFO writes must also fit the remote host's atomic pipe limit; an
oversized frame reports lost evidence before writing. A known dropped event ends the stream through a gap; a lost stream leaves
the agent Unknown. Provider snapshots refresh the currently observed state,
while attachment and recovery snapshots cannot create a live turn or completion
eligibility.

## Setup and troubleshooting

- Start Codex or OpenCode by typing `codex` or `opencode` in an integrated
  Zmux terminal. Headless commands, provider subcommands, and explicit remote
  commands keep their normal launch behavior without lifecycle tracking.
- For Codex, review the Zmux `SessionStart` command in `/hooks`. Zmux does not
  bypass Codex's hook trust controls. Until the hook runs and the existing
  daemon confirms the exact root thread, the sidebar stays Unknown.
- For OpenCode, look for the invocation plugin in startup output. If it does
  not report a root session, the terminal explains that plugin loading or
  provider startup needs review; the app does not infer state from cwd or
  output text.
- After an adapter, daemon, or SSH stream failure, state becomes Unknown. Start
  a new provider invocation to create a new observation generation. Reopening the same persisted provider session
  after normal exit also uses a fresh generation, retaining bounded alert
  history and rejecting retired generations.

See [SSH workspaces](ssh.md) for managed terminal setup and
[the provider lifecycle contract](../.agents/tasks/2026-11-01/01-live-agent-sidebar/references/provider-lifecycle-contract.md)
for normalized state, identity, ordering, and alert rules.

## References

- OpenAI Codex [app-server](https://learn.chatgpt.com/docs/app-server) and
  [hooks](https://learn.chatgpt.com/docs/hooks) documentation.
- OpenCode [plugin](https://opencode.ai/docs/plugins/) and
  [config](https://opencode.ai/docs/config/) documentation.
- [Provider adapter implementation notes](../.agents/tasks/2026-11-01/01-live-agent-sidebar/references/provider-adapters.md).
