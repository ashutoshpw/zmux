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
