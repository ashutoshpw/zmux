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
