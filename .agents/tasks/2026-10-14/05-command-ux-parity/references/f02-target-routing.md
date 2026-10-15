# F02a — stable live-model identity and executor-owned target resolution

Implementation started 2026-10-15 from `a02bd4d`. This is a bounded F02 foundation,
not full F02, CLI compatibility or whole-product parity.

## Implemented contract

- Each application model receives an OS-random UUID namespace. Workspace/pane/surface
  UUIDs derive deterministically from that namespace, kind and monotonic numeric ID.
  Move/reorder preserves UUIDs. Closed/reopened objects get fresh IDs and UUIDs.
  Replacing the app invalidates old UUIDs even when its numeric IDs start at 1 again.
- `list-workspaces` returns a typed target catalog with numeric ID, UUID, ref, one-based
  index and workspace/pane ownership. Pane/surface listings and mutation responses add
  UUID fields while retaining existing numeric fields. Snapshot's namespace is additive;
  existing model-v3 and C-ABI-v1 consumers retain their original numeric action schema.
- Existing targeted CLI commands accept UUID, `workspace:N`/`pane:N`/`surface:N`/`tab:N`,
  positive legacy IDs and explicit `index:N`. Bare numbers retain stable-ID semantics;
  positional indices require the explicit prefix during this migration. Pane indices
  are local to the selected workspace; surface indices are local to the selected pane.
- Leading `--workspace`, `--pane`, `--surface` selectors constrain ownership. Omitted
  targets use inherited caller context, then focus. Explicit selectors override caller
  context. A caller surface is the durable anchor after a cross-workspace move, so an
  older inherited workspace does not misroute its requests. A stale caller surface fails.
- Symbolic mutations resolve on the Rust workspace executor before using the same
  numeric Action owner as native UI. Targeted send resolves the exact surface/session
  there, including the legacy control send path. Read operations resolve and read one
  immutable snapshot. No client-side lookup/mutation race is introduced.
- Optional `--expect-revision` checks `model_revision` before a read or mutation. Errors
  distinguish malformed, wrong-kind, absent/stale, cross-scope and stale-revision targets.
- Shell launch overrides inherited `ZMUX_WORKSPACE_ID`, `ZMUX_SURFACE_ID` and `ZMUX_TAB_ID`
  with the actual newly created terminal's UUID context. Inherited values cannot route a
  nested zmux shell's commands back to its parent terminal.

## Code owners

- `crates/workspace-core/src/targets.rs`: pure identity/catalog/selector/scope resolution
  and translation into shared Actions. Namespace generation remains outside the core.
- `crates/desktop-runtime/src/targets.rs`, `control.rs`, `lib.rs`: symbolic request envelope,
  revision guard, serialized execution and additive inspection results.
- `apps/cli/src/targets.rs`: implemented target grammar, caller defaults and exact text
  input; positional compatibility and leading named selectors. Unknown/repeated options fail.
- `crates/session-runtime/src/lib.rs`: production shell environment override entrypoint.
- `crates/native-bridge/src/lib.rs`: isolated namespace for each standalone C-core instance.

## Verification

New tests protect independent contracts at their strongest owners: core scope/lifecycle
invariants, plus public CLI/socket parsing/transport and a real PTY's inherited context.
The latter verifies UUID rename, typed refs, explicit workspace indices, wrong-kind,
cross-owner and stale-revision failures, then restarts the app with reused numeric ID1
and verifies that the original UUID is rejected. Existing close/reopen test mistakenly
selected a restored tab by array position; it now identifies the fresh tab while retaining
the original, making the shell-context assertion meaningful.

Observed red: the focused CLI test failed on baseline because created workspaces lacked
stable UUID output. Observed green: that test, both new core target tests and all
`scripts/check` gates pass (47 Rust tests, fmt, clippy, notices and C ABI).
Review caught a legacy send-to-browser error regression. The real raw-wire test
failed with `command_rejected` instead of `not_terminal`; the repaired owner keeps
browser targets distinct from terminals that are not ready. Both legacy and targeted
wire requests plus the public CLI now preserve that code. Move destination indices
resolve relative to the source workspace; malformed/repeated options fail before any
socket access. Corrected checks pass with the same 47-test workspace suite.

UUID and its resolved dependencies retain upstream notices; inventory now includes 61
packages, with three top-level license-text follow-ups (r-efi and two Windows target
packages). None of those follow-ups represents a new macOS runtime dependency.

Fresh independent read-only repair review returned `ship` against the combined candidate
digest `30de1003ae3df16ef66bb8338b3fd2936252b7c20288a4ad719092268011e52b`
from base `a02bd4d`, with no remaining material findings.

No macOS native build, real remote session or user-facing artifact is accepted by this
receipt. Native compilation/runtime remains a hosted gate. Full-artifact parity policy
below applies even if an intermediate CI archive can be produced.

## Remaining F02 and integration scope

Window ownership/IDs, full focus/no-focus semantics, ID presentation, complete cmux flag
placement/grammar and caller socket/auth contracts remain open. UUIDs currently identify
live models; S01 must persist namespace and allocation state when adding durable restores.
The engine/latency budgets and SSH/transfer packages remain open. Existing typed native
Actions keep their numeric schema; additional symbolic routing can use the shared Targeted
envelope without forcing UI string lookups. No feature family is closed by this slice alone.

## Artifact sharing gate

User instruction on 2026-10-15: the next artifact shared with them must have full cmux
feature parity. Intermediate Actions builds are diagnostic evidence only. Do not present
them as the requested build or attach their download links as a completed delivery.
V01 must reconcile every required audit family/leaf/alias/option and UI interaction,
pass the applicable packaged native, two-host, deployed cloud, permission and measured
stability gates, and record exact source/artifact evidence before sharing a release.
Unavailable hosting/signing/device/provider gates remain incomplete; local green tests,
source review and a successful compile cannot substitute for them.
