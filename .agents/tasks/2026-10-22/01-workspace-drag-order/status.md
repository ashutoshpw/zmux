# Workspace sidebar drag ordering status

**Date:** 2026-10-22. **State:** source implementation, portable verification and packaged native smoke passed; independent review clear; compact sidebar scroll geometry validated on a native runner. Physical AppKit drag-threshold and OS gesture proof remain outstanding.

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

## Readiness fixture rerun — 2026-10-22

A prior native run at source `87f…` passed strict Swift, Rust, universal-build,
and selection checks, then failed the fixture before drag evaluation. Readiness
compared aggregate frame counts even though the new selected workspace replaces
the previous workspace's visible frames; cleanup also placed the `--window`
scope after the positional close-workspace target. At source
`9b92bbd7607063ca8c5dad3132ae14735ed94431`, readiness targets the created
workspace's focused-pane session and requires its `TerminalView` plus a matching
running frame with a process ID. Cleanup now places `--window` before the
positional workspace ID.

The full portable rerun passed at
`/tmp/zmux-workspace-drag-readiness-check.log`; focused verification passed
33/33. The packaged native run for source
`9b92bbd7607063ca8c5dad3132ae14735ed94431` is run `39772139796`, job
`119802044013`. Fixture setup completed, but the run failed its scroll
precondition before evaluating drag behavior. In that run, setting the clip
view origin with `setBoundsOrigin` and calling `reflectScrolledClipView` left
the observed origin unchanged. This is a fixture-specific observation and does
not establish that AppKit generally clamps scroll origins. Native drag-threshold
handling and the AppKit OS gesture remain unverified.

## Scroll geometry diagnostic — 2026-10-22

The first manual probe run, `39776999819`, compiled cleanly and measured a
260-point content height, 170-point viewport, 192-point document, and 22 points
of overflow. Its overflow scroll moved the clip origin from 8 to 16. The run
failed only its fit-state assertion: plain `scroll(to:)` moved the origin from
0 to 8 even though the document fit. That invalidated the probe's guessed
clamping precondition; it does not prove app drag/drop behavior.

Probe commit `b93a668bc3fbc11c286cb1ade1d25b4b4b63c016` now passes proposed
bounds through `constrainBoundsRect` before `scroll(to:)` in both fit and
overflow attempts, records requested and constrained origins, and resets the
overflow case to origin zero before testing movement. Manual run `39777707141`
(`119820781348`) passed strict Swift validation and the overflow case using the
real sidebar anchors: the document was 192 points high, the content viewport
170 points high, and the 22-point overflow allowed a constrained reset to zero
followed by an 8-point scroll to origin 8.

A second geometry probe passed at source
`d732587aad4f97757fd1087d09f6328c89b30e04`, run `39778466296`. At the selected
270-point content height, the same 192-point document had a 180-point viewport
and 12 points of overflow; the constrained reset and 8-point scroll reached
origin 8. This leaves the fixture's valid after-group target inside the visible
viewport. The exact geometry report is retained in
[`references/scroll-geometry-probe-270.json`](references/scroll-geometry-probe-270.json).

`WorkspaceOrderSmoke` now applies that 270-point content size only to its
existing CI window, lowers and restores that window's minimum size, and restores
its frame and sidebar origin from a defer even when fixture cleanup throws. It
waits for measured document overflow, scrolls from a legal constrained origin,
and checks that expanded-group and after-group targets are visible. The change
does not create additional workspaces or alter production sidebar code.
`git diff --check` and `RUST_TEST_THREADS=2 scripts/check` pass; the full
portable-check log is `/tmp/zmux-workspace-drag-scroll-check.log`.
After preserving the successful geometry report and removing the one-off probe
workflow, `git diff --check` and `RUST_TEST_THREADS=2 scripts/check` passed
again.

The patched packaged smoke passed at source
`87dfbfe51bcbbb16289ecbf53e37e04f0ea01642`, run `39779731513` (job
`119827593515`), and the retained artifact's `workspace_order_drag` and
`workspace_order` report values are true. The screenshot and full artifact
receipt are in
[`references/native-delivery-receipt.md`](references/native-delivery-receipt.md).
The geometry probes do not exercise the smoke's destination callbacks,
AppKit drag-threshold dispatch, or the OS drag loop; physical gesture behavior
remains unverified.
