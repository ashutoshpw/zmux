# A01a — Workspace status, progress and log metadata

Updated: 2026-10-15. This is a partial A01 slice; the package remains open.

## A01c manual pull-request metadata

`pr <canonical-GitHub-pull-URL|positive-number>` and `pr clear` use the shared
metadata owner, with `--workspace`, `--window` and `--expect-revision`. Options
may precede/follow the single value. An explicit workspace or live caller is
required; foreground focus is never a PR target. The initial shared-owner read
returns `workspace_uuid`; both workspace and window UUIDs are pinned before
lookup. Closing, replacing or moving that owner to another window cannot retarget
the final mutation to a reused numeric/index selection.

Set verifies the CLI's actual Git toplevel against at least one published local
terminal directory in the target workspace. Remote-only/unmatched contexts fail.
The GitHub `origin` must be HTTPS or `git@github.com:OWNER/REPO[.git]`, without URL
credentials, alternate hosts or decorations. Authenticated `gh repo view` verifies
origin and its optional fork parent; the actual provider parent schema is
`{name,owner:{login}}`, independently observed by the parent agent in a read-only
public query. URL inputs must match origin or that parent. Bare numbers select
the parent for forks. `gh pr view` must return the same canonical repository and
number. URLs allow only the exact `https://github.com/OWNER/REPO/pull/NUMBER`
shape; queries, fragments, escapes, extra path segments, trailing slashes and
leading-zero/overflow numbers fail. Numbers use positive Rust/Swift `UInt64`.

All Git/gh work runs in the CLI, outside the action executor. One `--timeout`
deadline covers all lookups and socket requests. Each physical child has one
additional, nonrenewing two-second cleanup reserve. The reusable CLI capture
owner uses the existing nonreaping Session child observer: retain the spawned
leader through group cleanup, then reap it once. Both streams are nonblocking
and capped at64KiB; EOF does not remove the deadline, and successful leaders do
not leave pipe-closed descendants running. Darwin `EPERM` is accepted only after a
bounded public `/bin/ps` snapshot, while the exact leader remains unreaped, proves
that the reserved leader is present and every matching group member is a zombie.
Live/missing/malformed/oversized/timed-out snapshots fail. The trusted process
snapshot probe owns only its direct child and shares the original cleanup reserve.
INT/TERM/HUP cancel before mutation.
The private Zellij launcher cannot directly serve these noninteractive lookups
because it also owns Zellij foreground/job-control policy. Tool environments
retain authentication/config and network-trust inputs but exclude ambient
Git/GitHub repository/host/compiler overrides. Child stderr is never promoted to
metadata or CLI errors. Post-admission socket timeouts remain unknown outcomes;
never automatically replay a mutation.

The shared owner independently validates the record's number/URL and includes it
in the existing512KiB aggregate encoded budget. `pull_request` is null or
`{number,url}`. Clear changes only that field, needs no Git/gh and requires write
access. `list-status`/`list-log` and `current` expose the same manual link.
Native rows and the plain-text activity inspector project that record without
focus or selection changes. Metadata is transient; automatic refresh,
TTY/unique-worktree target inference and full frozen PR parity remain open.
The machine catalog maps `pr`/`pr clear` only as compatibility-unverified local
counterparts; discovery describes parser preflight and never runs tools.

Test authoring boundary: real CLI/socket, temporary real Git worktrees and a
controlled external gh protocol fixture for offline provider responses; direct
wire requests independently cover owner validation bypassing CLI parsing. Existing
status tests could not exercise PR parsing, repository lookup or physical lookup
children. No test-only production seam was introduced. Initial focused CLI red
failed with `unknown command pr`; the repaired CLI metadata suite passes9 cases,
including slow lookup versus reused workspace index, fork-parent and repository
named `pull`, foreign worktrees, read-only clear, malformed response/input,
credential-safe failures, stream overflow, TERM owner cancellation,
closed-stdio and pipe-holding descendants. The direct owner suite passes5 cases,
including maximal u64, canonical URL/number agreement and strict clear fields.
Only TERM cancellation was physically exercised; INT/HUP share its registered
owner path. Optional nonrepository probes ignore only the exact C-locale Git
exit128/not-repository response; cleanup/status/stream/UTF8 failures propagate.
In an isolated source copy, swallowing those fatal errors makes the real second
Git probe test fail with a misleading worktree mismatch; restoring and rebuilding
the exact repaired source passes the same focused command. The provider-shape fix uses the parent's actual sanitized receipt
`/tmp/zmux-a01-gh-parent-live-contract-20261015.json`.

Final core `scripts/check` passes297 Rust behavioral tests, workspace formatting,
strict Clippy, the notice inventory and actual C ABI executable. The only lockfile
change adds the already resolved signal-hook dependency to the CLI package; no
dependency versions changed. Native PR row/inspector execution, actual Darwin zombie-group confirmation and
authenticated live GitHub PR resolution remain separate gates. The native
supplement sends a real shared-socket action and checks max-u64 decoding, row/open
inspector projection, no focus change and public `pr clear` preserving other
metadata plus the selected status text. The inspector maps selection through
unchanged document prefixes/suffixes so PR insertion/removal does not shift the
reader to different text. No local Darwin execution or regression red/green for
that native selection repair is claimed.

## Contract and provenance

Implemented original MIT Rust/CLI code using the repository's target executor,
control authorization and JSON command catalog. The frozen comparison remains cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`. External CLI help was inspected for public
command/option semantics (`CLI/cmux.swift:23129–23247`), together with the repository's
agents/remote audit and A01 work package. No GPL/BUSL implementation, tests or assets
were imported or translated.

Implemented CLI commands:

- `set-status KEY VALUE [--workspace TARGET] [--icon NAME] [--color #RRGGBB] [--priority N]`
- `clear-status KEY [--workspace TARGET]`, `list-status [--workspace TARGET]`
- `set-progress VALUE [--workspace TARGET] [--label TEXT]`, `clear-progress [--workspace TARGET]`
- `log MESSAGE [--workspace TARGET] [--level LEVEL] [--source NAME]`
- `clear-log [--workspace TARGET]`, `list-log [--workspace TARGET] [--limit N]`

Metadata leaf help uses a specialized target footer: caller workspace/surface agreement
and option placement are explicit, rather than inheriting the general surface-first
routing instructions. Options may precede or follow positional arguments; `--` preserves literal text.
`--expect-revision N` is accepted through the shared target grammar. Log levels are
`info` (default), `progress`, `success`, `warning`, `error`. Unsupported flags fail.
No window-selector or source-baseline text-output parity is claimed: this repository
uses JSON receipts consistently. Each command returns the resolved workspace metadata
row; `list-log --limit N` limits only the returned log list, never retained state.

Target resolution and metadata operations run on the same serialized desktop executor.
An explicit scope overrides ambient caller context and constrains ownership through
its workspace/pane/surface handles. Without explicit scope, the caller workspace is
used with any supplied surface checked against it; otherwise the caller surface or
current focus provides the workspace. Invalid, closed, mismatched or stale-revision
targets fail before mutation. Background updates do not select workspaces or panes.

## State, limits and errors

`Snapshot.workspace_metadata` contains workspace rows in stable numeric ID order:
`{workspace_id,workspace_uuid,pull_request,statuses,progress,logs,logs_dropped}`. Workspaces with no metadata are
absent. Status entries contain `{key,value,icon,color,priority}`; they replace by key
and sort by priority descending (default 0), then key ascending. Progress is null or
`{value,label}`; values must be finite and in 0–1, and completion remains until cleared.
Logs contain `{sequence,message,level,source}`, in oldest-to-newest sequence order.
`--limit` returns the newest N in this same order.

Per workspace: at most 64 status keys and the newest 256 log records. Replacing an
existing status remains allowed at key capacity. Log rollover increments the visible
`logs_dropped` counter; clear-log clears both records and the counter. Names are 1–128
UTF-8 bytes without control characters. Status text and progress labels allow at most
2048 bytes; log messages allow at most 1024 bytes. Text admits newline/tab but rejects
other controls. Color uses `#RRGGBB`; priority is a signed 32-bit integer. Limit is 1–256.

All workspaces together share a conservative **512 KiB encoded metadata budget**. Size
accounting includes bounded JSON framing, numeric fields and up to two encoded bytes
per admitted text byte. This independently bounds the aggregate native snapshot,
not merely one workspace's control response. Exceeding the byte budget rejects the
entire mutation and preserves all workspaces, log sequence and dropped counts. There
is no cross-workspace eviction. Clearing metadata or closing its workspace releases
capacity. The public errors are `invalid_metadata` and `metadata_capacity`.

List actions are classified as reads by the socket policy and do not change retained
metadata. Mutations require write access and retain executor-time authorization.
Metadata survives focus changes and surface moves because its owner is the workspace.
Closed-workspace state is pruned at publication. Source review confirms that a restored
identity namespace resets the store; the restore test closes the workspace first, so
its narrower behavioral evidence is that close-then-restore does not resurrect metadata. This is transient process state; session persistence is not implemented.
No PTY, filesystem, network or native-host wait was added to the action executor.

## Verification and test authoring gate

Production owner: the private runtime metadata store behind `Command::Targeted`,
workspace target resolution, and the real CLI/control server. Existing attention and
session tests do not exercise these metadata channels. Credible regressions are
wrong-workspace updates after focus/moves, mutation on invalid values/stale targets,
write access through read-only sockets, unbounded aggregate snapshots and stale
metadata after close/restore. Tests use public requests/snapshots and the actual CLI;
no production test-only seam or exported private store was added.

Red/green evidence:

- `cargo test -p zmux --test metadata`: initial two behavioral tests failed on the
  baseline with `unknown command set-status`; after implementation all four pass.
- `cargo test -p zmux --test metadata readonly_socket`: failed with read-only denial
  for `list-status` before metadata read classification; passes after the policy fix.
- `cargo test -p zmux-desktop-runtime --test metadata process_byte_capacity_is_shared`:
  with the aggregate rejection guard temporarily removed, failed because the actual
  serialized metadata exceeded 512 KiB. Restoring the guard passes. This is a focused
  pre-enforcement reproduction within this slice, not a baseline repository claim.
- `cargo test -p zmux-desktop-runtime --test metadata`: all four pass, covering entry
  capacity, log rollover/order/drop counts, atomic invalid/wrong-owner/stale-revision
  rejection, close/replacement cleanup, close-then-restore without metadata resurrection, aggregate
  multi-workspace byte capacity and capacity recovery without foreign-owner eviction.
- `cargo clippy -p zmux -p zmux-desktop-runtime --all-targets --locked -- -D warnings`: passed.
- `git diff --check`: passed at the metadata worker freeze.

The public escaped-payload test fills maximum-size status records, appends maximally
escaped log records until aggregate capacity rejects atomically, and then reads all
accepted metadata through the real CLI/socket. It verifies readable output, rather
than restating the accounting formula.

Full-workspace checks and native presentation are integrated by the parent task.
No claim of macOS display, downloaded-artifact behavior or full A01/parity is made by
these portable checks. Events/replay, remaining PR compatibility, todo owners, rules/hooks and richer inspection
remain separate A01 work.

Parent full `scripts/check` passes189 Rust tests, formatting, strict Clippy,
notices and the C ABI harness on the integrated metadata source.

## Native integration (awaiting hosted validation)

Native workspace rows show the highest-priority status and progress or log count.
An accessible activity button opens a bounded selectable plain-text inspector for
all retained metadata. Its owner is the workspace plus identity namespace; replacement
or closure retires the inspector. Unchanged snapshots do not rebuild its document.
Metadata updates preserve selection and scroll position and never select a workspace.

The packaged smoke uses the public CLI to update a background workspace, inspects the
rendered row and open inspector, appends logs, and clears metadata while the inspector
is open. It checks existing workspace/tab identities and focus after dismissal. This
covers native decoding, ownership, and presentation that portable runtime tests cannot
reach. The required `workspace_metadata` receipt is not yet a passing native result.

Hosted receipt: Workspace metadata native row/inspector acceptance executed successfully at5f5005e in
macOS38753451286 (universal build and packaged smoke). Parent downloaded and checked
archive/source/required flags. This closes that subset native gate only; full package
parity and applicable physical/external-host gates remain open.

## PR lookup fixture admission repair (2026-10-15)

The first actual Darwin PR validation rejected a lookup at its shared 250 ms
deadline, then the test unconditionally read a gh descendant PID that had never
been proven admitted. The retained failure cannot distinguish pre-gh preflight
expiry from shell pre-marker scheduling. A real first-Git exec-sleeper reproduces
the identical missing-marker panic while honoring the unchanged timeout.

The repaired fixture separately requires real preflight timeout rejection within
the original three-second bound, unchanged PR metadata and no gh child marker.
Held-pipe/exited-leader and closed-stdio/live-leader fixtures publish readiness
only after writing their actual descendant PID. Cancellation is sent only to the
CLI process after that readiness, with a finite admission wait and three-second
termination bound; descendants must be absent or zombies. Successful lookup
closed-stdio cleanup and existing live-leader TERM coverage remain. No production
process owner, timeout or cleanup assertion is relaxed. Evidence is specifically
deadline before Git completion and descendant cleanup after TERM; this does not
claim a timeout occurred after descendant admission.

The deterministic preflight failure is retained in
`/tmp/zmux-a01c-preflight-expiry-marker-red.log`; the same focused command passes
in `/tmp/zmux-a01c-preflight-expiry-admission-green.log`. Actual macOS rerun and
independent source/negative-control review remain separate gates.
