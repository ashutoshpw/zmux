# A01a — Workspace status, progress and log metadata

Updated: 2026-10-15. This is a partial A01 slice; the package remains open.

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
`{workspace_id,statuses,progress,logs,logs_dropped}`. Workspaces with no metadata are
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
these portable checks. Events/replay, PR/todo owners, rules/hooks and richer inspection
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
