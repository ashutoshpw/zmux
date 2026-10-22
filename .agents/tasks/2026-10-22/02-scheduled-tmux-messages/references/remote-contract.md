# Remote scheduler contract

This is the proposed behavior contract for later implementation. It records the
selected persistent remote scheduler design; it does not authorize runtime changes.

## Current code boundary

- `apps/cli/src/targets.rs:287-303` parses immediate `send-text`, caps `--stdin`
  input at 4096 UTF-8 bytes, and submits a `TargetOperation::SendText` request.
- `crates/desktop-runtime/src/lib.rs:2518-2534` resolves that request to the
  exact Zmux terminal surface and passes its bytes to the local session owner. It
  does not inspect a remote tmux server or add Enter.
- `crates/remote-runtime/src/connection.rs:136-165` builds an auxiliary SSH
  operation over an existing private ControlMaster, with no SSH config or proxy
  fallback. This is the management transport for a live managed SSH surface.
- `crates/remote-runtime/src/local_multiplexer.rs:73-95` defines a local tmux
  identity. It is not a remote tmux target or remote scheduler identity.

The `--surface UUID` selects a Zmux surface/managed SSH route. It does not pick a
remote tmux pane. Manual `ssh` typed into a local terminal has no managed SSH
metadata, so it cannot be used as a schedule route. The schedule must identify a
second object explicitly with `--remote-pane PANE_ID`.

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

Linux with a systemd user service is the proposed first supported target. The
read-only `setup --check` preflight reports OS, tmux, helper protocol, service
manager, storage, and clock support before installation. `setup` without
`--check` is an explicit install and enable action; creating a task never
installs or starts the service silently. `status` reports helper and unit
versions/state, store health, tmux identity, clock status, and whether the
service can survive logout. Start, stop, restart, upgrade, and removal are
explicitly documented systemd/user-admin operations, with clear effects on
stored jobs; they are not implied by create/list/edit/cancel.

A systemd user service may stop after the last SSH login unless user lingering is
enabled. Explain this requirement and how to enable it; never make a privileged
system change silently. If the service is stopped, the host sleeps through a
deadline, or the host is down, future pending jobs are rechecked on service
restart and overdue jobs become `missed` by default, with no replay or catch-up.

## Immutable target binding

At create time, `targets` discovers remote tmux objects and the user chooses an
exact `--remote-pane` ID. The helper binds the job to the verified remote account,
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

Each task has a stable UUID and monotonically increasing version. Mutations carry
a client request UUID; the remote request ledger returns the original receipt for
an exact duplicate and rejects reuse of an ID with a different payload. Edit and
cancel require the version read by `show` or `list`. `status --request-id UUID`
returns the original operation receipt, including the task UUID for a create
whose acknowledgement was lost. Retain receipts for a documented bounded period;
return `request_expired` when expiry is provable, otherwise
`not_found_or_expired` with `outcome_unknown`. Preserve enough task/history
linkage to reconcile receipts for their advertised retention period. Clients
must not automatically retry an ambiguous request or create a replacement ID.

| State | Meaning |
| --- | --- |
| `pending` | Durable and eligible for edit/cancel until claimed. |
| `dispatching` | The due worker atomically claimed this version; edits/cancels now conflict. |
| `sent` | tmux accepted the requested input; this does not prove the application processed it. |
| `failed` | A known pre-dispatch or deterministic failure, such as missing target or unsupported host. |
| `cancelled` | A version-checked cancel won while the task was still pending. |
| `missed` | The deadline passed while the service was unavailable; no automatic replay. |
| `outcome_unknown` | A crash, timeout, or partial write prevents proving whether input was accepted. |

One serialized durable transaction owner arbitrates all transitions:

- Dispatch claims only with `pending(v) -> dispatching(v)`.
- Edit commits only with `pending(v) -> pending(v+1)`.
- Cancel commits only with `pending(v) -> cancelled`.

The timer carries its expected version and rechecks it before the dispatch claim.
A winning edit invalidates the old timer; a winning claim prevents later edits
and cancels. Persist the claim before the tmux side effect. On restart, rearm
future pending jobs; turn overdue pending jobs into `missed`, and stale
`dispatching` jobs into `outcome_unknown`.

Never claim exactly-once delivery. Do not automatically retry `dispatching` or
`outcome_unknown` work, including after a crash between text input and optional
Enter. `failed` is safe to report only when the helper knows no input was
accepted. Every dispatch receipt records separate text and optional-submit phase
outcomes (`not_started`, `accepted`, `failed`, or `unknown`) plus the last
durable phase, so `outcome_unknown` does not hide known partial input. Keep
message bodies out of logs and bound jobs, payloads, request receipts, and
retained history; document limits and retention.

## Message and dispatch contract

The proposed v1 bound matches immediate `send-text`: at most 4096 UTF-8 bytes.
Accept exactly one source, `--message TEXT` or `--stdin`; never read stdin while
processing help or before other arguments have passed validation. Preserve the
provided bytes and do not append a newline. `--submit` is create-only and defaults
off; it requests one separate Return after the text has been accepted. Embedded
newlines can still cause an application to react even when `--submit` is off.

Before implementation, validate public tmux behavior for multiline text, bracketed
paste on/off, target selection, and temporary buffer cleanup. A candidate is to
feed text to `tmux load-buffer -` through stdin, paste the named temporary buffer
to the exact pane, then delete that buffer; use tmux's supported bracketed-paste
behavior when enabled. Do not put message text in the SSH shell command. Do not
claim argv privacy until the actual tmux invocation proves it. Reject NUL and
unsupported control bytes before accepting a task, and document multiline and
terminal-control behavior. Pass tmux target IDs as structured process arguments,
never by shell concatenation.
