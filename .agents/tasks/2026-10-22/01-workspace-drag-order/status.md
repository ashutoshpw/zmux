# Workspace sidebar drag ordering status

**Date:** 2026-10-22. **State:** source implementation and portable verification complete; independent review clear; packaged AppKit execution pending.

The sidebar now captures the workspace UUID, namespace, window, canonical source
index and revision on row mouse-down. Crossing the drag threshold starts a move
only while that token still describes the same attached row in the same live
sidebar and window. Ordinary row selection remains on mouse-up, so beginning a
drag does not select or focus another workspace.

The sidebar destination rechecks the source owner, window, row membership,
pasteboard token, current revision and live workspace mapping. Payloads are
bounded before decoding. Insertion positions are translated through the
flipped, scrolled document view and are offered only when removing and
reinserting the row preserves each group's membership, contiguity and anchor.
Accepted drops use the existing revision-bound command queue; the AppKit drop
callback reports success only when that queue accepts the action. The drag
preview captures the rendered row bitmap.

Independent production review cleared with no remaining blockers.
`WorkspaceOrderSmoke` exercises real row mouse-down/up and the sidebar's
destination callbacks, including insertion geometry, grouped and collapsed
destinations, stale/foreign rejection, canonical plus rendered order, and live
PTY/browser/responder retention. For its synthetic drop it captures the source
token through row mouse-down, then delivers fake `NSDraggingInfo` to the real
destination callbacks. It does not drive `mouseDragged` past the threshold,
call `beginDraggingSession`, or exercise `NSDraggingSource` in AppKit's OS drag
loop. The smoke therefore does not establish physical gesture dispatch. It sets
`workspace_order_drag` in its report, and `scripts/check-macos-app` requires
that key. `git diff --check`, the Python syntax check for
`scripts/check-macos-app`, and `RUST_TEST_THREADS=2 scripts/check` pass; the
full portable-check log is `/tmp/zmux-workspace-drag-check.log`. The
collapsed-header smoke point is one point above the exact midpoint so it
resolves to the group-start slot.

Packaged macOS compilation and execution have not run in this Linux environment
because `swiftc` and `xcrun` are unavailable.
