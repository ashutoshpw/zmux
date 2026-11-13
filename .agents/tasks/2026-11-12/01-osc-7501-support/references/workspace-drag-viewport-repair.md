# Workspace drag viewport fixture repair

**Date:** 2026-11-13 UTC

**Scope:** `WorkspaceOrderSmoke.validateSidebarDrag` fixture geometry and this
reference only. No production UI, drag callback, scheduling, workflow, task
status, or plan changes.

## Accepted diagnosis

Universal run `43386617074` at source `9f2f13c3f6742ef8b69af10f6a75a9960ef88682`
passed the Foundation jobs, app build, terminal-selection/browser-link smoke,
and all 16 scheduled-message checks. The scheduler RPC summary retained 69
records. The first failure was the later packaged workspace-order smoke:

```text
Expanded workspace group drop points are outside the visible sidebar viewport
```

The failure is at the simultaneous visibility guard in
`validateSidebarDrag`, before the first `draggingEntered`, `prepareForDragOperation`,
or `performDragOperation` call. It therefore establishes a fixture viewport
failure, not a general production drag-callback defect. The fixture had lowered
the live window minimum to 180 points and requested a 270-point content height.
The production window sets its minimum frame to 680×420 in `App.swift`; the
sidebar reserves 32 points for chrome, a 4-point top gap, and 54 for the footer.
That leaves 180 points for sections, while the production sidebar stack needs
264 section points. The compressed layout therefore cannot provide the intended
Spaces viewport for the expanded header and anchor targets.

The earlier compact geometry probe is useful only for the scroll invariant: at
270 content points it measured a 192-point document and 180-point viewport, so
the document overflowed by 12 points and accepted an actual 8-point scroll. It
did not exercise the app's sidebar stack or drag callbacks and is not treated as
proof of this repair.

## Repair

`validateSidebarDrag` now retains the `NSWindow.minSize` captured on entry,
converts that minimum frame size to its corresponding content height with
AppKit's `contentRect(forFrameRect:)`, and sizes the real window content from
that result. This removes the 180/270 fixture overrides and avoids duplicating
the app's minimum as a second constant. The existing cleanup still restores
the saved frame, minimum, and clip origin.

The fixture still requires document height to exceed the viewport by more than
8 points and verifies that the requested 8-point scroll produced at least one
point of real clip-origin movement. The simultaneous three-point visibility
guard is unchanged. If a future native layout still misses it, the failure now
includes the three points, sidebar visible rect, clip bounds, document height,
window frame, and minimum size. The real `NSWindow`, `NSScrollView`, clip view,
workspace rows, callback checks, group-start cue, order/group identity,
selection, key window, first responder, live process/browser identity, and
cleanup assertions remain in place.

## Test-authoring gate

1. **Observable contract:** The packaged native smoke must make the expanded
   group header and anchor drop points visible, then exercise the existing
   sidebar drop callbacks and preserve canonical order, group membership, live
   selection/focus, processes, browser identity, and fixture cleanup.
2. **Credible regression:** Lowering the test window minimum below the sidebar
   layout's chrome/footer/section requirements can clip the first expanded
   group targets before any destination callback is called. This can cause the
   reported precondition failure; the exact failing rectangles were not retained,
   so post-repair native execution remains required.
3. **Coverage boundary:** Rust and portable checks do not own AppKit window
   sizing, `NSScrollView` clipping, or this packaged native smoke path. This
   existing packaged smoke remains the single owner; no duplicate test was
   added.
4. **Production seam:** None. The fixture uses the app's existing window,
   scroll hierarchy, rows, and sidebar callbacks. No production API or test-only
   hook was introduced.

## Validation and limits

- Read-only source audit confirmed the app's 680×420 minimum, 32-point sidebar
  chrome, 54-point footer, fixture cleanup, pre-callback guard location, and
  existing scroll and identity assertions.
- `git diff --check` passed for the Swift source, and the new reference passed a
  trailing-whitespace scan.
- This Linux host has no Swift compiler or macOS SDK. Swift type-checking,
  native AppKit execution, post-repair packaged acceptance, and a fresh hosted
  build were not run here. The earlier geometry probe and the historical
  scheduled-message pass do not establish those results.
- The existing `scripts/check-macos-sources` preflight was attempted and exited
  at its platform guard with `Native source checks require macOS and its SDK`.
- No scheduler source or claim about the cause of any historical scheduler
  timeout is part of this repair.

Parent accepted the small sizing diff: the app-owned minimum is used as the
fixture floor through AppKit conversion, the original scroll and drag guards
remain, and only bounded numeric geometry is added to the existing failure.
The full local aggregate is running before commit.
