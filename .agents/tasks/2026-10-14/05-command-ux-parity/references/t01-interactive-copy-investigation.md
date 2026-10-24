# T01 copying inside interactive applications

**Date:** 2026-10-24
**State:** implementation authorized; native regression pending

## Objective and scope

Investigate the user's intermittent failure to select/copy text while running
tmux or Claude in Zmux. The user described Ctrl+C on their Mac and believes cmux
works. Determine current behavior and missing paths before proposing a repair.
Source checkout is `dbb5ea6eb4509262f135c00ae7102c19529ad5ed`; application source
matches the accepted `cbc7ac3` build. Preserve unrelated work. No source/test,
installed-app, remote-account or production-tmux changes are authorized by this
investigation. Only this durable report and its task-plan link were added.

## Confirmed findings

1. **Ctrl+C is terminal input, not native Copy.** The Edit menu's Copy action uses
   Cmd+C (`App.swift`, menu and `copySelection`). `TerminalView.keyDown` clears
   the local selection before mapping Control-letter input to its ASCII control
   byte; literal Ctrl+C therefore sends byte 3 to the terminal. Native Copy only
   writes an existing Zmux selection and returns without changing the clipboard
   when that selection is absent. This is independent of which program is running.
2. **Mouse tracking changes ownership of the drag.** `TerminalView.beginPointer`
   sends ordinary presses/drags to a running application when tracking is active,
   clearing its own selection. The gesture route is latched at mouse-down. Shift
   held before starting a drag forces local selection for the entire gesture,
   including if Shift is released later. Shift-drag followed by Cmd+C is the
   current local-copy route while an application owns the mouse. A highlight drawn
   by tmux is not a Zmux selection, so Cmd+C cannot extract that application buffer.
3. **Program-requested clipboard writes are missing.** The vendored vt100 parser
   recognizes OSC 52 and dispatches `Callbacks::copy_to_clipboard`. The default
   callback is empty. Zmux's `Metadata` callbacks implement titles, notifications,
   working directory and program-open requests but never override clipboard copy,
   nor publish a clipboard event for the AppKit host. Thus OSC 52 writes reaching
   this engine are silently dropped. tmux can use this protocol to bridge its own
   copy buffer to the outer terminal clipboard, depending on its configuration and
   terminal capability. This is a confirmed missing capability, not confirmation
   that the user's particular tmux/Claude session emitted OSC 52.

## Comparison and verification limits

- Reviewed cmux's public configuration documentation only; no implementation or
  assets were copied. Its settings reference explicitly describes copies initiated
  by programs through OSC 52, and optional `terminal.copyOnSelect`. Zmux has no
  copy-on-select action in its pointer-release owner either. The user's personal
  cmux settings were not inspected, so automatic selection copying is only a
  possible explanation of the perceived difference.
- Re-read the accepted build's downloaded native report at
  `/tmp/zmux-right-inspector-delivery-39929941810/native-terminal-selection-universal/native-selection.json`:
  source `cbc7ac3ced002ff8a519d17c6b7c517c82396f21`, state `complete`, 41 cases.
  `pointer_shift_selection` expects and observes exactly `pointer` through mounted
  AppKit mouse handlers and the actual Cmd+C menu path under application tracking.
  This is existing same-source hosted proof, not a new physical-device reproduction.
- Existing coverage exercises the local Shift-selection route and raw application
  mouse reports. It does not establish actual Claude behavior, real tmux-to-system
  clipboard transfer or the user's current shortcut. No new tests were authored
  and no native execution was possible on this Linux host.

## Proposed next slice

Implement bounded, explicit terminal clipboard-write events at the Rust terminal
owner and deliver them to the correct native terminal host for AppKit pasteboard
writes. Preserve Ctrl+C interrupt semantics and the existing Shift override. Define
clipboard-write policy, supported selections, invalid/oversized input behavior,
event consumption and lifecycle/identity boundaries. Clipboard reads need separate
policy and are not necessary to repair this write path. Document Shift-drag plus
Cmd+C so application-owned highlights are distinguishable from native selection.

Test authoring gate for that slice: the independent contract is a real terminal
OSC 52 write reaching the system clipboard exactly once; the credible regression
is the current inherited no-op callback. Existing local-selection tests cannot
catch this missing output path. Protect protocol parsing/bounds at its real Rust
owner and use a distinct actual AppKit clipboard journey, including isolated tmux,
for transport and native delivery; no test-only production seams. Confirm the
current failure before the repair and retain Ctrl+C input and Shift-copy checks.

## Authorized direct Cmd+C repair

The user clarified Cmd+C, confirmed that Shift-drag works, and on 2026-10-24 asked
for ordinary dragging followed by Cmd+C to work. The local selection owner can
retain the dragged text while forwarding the original application mouse events.
This directly addresses the reported gesture without depending on application
clipboard support or changing clicks/scrolling into local-only gestures. OSC 52
remains a separate missing capability; the proposed output-event slice above is
not sufficient for arbitrary application-owned selection and is deferred here.

Plan: extend the existing actual AppKit/PTY acceptance with normal forward/reverse
and coalesced drags, preserve Ctrl+C input, and reject an old candidate after input
or a new click. Demonstrate the current failure, then retain bounded copy text in
the existing native owner. Invalidate on new gestures, terminal input, scrolling,
geometry/history/screen changes and focus/lifecycle loss. Preserve ordinary native
selection, link routing and application pointer byte contracts. Run portable and
hosted native gates, personally review the committed repair and deliver a fresh
verified universal build. No changes to remote tmux configuration or Claude.

Authoring gate: (1) ordinary drag plus the real Cmd+C menu must copy the literal
visible text while exact application mouse reports and Ctrl+C still reach the raw
PTY; (2) the current application route clears local selection, so the sentinel
clipboard survives instead of receiving `pointer`; (3) existing Shift-copy cases
explicitly bypass this route and cannot detect the gap; (4) reuse mounted public
AppKit events, the real menu and the independent raw PTY child, with no new
production test seam. Stale-candidate cases protect a distinct lifecycle contract.

Regression-only slice: `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at
`/tmp/zmux-direct-copy-regression-check.log`; the finite real-PTY fixture input
check and `git diff --check` passed. No production copy change is included in this
slice. Native compile and the expected clipboard-sentinel failure require the
hosted Mac lane; they have not been established by the portable check.
