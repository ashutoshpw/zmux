# Workspace sidebar drag ordering

**Date:** 2026-10-22. **Task:** 01-workspace-drag-order.

## Objective
Allow dragging a workspace row to reorder workspaces in its current window while preserving the existing selected workspace, live sessions, and group invariants.

## Context
The shared revision-bound `workspace_order` route and native menu already work. Sidebar rows currently select on mouse-down and neither rows nor the sidebar list participate in workspace drag and drop. Keep the implementation independent to Zmux; do not copy cmux code.

## Approach
Use a frozen namespace/workspace/window/revision drag token and require the actual originating row and window owner at the destination. Defer normal row selection until mouse-up; a drag must not focus or restart a workspace. Show insertion cues only for valid final canonical slots, using scrolled/flipped row geometry, and submit accepted drops through the existing order action. Keep Rust validation authoritative.

## Test contract gate
1. **Observable contract:** a real packaged sidebar drag changes same-window canonical and rendered row order, preserves group anchors/membership/contiguity, and retains selection, responder, PTY processes/views, and browser identity.
2. **Credible regressions:** a click selects before drag threshold; an off-by-one slot after removing the source; stale/foreign source data accepted; collapsed/scrolled/flipped coordinates target the wrong row; or a drop splits a group or changes its anchor.
3. **Remaining coverage gap:** core and CLI tests cover the order planner. `WorkspaceOrderSmoke` covers CLI/menu reconciliation and live identity retention, then calls the real row mouse-down/up and sidebar destination callbacks. It does not drive `mouseDragged` across the threshold, start `beginDraggingSession`, or exercise the `NSDraggingSource` through AppKit's OS-controlled drag loop.
4. **Production seam:** none. The packaged smoke uses synthetic `NSDraggingInfo` to deliver the real destination delegate callbacks after the row captures its source token on mouse-down. This does not verify drag threshold dispatch, creation of an AppKit dragging session, or physical OS gesture behavior.

## Steps
- [x] Implement row drag threshold, immutable source token, owner validation, valid-slot insertion cue, and shared-action drop.
- [x] Extend packaged `WorkspaceOrderSmoke` and require its report key; cover click/drag disposition, geometry, groups, stale/foreign rejection, and live identity retention.
- [x] Update task status and concise native-drag evidence/limitations.
- [x] Run `RUST_TEST_THREADS=2 scripts/check` after fixture and documentation cleanup.
- [x] Run the scoped packaged native workflow and report native delivery separately from physical gesture proof.

## Acceptance criteria
- [x] Only same-window, same-namespace, live-owner sources at the captured revision can drop.
- [x] Hover changes only the insertion cue; click still selects normally and a drag leaves selection/responder and live content intact.
- [x] Accepted slots match canonical order after removing the source and preserve group invariants; invalid slots show no cue and are rejected.
- [x] Packaged smoke requires the drag report key and verifies rendered order plus PTY/browser/responder retention.
- [x] Portable checks pass; native packaged execution is reported separately from physical gesture proof.

## References
- `.agents/tasks/2026-10-14/05-command-ux-parity/references/t03-multiwindow-design.md` — canonical order, group invariants, and existing native smoke contract.
- `.agents/tasks/2026-10-14/05-command-ux-parity/references/n02-workspace-context-menu.md` — current native ordering entrypoints and shared owner.
- `.agents/tasks/2026-10-22/01-workspace-drag-order/references/native-delivery-receipt.md` — pinned successful packaged native artifact and remaining physical-gesture limit.
