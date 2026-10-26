# T02 navigation and reversible split closure

Working implementation, 2026-10-15. This is a subset receipt; T02 and V01 remain open.
Original MIT implementation with frozen cmux behavior used only as a reference.

## Shared owners

WorkspaceStore now owns workspace selection, next/previous/last workspace/pane/tab
selection, four-direction splits and adjacent pane focus, and bounded global focus
back/forward history. Selection changes do not restart sessions. Split creation inherits
the selected terminal route without replaying an SSH startup command. Explicit no-focus
preserves the active workspace and pane. History is bounded to 128 records per collection,
prunes closed targets and preserves owner history when surfaces move.

Global focus history deliberately uses a direct action; there is no scoped TargetAction
that could accept a workspace selector and then silently leave its scope. Other targeted
operations resolve ownership and revision on the serialized executor. Creation receipts
identify new panes/surfaces independently from focused objects, including no-focus splits.

Close/reopen now retains bounded ancestor placement descriptors rather than whole live
subtrees. Reopening a pane or the last tab reconstructs the original split around surviving
siblings; missing anchors use surviving ancestors, then a safe live-workspace fallback.
Fresh IDs/sessions are allocated only for restored content. Existing sessions remain intact;
failed grouped launches remove provisional content and keep retryable close history.

## Entrypoints and baseline observations

CLI adds new-split, select-workspace, workspace/tab/pane navigation, focus-direction,
focus-history, and tab-action rename/close-others through shared actions. Options not yet
implemented continue to fail rather than report success. Frozen CLI/cmux.swift new-split
uses default focus false at line 8671; next/previous/last-window delegate to workspace
operations at lines 30839, 30847 and 30855. Native multiwindow scope remains unavailable.

Native menus use shared actions, including Cmd-Option arrows for directional focus,
Cmd-Control brackets for workspace cycling and Cmd brackets for global focus history.
The frozen native handler prioritizes global history over browser history. Browser
back/forward remain available through their menu and toolbar controls. Focus transitions
update the actual terminal/WebKit responder, including transitions out of address fields.

## Verification

Parent scripts/check passes all 98 Rust tests, formatting, Clippy with warnings denied,
resolved dependency notices and C ABI checks. Owner regressions cover nested geometry,
scoped targets, no-focus, MRU movement, bounded history, grouped restore rollback, anchor
loss/remapping and depth capacity. Real CLI/socket tests assert creation receipts and
navigation outcomes. Initial regression runs failed before the corresponding repairs.

Packaged smoke now exercises shared navigation shortcuts, browser/terminal responder
ownership and original split restoration with unchanged surviving process IDs. Universal
Swift 6 compilation and executed native evidence are pending GitHub Actions. Fresh read-only review passed; no user-facing artifact is approved.

## Open gates

Directional selection currently uses equal-share topology, not native divider ratios.
Histories and close descriptors are in-memory only. Multiwindow, persisted layouts,
remaining surface/tab actions and physical input/accessibility/resource gates remain open.

## Same-pane surface reorder subset, 2026-10-15

The shared Rust placement planner resolves signed final indices, same-pane before/after
anchors and bounded native offsets. Signed indices clamp to the first/last tab; both
self anchors are no-ops. The strict unsigned raw action keeps its historical bounds.
Reorder now mutates the existing pane in place and defaults to no focus, including the
omitted action field and retained legacy `[SURFACE] INDEX` CLI. Cross-pane `MoveSurface`
behavior is unchanged.

Canonical CLI accepts one subject and exactly one `--index`, `--before`/`--before-surface`
or `--after`/`--after-surface` placement, scoped selectors, optional revision and explicit
`--focus true|false`. Focus true reserves a fresh exact native receipt before model
commit; host/capacity failure rejects without mutation. Native focus failure or timeout
reports that order is already committed and never retries or rolls it back. Read-only
sockets deny all reorder requests. Native left/right menus and Command-Shift-Option
brackets use the same planner; same-pane drag also routes through it with captured
namespace, source pane and revision. Cross-pane drag retains the move action.

Behavior research used only the pinned cmux `3ba653b9e5afb11d0bc578031142950d6136697e`
CLI/native contracts and exact Bonsplit dependency `d67d1b114fd20edba7a19f76cc39e35bde43c2d8`
to resolve signed bounds and self anchors. No source, tests or assets were imported or
translated. Frozen CMD-071 is a local counterpart with compatibility unverified.

The owner regression failed before the repair because a background reorder focused its
workspace; the same focused core test command now passes. Core placement tables cover
adjacent movement, both self anchors, extreme signed bounds and non-wrapping offsets.
Actual CLI/socket tests cover canonical/legacy grammar, target fencing, focus defaults,
policy, exact native acknowledgements, failure and uncertainty without replay. Runtime
preflight covers unregistered native hosts and the full exact-focus queue. Real PTY and
filesystem save/restart tests retain live session/PID identities during reorder, persist
the exact order and restore fresh ordinary shell sessions; namespace/revision guards
reject captured stale targets atomically.

An isolated full `scripts/check` on baseline `692b12c` plus only this slice passed 271
Rust tests, formatting, Clippy with denied warnings, dependency inventory and C ABI.
Evidence lives in `/tmp/zmux-t02-surface-order-{red,green,socket,persistence,preflight}.log`
and `/tmp/zmux-t02-surface-order-isolated-check-final.log`. Native smoke is authored for actual
CLI/menu/key-equivalent paths, tab-strip order, key/responder and terminal/browser host
retention. Swift compilation, executed macOS smoke and physical AppKit drag acceptance
remain pending; portable evidence does not establish native acceptance or V01 closure.
