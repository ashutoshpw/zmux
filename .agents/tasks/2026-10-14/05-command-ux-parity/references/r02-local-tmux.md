# R02a headless local tmux owner

2026-10-15. Implementation authorized; original MIT code, no imported upstream code.

## First cohesive subset

Implement the real user-owned local tmux lifecycle independently of the app control
socket: `start NAME --detached [--cwd PATH] [--command SHELL]`,
`attach NAME|--id UUID --headless`, list/status, detach (single client by default or explicit `--all`), close,
and cleanup preview/`--prune`. `tmux attach` is an attach-only compatibility alias.
App-bound start/attach forms fail explicitly until typed workspace launch/restore
integration lands; unsupported GUI options are not advertised in this slice.

Public contract evidence: frozen cmux `3ba653b9e5afb11d0bc578031142950d6136697e`
`docs/local-tmux.md`, `docs/cli-contract.md:146–147`, and the public usage text in
`CLI/LocalTmuxInvocation.swift:234–256`. Implementation source was not imported.
The profile uses this product's `~/.config/zmux/local-tmux`, with private directory,
registry and lock permissions. Binary override: `ZMUX_LOCAL_TMUX_BIN`.

## Owners and boundaries

- `crates/remote-runtime/src/local_multiplexer.rs`: typed tmux profile, bounded
  subprocess execution, private/locked/atomic registry, immutable session binding,
  inspection, mutation fencing and headless attach process description.
- `apps/cli/src/local_multiplexer.rs`: public grammar and headless execution;
  parent coordinates main/registry dispatch integration alongside browser work.
- Real tmux owner tests plus actual CLI lifecycle/PTY attachment tests. No fake
  executable implementing tmux semantics or test-only production exports.
- App/core launch descriptor, session-document migration, replacement client surfaces,
  focus/no-focus, Settings entrypoint and automatic app restore remain later R02 work.

Every record has a logical UUID, server-incarnation marker stored as a tmux user option, immutable tmux
session ID and creation time. Names are validated bounded identifiers. Headless
inspection never requires app credentials; OS-user/private-profile ownership governs
it. Commands use a selected executable plus fixed argv; start's shell command is
explicit user code and is not persisted in the registry. Inspection/mutation never
runs on the desktop action executor.

Registry locks have deadlines, reads/output/record counts are bounded, writes replace
atomically using mode-0600 files, and malformed/symlinked/wrong-owner state fails
closed. Missing/stale servers do not turn into a fresh server during inspection or
attachment. Mutations verify identity inside the same tmux request. Cleanup only
previews/prunes stale records; it never kills unknown live sessions. Multiple clients
require explicit `--all`; individual `--client` detach remains deferred. Closing an attached client leaves the owner alive.

## Meaningful acceptance

A real tmux executable exists locally (`/usr/bin/tmux`, 3.2a). First verify baseline
unsupported CLI failure. Exercise detached start, exact session/command output, rename
reconciliation, status/list, stale-server/reused-ID rejection, unknown-session
preservation, cleanup preview/prune, private state and malformed/symlink refusal.
Attach through a real PTY, retain the tmux child across client detach/reattach, and
verify single/all detach and ambiguous-client refusal. Public invalid grammar/help
must fail before socket/profile work. Existing local/PTy tests do not own a durable
multiplexer registry or CLI exec lifecycle; these tests add no production test seam.

Preimplementation protocol probes on private temporary sockets confirmed guarded
`if-shell -F` requests, nonzero rejection for a changed marker, no-start inspection,
10,000-line history setup and literal shell-command argument preservation. Probe
servers were terminated only through their exclusively owned sockets. These are
protocol discovery, not delivered Zmux acceptance.


## Delivered headless subset and evidence

All commands here bypass app socket discovery, credentials and caller-surface lookup.
`local-tmux list/status` emit bounded structured JSON; each record binds a logical UUID
to a server incarnation, immutable `$N` session ID and creation timestamp. Status and
mutations reject ambiguous names after external rename/stale-record collisions;
`--id` remains exact. Attach embeds the binding guard in the executed tmux request,
so a replaced server cannot retarget a precomputed attachment.

The profile is mode 0700; registry, lock and socket are mode 0600. Files require the
current UID, regular file type, single link and no symlink following. Registry input
is limited to 128 KiB and 64 records; locks and subprocess operations have 5-second
bounds, subprocess output is limited to 64 KiB per stream. Writes use a fresh private
file, fsync, atomic rename and directory fsync. Startup shell commands are never
saved. A durable-save failure after successful tmux creation reports that the owner
started but remains unmanaged, rather than claiming rollback. Inspection and attach
use tmux's no-start flag; cleanup only removes stale registry records when `--prune`
is explicit. Unknown live sessions survive cleanup and managed close.

A real tmux 3.2a probe showed `detach-client -s OWNED -t ONE` detaches every client on
OWNED, so this subset refuses `--client` and does not advertise individual detach.
Default detach refuses more than one client and rechecks that condition in the same
server request; `--all` is explicit and only affects the managed session. Reported
count comes from that guarded request immediately before detach.

Before implementation, `target/debug/zmux local-tmux list` exited 1 with
`unknown command local-tmux`. Portable acceptance uses a real `/usr/bin/tmux` and
private per-test sockets, with no fake executable implementing tmux semantics:

- `CARGO_TARGET_DIR=/tmp/zmux-metadata-review-target cargo test -p zmux-remote-runtime --locked`:
  25 passed, including 5 real tmux cases and 2 real subprocess ownership cases.
  Lifecycle checks cover exact shell text/unusual cwd, rename reconciliation,
  unmanaged preservation, stale-server reused-ID rejection, single/all detach,
  foreign-client preservation, ambiguous names, permissions, symlinks and bounded
  malformed registry refusal. The process tests use `/bin/sh` directly against the
  private runner to verify cancellation after leader exit and cleanup after normal
  completion; they emulate no tmux protocol.
- Independent review reproduced status selecting a stale same-name record. The new
  real-tmux name-collision regression failed before the shared `find_record` repair
  and passed afterward. Independent review also reproduced a pipe-holding child
  surviving timeout after leader exit. Both inherited-pipe timeout and closed-stdio
  normal-completion subprocess regressions failed before their respective cleanup
  repairs and passed afterward. The launcher group is now cleaned on every exit;
  tmux's separate daemon session retains the durable owner.
- `CARGO_TARGET_DIR=/tmp/zmux-metadata-review-target cargo test -p zmux --test local_multiplexer --test cli --locked`: 2 actual CLI/PTy tests and 13 existing CLI tests passed. They exercise detached start, typed shell output, attach, detach,
  reattach through the alias with the same shell PID, attachment-client exit without
  owner loss, and explicit owner close. Invalid GUI/alias/options and non-TTY attach
  fail before profile creation; tests set invalid app socket/credential/caller values
  to establish independence from the desktop app.

- `CARGO_TARGET_DIR=/tmp/zmux-metadata-review-target cargo clippy -p zmux -p zmux-remote-runtime --all-targets --locked -- -D warnings`: passed after parser formatting repair. `git diff --check`: passed.

Independent correctness/security re-review accepted the frozen repair. Fresh public
binary probes rejected ambiguous names while UUIDs returned the correct stale/live
records; inherited-pipe children were absent after timeout (5.052 seconds), and
closed-stdio children were absent after successful launcher completion (0.095 seconds).
No further correctness/security blockers were found in that scoped review.

These results establish the portable headless subset on Linux. Native macOS tmux,
GUI-bound launch/attachment, app Settings, replacement surfaces, focus policy,
automatic app restore, full R02 completion and native release acceptance remain
separate gates. No user app process was launched, replaced or terminated.

Parent isolated full validation passes 202 Rust tests, strict Clippy, formatting, notices
and C ABI. CI explicitly installs real tmux on Linux/macOS; hosted execution pending.

## R02b app-bound lifecycle (authorized 2026-10-15, in progress)

Implement authenticated app-bound CLI start/attach, client reuse/replacement, and
identity-only reopen/session restore. Settings remains a later entrypoint. Coordinate
model5/sessionv3 after B01 model4/sessionv2; never overwrite the browser worker's schema.

- [x] Typed immutable tmux identity and exact saved-identity/pane resolution APIs.
- [x] Bounded background operation owner and receipts; no process/profile work on
  the UI or serialized workspace executor. Report owner creation separately from
  client attachment, including partial failure.
- [x] Pure terminal descriptors/effects plus sessionv3 readers/writers, retaining
  v1/v2 compatibility. Never persist shell commands, environment or executable paths.
- [x] Actual GUI terminal PTY attach, exact client reuse and explicit additional client,
  stable workspace targeting/focus policy, close/reopen and app restore.
- [x] Managed-pane caller resolution survives new surface IDs; ambiguous hosts require
  explicit targeting. Owned launch environment strips ambient tmux/app identities and
  passwords before applying current client context.
- [x] Real CLI/socket/tmux acceptance proves stable shell PID and one startup execution
  across close/reopen and desktop save/restore. Failure/security cases cover stale
  server identity, auth/capacity before spawning, target closure and client cleanup.

Test authoring boundary: real remote-runtime tmux tests own immutable/pane binding;
actual CLI/socket/desktop/PTY tests own lifecycle and routing; session document tests
own version/validation/privacy. Existing headless tests cannot reach app launch,
caller rebinding or persistence. No fake tmux protocol executable and no test-only
production seam will be added.


### R02b implementation and validation checkpoint

Production source frozen for independent review. GUI CLI start/attach enter the
normal authenticated socket policy; the direct headless lifecycle stays separate.
`DesktopOptions` selects the installation state directory and a tmux profile
(environment default or explicit validated profile), without process-global mutation.
The new runtime owner admits at most four pending operations and four physical jobs,
uses bounded completion channels, and retains 32 epoch-scoped receipts. Timeouts
signal worker cancellation while the receipt retains its capacity charge until the
actual outcome arrives. Startup side effects and client attachment outcomes are
reported separately; target loss after successful start leaves an explicitly reported
live owner. Launch/registry/process waits run on background workers. Shutdown cancels
operation and launcher tokens and reports a cleanup deadline failure rather than
claiming that an unknown owner was removed.

Core model5/sessionv3 use a distinct immutable local-tmux terminal descriptor. Explicit
v1/v2 readers remain, new descriptors are rejected under older versions, and browser
profile-presence validation remains intact. Exported tmux content contains only the
four-field identity; it has no shell command, cwd, executable, socket or environment.
Reopen receives fresh client/surface IDs; app restore retains topology IDs but creates
fresh client sessions. Every launch regenerates an exact guarded tmux attach request,
then confirms the spawned client PID on the expected immutable session before success.
Closing/shutdown only cancel clients; they never kill the durable owner.

Default GUI attachment reuses an exact live client; `--new-client` explicitly creates
another. Saved routing hints are stable UUIDs validated against the current desktop.
A still-live exact surface follows workspace moves. Reuse refreshes the hint on a
separately bounded background worker before its receipt completes. A missing surface
can be replaced in its verified workspace; a stale namespace/workspace requires
`--workspace`, with no title/cwd fallback. The moved-surface public regression failed
before this repair (a duplicate client appeared in the old workspace) and passed after.

Durable shells export only owned tmux profile/executable context plus the opt-in
marker; inherited app/terminal identities and credentials are stripped. Caller routing
validates the actual owned `TMUX` socket and `%N` pane, resolves its current immutable
managed session, and matches exactly one app surface. This also follows pane moves
between managed sessions. Multiple app hosts fail implicit targeting; explicit targets
override inherited caller state. No startup shell environment is rewritten on reattach.

Validation on Linux with real tmux 3.2a:

- `CARGO_TARGET_DIR=/tmp/zmux-metadata-review-target cargo test -p zmux -p zmux-desktop-runtime -p zmux-remote-runtime -p zmux-session-runtime -p zmux-workspace-core --locked`: all 205 tests passed. A later added four-operation resource-bound acceptance test also passed independently.
- Public local-tmux acceptance: 8 tests passed, including authenticated socket/PTY
  start, focus preservation, reuse/additional client, ambiguous caller refusal,
  close/reopen, moved-surface reuse, actual CLI caller rebinding despite stale app
  environment, durable save/shutdown/restore with the same owner PID and startup count
  one, stale receipt epoch rejection, read-only/capacity rejection before owner creation,
  target closure during a real registry-lock wait with an honest surviving-owner
  receipt, four blocked starts rejecting a fifth while workspace actions remain
  responsive, and replaced-server restore failure without command replay or retargeting.
- Remote owner acceptance: 6 real tmux tests passed, including exact saved-identity,
  owned-pane movement, unmanaged refusal and durable GUI-hint checks. Core session
  document acceptance: 7 tests passed, including v1/v2/v3 rules, identity validation
  and command injection refusal.
- `CARGO_TARGET_DIR=/tmp/zmux-metadata-review-target cargo clippy -p zmux -p zmux-desktop-runtime -p zmux-remote-runtime -p zmux-session-runtime -p zmux-workspace-core --all-targets --locked -- -D warnings`: passed.

Native AppKit rendering/dogfood, packaged macOS tmux acceptance, Settings entrypoint,
individual-client detach and full R02/V01 parity remain separate gates. This checkpoint
adds no native execution claim and never touches the user's running cmux application.


### Independent review repairs

The review found two additional caller entrypoints that bypassed the intended scope:
implicit `list-panes` mistook a managed caller for an external global request, and
explicit-workspace `notify` let ambiguous ambient attribution veto its hard scope.
One actual CLI-inside-tmux regression independently observed both failures before
repair: list succeeded globally while the explicitly scoped notification failed.
After repair, implicit reads require exact caller binding; explicit workspace
notification attribution uses a unique ambient surface when available and otherwise
retains the explicit workspace. A moved caller's pane listing contains only its current
workspace.

A related audit found default `clear-notifications` could choose global scope because
durable shells intentionally lack a legacy workspace variable. The two-workspace
public regression failed here and in the independent review snapshot, deleting the
other workspace's notification. The parser now recognizes managed context before
reading legacy workspace variables, defers workspace resolution to the immutable caller,
and preserves the other workspace's notification. Explicit global/exact-ID operations
remain caller-independent.

Failed post-spawn confirmation/registry writes now return the owned client Session
through `Completion.cleanup`. Both normal and shutdown handlers move it to tracked
retirement before releasing the pending launch credit. This is a source ownership
invariant. The real failure-path acceptance fills a valid registry to 128 KiB minus
16 bytes: guarded attachment succeeds, its GUI-hint write fails, the new client is
cleaned up, and the original client PID/owner/registry bytes survive. That acceptance
proves actual client cleanup and owner preservation; it does not make a scheduler-timing
claim that distinguishes retained retirement from an asynchronous destructor.

Ordinary/SSH app terminals remove inherited parent `TMUX`, `TMUX_PANE`, opt-in marker
and profile context, preventing an app launched from a managed shell from misattributing
its new ordinary terminal. The intentional tmux executable override remains available
as user configuration. Local-tmux clients already strip inherited app/tmux context.

### Final portable receipt

After the review repairs, the same five-package test command passed all 207 tests
(exit 0; `/tmp/zmux-r02b-final-tests.log`). The independent frozen snapshot also
passed all eight public tmux and seven session-schema tests. An independent actual
Desktop/PTY probe verified inherited parent tmux context removal while preserving
owned workspace/surface IDs and the configured executable override. Removing the
ordinary launch removal call in that private snapshot made the same probe fail on
inherited `TMUX`, and restoring it returned green. No repository test-only API was
introduced for that probe.

Parent acceptance: frozen index snapshot on528be92 passed full `scripts/check`
(Rust workspace tests, formatting, strict Clippy, notices and C ABI). Native packaged
attachment/restore acceptance remains pending; portable tests do not close R02/V01.
