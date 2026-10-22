# Remote scheduler contract

This source-backed contract describes the persistent remote scheduler v1 shared
by the CLI, managed-SSH transport, native app and remote helper. A locally passing
implementation does not establish packaged-artifact or live-host availability.

## Current code boundary

- `apps/cli/src/targets.rs:287-303` parses immediate `send-text`, caps `--stdin`
  input at 4096 UTF-8 bytes, and submits a `TargetOperation::SendText` request.
- `crates/desktop-runtime/src/lib.rs:2791-2808` resolves that request to the
  exact Zmux terminal surface and passes its bytes to the local session owner. It
  does not inspect a remote tmux server or add Enter.
- `crates/remote-runtime/src/connection.rs:136-165` builds an auxiliary SSH
  operation over an existing private ControlMaster, with no SSH config or proxy
  fallback. This is the management transport for a live managed SSH surface.
- `crates/remote-runtime/src/local_multiplexer.rs:73-95` defines a local tmux
  identity. It is not a remote tmux target or remote scheduler identity.

The `--surface UUID` selects a Zmux surface/managed SSH route. It does not pick a
remote tmux pane. Manual `ssh` typed into a local terminal has no managed SSH
metadata, so it cannot be used as a schedule route. `schedule targets` returns an
opaque, identity-fenced token for each supported pane; create passes that token as
`--remote-pane TARGET_TOKEN`. Raw `%17`-style pane IDs are rejected.

## Remote service and transport

- Run one Rust helper/service for each remote Unix account. Persist a versioned,
  bounded queue and request ledger in a user-owned private directory with
  restrictive directory/file permissions. Use atomic transactions for create,
  edit, cancel, claim, and result changes; select storage from existing workspace
  dependencies where practical and justify any new dependency.
- The helper exposes no public listener and requires no cloud service. Zmux
  invokes a fixed helper command over the selected live SSH ControlMaster; the
  request is a bounded, versioned stdin/stdout protocol. Keep message content out
  of SSH command interpolation, process diagnostic text, and logs. The service
  talks only to its same-user local control endpoint and tmux server.
- `--surface` is a route for each request, not the durable account identity.
  Verify the SSH endpoint/account, remote UID, helper/protocol identity, tmux
  socket, and server incarnation before accepting or showing jobs. Reconnects
  preserve the Zmux surface UUID but create a new SSH launch/session identity;
  they must pass the remote identity check again.
- Create/list/show/edit/cancel/status fail clearly when the selected SSH master
  or remote service is unavailable. Never create a local timer as a fallback.
  After the service acknowledges a job, its timer does not depend on the Mac, app,
  or SSH connection.

## Service lifecycle and supported host

Linux with a systemd user service is supported when `setup --check` confirms a
compatible bundled helper, systemd user manager, and tmux. The preflight reports
host OS/architecture, tmux, helper protocol, service manager, service/helper
state, and linger support before installation. `setup` without `--check` is an
explicit install and enable action; creating a task never installs or starts the
service silently. `status` reports helper and unit versions/state, store health,
tmux identity, clock status, and whether the service can survive logout. Start,
stop, restart, upgrade, and removal are explicit systemd/user-admin operations,
with clear effects on stored jobs; they are not implied by create/list/edit/cancel.

A systemd user service may stop after the last SSH login unless user lingering is
enabled. Explain this requirement and how to enable it; never make a privileged
system change silently. If the service is stopped, the host sleeps through a
deadline, or the host is down, future pending jobs are rechecked on service
restart. The dispatcher has a five-second grace after each deadline: jobs more
than five seconds late become `missed` without being claimed, and there is no
catch-up after the grace. The dispatcher checks again before paste, so if
pre-paste work crosses the grace it marks the task `missed` without sending
input.

## Immutable target binding

At create time, `targets` discovers remote tmux objects and the user chooses an
opaque token returned by that call. The helper binds the job to the verified remote account,
tmux socket/server incarnation, immutable session identity, and pane ID. Capture
window ID/name/index for display and audit, but never use those mutable values to
find a substitute.

Follow the same pane ID across window, index, name, layout, and focus changes while
it remains in the originally bound session on the same server. Refresh window
metadata for display. If the pane leaves that session, the session disappears,
or the server/socket incarnation changes, fail the job as `target_changed` or
`target_missing`. Do not fall back to the currently selected pane, a matching
name/index, a recreated session, or a new tmux server. A different pane requires
a new job.

## Time and editing semantics

- `--at` accepts an RFC3339 timestamp with an explicit numeric offset or `Z`;
  reject local timestamps and timezone abbreviations. Store the resolved instant
  in UTC and retain enough display context for native/local timezone rendering.
- `--in DURATION` is evaluated against the remote helper's acceptance time, both
  at create and at edit. It does not use the client clock or the task's creation
  time. Reject past deadlines and durations outside documented bounds.
- A localized AppKit edit must reject nonexistent DST times and require an
  explicit choice for repeated DST times, then submit an absolute instant. Show
  the resolved date, time, and zone before acknowledgement.
- A message-only edit preserves the exact deadline. A time-only edit preserves
  the exact message. Supplying both changes them atomically. An edit cannot
  retarget the job; create a new job for a different pane.

## Job state and race contract

Each task has a stable UUID and monotonically increasing version. Create, edit,
and cancel carry a client request UUID; the remote request ledger returns the
original receipt for an exact duplicate and rejects reuse of an ID with a
different payload. Edit and cancel require the version read by `show` or `list`.
`status --request-id UUID` returns the original task-mutation receipt, including
the task UUID for a create whose acknowledgement was lost. The service keeps up
to 1024 mutation receipts for 30 days, caps private state at 16 MiB, and allows
at most 100 pending tasks. It reserves one receipt slot per pending task so a
valid version-matched cancel remains recordable at capacity. A new create needs
two free receipt slots; an edit needs one unreserved slot. The service returns
`request_expired` when expiry is provable, and otherwise reports
`not_found_or_expired` with `outcome_unknown`. Clients must not
automatically retry an ambiguous request or create a replacement ID.

`setup install` also carries a request UUID, but installation has no durable
mutation receipt in the task ledger. If install outcome is uncertain, run
`schedule setup --surface UUID --check` to inspect current setup state; do not use
`schedule status --request-id` for that request ID or blindly repeat installation.

| State | Meaning |
| --- | --- |
| `pending` | Durable and eligible for edit/cancel until claimed. |
| `dispatching` | The due worker atomically claimed this version; edits/cancels now conflict. |
| `sent` | tmux accepted the requested input; this does not prove the application processed it. |
| `failed` | A known pre-dispatch or deterministic failure, such as missing target or unsupported host. |
| `cancelled` | A version-checked cancel won while the task was still pending. |
| `missed` | The task passed the five-second dispatch grace before text input; no input was sent and there is no automatic replay. |
| `outcome_unknown` | A crash, timeout, or partial write prevents proving whether input was accepted. |

One serialized durable transaction owner arbitrates all transitions:

- Dispatch claims only with `pending(v) -> dispatching(v)`.
- Expiry changes `pending(v) -> missed` before claim when the deadline plus
  five-second grace has passed; if an already claimed task crosses the grace
  before text is accepted, `dispatching(v) -> missed` with no input.
- Edit commits only with `pending(v) -> pending(v+1)`.
- Cancel commits only with `pending(v) -> cancelled`.

The timer carries its expected version and rechecks it before the dispatch claim.
A winning edit invalidates the old timer; a winning claim prevents later edits
and cancels. Persist the claim before the tmux side effect. Claim only while the
task is within its five-second grace, then recheck the grace before text reaches
tmux; a pre-paste expiry is `missed` with no input. On restart, rearm future
pending jobs, mark jobs beyond the grace `missed`, and mark stale `dispatching`
jobs `outcome_unknown`.

Never claim exactly-once delivery. Do not automatically retry `dispatching` or
`outcome_unknown` work, including after a crash between text input and optional
Enter. If text is accepted but the optional Return becomes stale after the
five-second grace, send no Return and record partial `outcome_unknown`; never
replay either phase. `failed` is safe to report only when the helper knows no
input was accepted. Every dispatch receipt records separate text and
optional-submit phase outcomes (`not_started`, `accepted`, `failed`, or
`unknown`) plus the last durable phase, so `outcome_unknown` does not hide known
partial input. Keep message bodies out of logs and bound jobs, payloads, request
receipts, and retained history; document limits and retention.

## Message and dispatch contract

The v1 bound matches immediate `send-text`: at most 4096 UTF-8 bytes.
Accept exactly one source, `--message TEXT` or `--stdin`; never read stdin while
processing help or before other arguments have passed validation. Preserve the
provided bytes and do not append a newline. `--submit` is create-only and defaults
off; it requests one separate Return after the text has been accepted. Embedded
newlines can still cause an application to react even when `--submit` is off.

The helper feeds text to `tmux load-buffer -` through stdin, pastes the temporary
buffer to the verified pane, then deletes the buffer. Message text does not enter
the SSH shell command, tmux argv, environment, CLI diagnostics, or the local
request journal. The helper rejects NUL and unsupported control bytes before
accepting a task. Preserve test evidence for multiline text, bracketed paste on/off,
target selection, buffer cleanup, and subprocess failure between dispatch phases.
Pass tmux target IDs as structured process arguments, never by shell concatenation.
