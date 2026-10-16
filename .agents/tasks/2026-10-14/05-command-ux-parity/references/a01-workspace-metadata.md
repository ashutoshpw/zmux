# A01a — Workspace status, progress and log metadata

Updated: 2026-10-15. This is a partial A01 slice; the package remains open.

## A01f final-surface checklist lifetime (2026-10-16)

The pinned behavior reference is cmux
`3ba653b9e5afb11d0bc578031142950d6136697e`, inspected read-only. Public move
help (`CLI/cmux.swift:21897-21919`; `docs/cli-contract.md:138,163`) permits moving
a live surface between workspaces; checklist documentation
(`docs/cli-contract.md:875-899`) assigns the persisted list to a workspace.
`Sources/WorkspaceTodoState.swift:8-30` and workspace snapshot capture/restore
(`Sources/Workspace.swift:215,379`; `Sources/Workspace+Todos.swift:254-272`)
confirm that list ownership. The detached surface payload
(`Sources/Workspace+DetachedSurfaceTransfer.swift:26-88`) carries panel/session
attributes without the workspace checklist. Cross-workspace move calls empty
source cleanup (`Sources/AppDelegate.swift:6032`); cleanup removes the emptied
workspace/window with `recordHistory: false` (`:6915-6928`), and history creation
is gated by that argument (`Sources/TabManager.swift:2637`). Session snapshots
capture only live workspaces (`Sources/TabManager.swift:6887-6952`). This is source
inspection, not a fresh native execution result. No implementation, test or asset
was copied or translated into Zmux.

Therefore moving the final surface is allowed even when its source has todos.
The same live surface/session/process reaches the destination; the empty source
workspace retires together with its checklist. The destination's exact list is
unchanged, and the move neither adds an undo descriptor nor changes older close
history. Source items are omitted from the next S01 save and never transferred
or restored with the destination. Moving one of multiple surfaces leaves the
surviving source checklist intact. Moving the full workspace between windows
preserves its checklist. Explicit close/undo keeps A01e's exact removed-owner
recovery behavior. Checklist-only archives would add behavior the reference does
not promise, so none is introduced; session version remains 5.

The original MIT repair removes only the temporary A01e final-surface refusal;
close capture stays in `remember`, never generic removal. The core table covers
nonempty/empty source lists, a distinct nonempty destination list, and stale/no
history, including undo of an older tab after its source retired. The real S01
regression launches two PTYs, retains session/PID and shell-local state through
the move, inspects the saved file, then reopens and verifies only the exact
destination checklist and moved surface return with fresh shell generations.
No test-only production seam or schema change is needed. Native drag/multiwindow
entrypoint parity and the broader frozen command/schema gates remain separate.
The focused core and real-PTY commands first fail on the explicit A01e refusal
and pass after its removal; retained logs are
`/tmp/zmux-a01f-final-surface-{core,runtime}-baseline-red.log` and the matching
`-green.log` files. The earlier A01e 310-test full receipt remains applicable to
unchanged persistence/metadata architecture; A01f binds scoped suites to its exact
six-file frozen delta on top of the committed browser-rename integration.

## A01e todo persistence implementation (2026-10-16; native acceptance pending)

S01 currently writes version 4 documents through `stable_document()` and the
bounded filesystem worker. Autosave compares whole committed documents; explicit
save/export and final shutdown use the same capture. Restore validates and reserves
IDs, launches an isolated candidate, then publishes it only after successful
completion. Corrupt startup data remains intact and disables automatic overwrite.
The storage owner reports no overlapping persistence edits. Preserve these owners,
receipts, no-follow traversal, atomic publication and physical shutdown barriers.

Make the durable checklist one authoritative value on the canonical workspace.
Move todo item/state/origin value types and validation into workspace-core; keep
all public todo requests at the existing serialized metadata action owner. The
metadata owner plans a candidate against the existing aggregate budget and commits
its canonical todo change only after validation. Transient status/progress/log/PR
remain in MetadataStore and are never captured. Native and CLI metadata snapshots
project canonical todos through the existing `todos` response field; hide the
internal workspace list from generic topology JSON to avoid duplicating its wire
payload. Existing item UUIDs, order, state and origin are preserved, including
completed items. Pure core changes perform no I/O or random identity generation.

Write session version 5 with a required todo array per SavedWorkspace. Share the
same item validation between wire mutation and document validation: nonzero unique
UUIDs, existing text/control/state/origin rules, 50 items per workspace and bounded
aggregate data. Retain explicit readers for versions 1–4, which migrate to empty
checklists and reject injected newer fields; version 5 rejects missing/null lists,
unknown fields, duplicate IDs, malformed items and excessive counts/bytes. Older
apps will visibly refuse version 5 rather than discard its data. Imported legacy
source remains untouched by loading; a successful committed autosave may write a
new version 5 authoritative file under the existing S01 policy.

The session document and metadata budgets are independently capped at 512 KiB.
Todo admission must account for the full candidate saved topology, not merely a
list that fits the metadata budget, using the existing bounded serializer without
disk I/O on the executor. Loading rejects an oversized file before parsing and
validates all lists before reserving/launching. The prepared candidate owns restored
todos until model commit; failed/late/cancelled restoration cannot replace the old
model or metadata, and final save never captures an uncommitted restore. Existing
reservation/fresh terminal-session identity rules remain unchanged. Metadata writes
and explicit save admission are distinct: mutation is not a durable save receipt;
existing persistence status/operation receipts report failures.

Lifecycle boundary: app quit/reopen, explicit export/import and live workspace
moves preserve todo UUIDs/order/state/origin with the saved workspace. Closing a
workspace removes it from the live document; undo-close retains the checklist in
that exact bounded recovery descriptor and rebinds it to the newly created
workspace identity without regenerating item IDs. Closing a pane/tab in a surviving
workspace leaves its current checklist intact. If closing the last pane/tab removes
the workspace, attach its checklist only to the recovery item that actually removed
it; do not snapshot stale lists into every tab history entry. Reopening into an
existing workspace never overwrites its newer checklist. Recovery failure retains
the original recovery payload; history pruning/eviction drops it. Recovery history
remains process-local and is excluded from session files. Checklist recovery must
remain bounded and cannot let user edits or autosave publish a partially restored
owner; final implementation must verify the existing undo transaction boundary.

Verification boundary: extend actual core document migration/corruption and
undo-close tests, plus S01 real-filesystem/real-PTY transactions. First retain a
focused failing test showing exact UUID/order/state/origin lost across real save,
shutdown and fresh reopen, then run the same focused command green. Assert other
metadata is absent after reopen, autosave observes a todo-only change, export/import
uses the same data, clear stays cleared, and startup/explicit corruption or failed
launch preserves files and authoritative state. Exercise close-workspace and
last-tab undo plus surviving-workspace tab undo at the existing recovery owner.
Native packaged close/save/reopen must later verify rendered saved checklist state;
Linux source/portable acceptance cannot establish that native gate. Broader todo
edit/set/reorder/open command parity remains outside this lifecycle slice.

Implemented evidence and boundaries:

- `Workspace.todos` is the sole authoritative list. MetadataStore stores only
  transient channels and projects canonical todos; all combined candidates are
  checked before canonical/transient commit. Core setters/actions and v5 import
  validate lists independently, including duplicate UUIDs across live workspaces.
- Undo retains at most 512 KiB of todo payload in addition to the existing 32-entry
  history cap. It evicts oldest settled descriptors, never an in-flight or newest
  descriptor. If no eligible older descriptor can free space, close fails atomically.
  Full-workspace and last-tab/last-pane recovery preserve IDs; surviving owners keep
  newer lists. Failed launches keep the exact recovery payload available for retry.
  Capture happens in the exact close descriptor, never generic topology cleanup.
- Final-surface movement follows the A01f lifecycle below: retire the empty source
  workspace/checklist without recovery history and preserve the destination list.
  A01f supersedes the temporary A01e refusal, retaining exact close capture.
- Todo-only changes preserve topology `model_revision`; a new desktop publication
  revision carries metadata changes. `--expect-revision` guards topology/target
  identity, not todo-list CAS. S01 compares whole captured documents, and actual
  disk payload verifies todo-only autosave even when its saved model revision is
  unchanged. Native row rebuild/target-resolution guards were checked against this
  distinction. Generic Snapshot/Workspace are Serialize-only display types; the
  C ABI accepts only snapshot reads or typed Action mutations. S01 SessionDocument
  validation/preparation is the explicit durable replacement path.
- Candidate size preflight and stable save share live cwd enrichment and the same
  bounded serializer. A real eight-PTY OSC7 fixture proves oversized enriched
  admission fails atomically before autosave latches an error. Restore checks the
  migrated document size even when legacy lists are empty, excludes old transient
  charges, and resets transient channels only at successful candidate commit.
- The real save/shutdown/fresh reopen regression first loses all three exact item
  identities and states, retained in `/tmp/zmux-a01e-todo-persistence-baseline-red.log`.
  Focused green verifies save/reopen, actual todo-only autosave, export/import,
  durable clear, corrupted input and failed PTY startup. Populated same-namespace
  import rejects with current channels unchanged; successful import is admitted
  only after explicit close (which already prunes transient rows). No unreachable
  live-channel replacement case or test-only production seam is claimed.
- Core document/recovery tests cover v1–4 migration, injected empty new fields,
  missing/null/invalid v5 arrays, malformed entries, direct mutation validation,
  duplicate recovery atomicity, history eviction and in-flight preservation.
  Existing legacy fixtures explicitly omit todos so their older browser/tmux/window
  negative controls still fail for the intended contract, not a new-field guard.
- The existing packaged close/save/reopen journey now creates two real CLI todos
  and transient channels, verifies saved v5 IDs/order/state/origin externally, then
  checks exact restored values and the real rendered completion count. Its new
  required receipt flags are `workspace_todos_restored`,
  `transient_metadata_not_restored` and `workspace_todo_row_rendered`. Swift6 typing
  and actual packaged execution remain pending on macOS; no local native success
  is inferred. No browser, idle startup or application profile is modified.

## A01d workspace todo implementation (2026-10-16; native acceptance pending)

This slice implements `todo add/list/check/uncheck/start/rm/clear` through the
existing scoped metadata action owner and native activity inspector. The frozen
public help at cmux `3ba653b9e5afb11d0bc578031142950d6136697e`,
`CLI/CMUXCLI+WorkspaceTodo.swift:350-390`, documents a caller-scoped checklist,
50-item bound, positive one-based positions or UUID item handles, three states
(`pending`, `in-progress`, `completed`) and origins (`user`, `agent`). Only that
public grammar and existing authored audits inform this original MIT design;
no reference implementation, tests or assets are imported or translated.

The shared metadata record gains an ordered `todos` array of `{id,text,state,origin}`.
The owner generates UUIDs when adding and retains insertion order and identity
through state changes, removal of other entries and workspace moves. A positive
one-based selector identifies the current array position at the serialized owner;
a UUID is the stable handle and is always scoped to the target workspace. Missing
or foreign item handles fail without mutation. `check`, `uncheck` and `start` set
completed, pending and in-progress respectively, including idempotent repeats.
Add defaults to pending/user (a local choice; frozen help does not promise defaults).
Clear removes only todos; other metadata and focus remain untouched.

Add accepts exactly one text operand plus optional `--state`/`--origin`. Text must
contain nonwhitespace content, fit the existing 2048-byte UTF-8 text limit and obey
its control-character rules. Every workspace admits at most 50 items and shares
the existing conservative 512 KiB encoded process metadata budget. Failed count,
byte, text, enum or selector validation is atomic; no foreign workspace is evicted.
All item selectors and enums are validated again at the wire owner, independently
of CLI parsing. No network/process work or new executor is needed.

Every command accepts `--workspace`, `--window`, `--expect-revision` and explicit
`--json` (JSON is already the default), with flags
before/after operands and `--` for literal text. Explicit workspace or a proven
caller workspace/surface/managed-tmux claim is required at both CLI and wire owner;
window-only and mutable foreground selection are insufficient. The public help
says caller-scoped but does not explicitly specify behavior without a caller;
rejecting that case is a conservative local choice and remains a compatibility gap.
Reuse existing
target resolution, ownership checks and metadata responses. List is allowed by
read-only control policy; every mutation is denied. Existing PR/status behavior
and targeting semantics remain unchanged. Catalog/help describe these local
parser contracts, and CMD-179 through CMD-185 remain compatibility-unverified.
JSON returns the shared metadata including ordered todos; their array positions
are one-based CLI selectors, and IDs remain preferred for later mutations.

Native Models decode the same ordered records and enums. The existing activity
inspector renders one-based positions, IDs, state, origin and text; row activity
accessibility includes todo counts. Updates reuse the existing selection/scroll
mapping and never open an inspector, select a workspace or replace a responder.
Coordinate metadata-smoke App wiring separately from the runtime startup owner.
The native fixture should exercise actual CLI/socket actions in a background
workspace and the real already-open inspector, with foreground/responder retained.

Test boundary: extend real CLI/Unix-socket and desktop owner integration suites,
not parser-only snapshots or a fake todo store. One CLI lifecycle covers order,
stable IDs, index/UUID state changes, scoped clear, no focus and read-only list
versus mutation. Direct wire cases independently guard text/enums/unknown fields,
foreign IDs, count/aggregate bounds, stale revision and atomic rejection. Include
workspace-move identity and close/namespace pruning at the existing owner boundary.
The new lifecycle must first fail on the public CLI's unsupported `todo` command;
record identical focused red/green commands. Native Swift6 typing and packaged
inspector execution remain actual macOS gates; portable tests do not replace them.

Persistence across save/restore, editing/reordering/replacing/opening todo panes,
automation and frozen text/schema compatibility are outside this bounded slice.
Todos share the existing transient metadata lifetime; the broader workspace.todo
restore acceptance remains open and must not be marked complete.

Portable implementation evidence: the real CLI lifecycle first failed with
`unknown command todo` in `/tmp/zmux-a01d-todo-baseline-red.log`. Repaired CLI
lifecycle and direct wire capacity/validation cases pass in
`/tmp/zmux-a01d-todo-wire-green.log`; all six owner metadata tests pass in
`/tmp/zmux-a01d-todo-owner-green.log`, including actual acknowledged destination
window movement and close/restore namespace pruning. Initial fixture attempts
hit the unrelated eight-terminal dev bound and native window admission rule;
these were corrected at the real fixture boundaries, without altering owners.
Final `scripts/check` passes 300 Rust tests, formatting, strict workspace Clippy,
notice inventory and the actual C ABI harness in
`/tmp/zmux-a01d-todo-full-check.log`. Offline catalog validation and help pass.
The actual packaged receipt gate rejects
missing/false `workspace_todo_metadata` and accepts true; that gate exercise does
not stand in for producing a native receipt. No native Swift compile, typecheck,
inspector execution or localization runtime audit is claimed on this Linux host.
Native strings follow the app's current English literal presentation convention.

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
Metadata survives focus changes and surface moves while its owning workspace survives.
Moving its final surface retires the empty source workspace and its checklist; it does
not transfer metadata to the destination or create recovery history.
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
