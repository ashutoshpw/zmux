# T01 copying inside interactive applications

**Date:** 2026-10-24
**State:** done — direct Cmd+C repair and universal build verified

## Objective and scope

Investigate the user's intermittent failure to select/copy text while running
tmux or Claude in Zmux. The user described Ctrl+C on their Mac and believes cmux
works. Determine current behavior and missing paths before proposing a repair.
The initial source checkout was `dbb5ea6eb4509262f135c00ae7102c19529ad5ed`, with
application source matching the accepted `cbc7ac3` build. That investigation
changed only this durable report and its task-plan link. The user subsequently
authorized the direct-copy repair below. Preserve unrelated work, the installed
user apps, remote accounts and production tmux configuration.

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

The repair retains a separate native selection and one value snapshot of the
visible viewport while an admitted application-owned left gesture is forwarded.
Cmd+C prefers the ordinary native selection, then extracts text from this retained
snapshot. It does not depend on tmux buffer access or OSC 52, parse terminal escape
bytes in AppKit, or send any copy shortcut to the program. Row storage is shared
by Swift value semantics; full text extraction happens only on Copy, not motion.
An unchanged release retains the last drag snapshot across program redraws;
mouse-up movement still handles coalesced drag events. Typing, IME, paste, new
gestures/links, focus loss, scrolling, dimensions/history/alternate-screen changes
and terminal failure retire the candidate. Candidate data stays private to its
view and is never serialized into the control API. Native Shift selection remains
the visible override; arbitrary offscreen application buffer selection is outside
this visible-text contract.

The final regression also exercises actual Cmd+V followed by Cmd+C, proving that
paste retires the previous candidate while its literal bytes reach the raw child.
The expected child stream retains every original pointer report and adds only
the explicitly requested new drags, Ctrl+C byte 3 and paste content. No native
execution of the repair has completed yet.

## Verified regression and repair validation

Regression-only source `bca73920f4bd081dccfcf388b9b6c1dde56eb78a` ran in scoped
Mac workflow [40123041671](https://github.com/ashutoshpw/zmux/actions/runs/40123041671).
Both Swift source slices, the host app build and metadata contract passed. All
32 preceding clipboard cases passed. The new `pointer_drag_copy_without_shift`
case expected `pointer` and observed `ZMUX_SELECTION_SENTINEL`, the intended bug.
There was no timeout. Root downloaded artifact `12572462292` to
`/tmp/zmux-direct-copy-red-40123041671/` and verified the source/archive identity,
the exact failing case and that the production TerminalView still matched the
pre-regression source. The automatic full push run was cancelled in favor of this
scoped regression lane; no candidate from it is a delivered build.

The repair's full local `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at
`/tmp/zmux-direct-copy-fix-check.log`, including workspace tests, strict Clippy,
formatting, license inventory and both C ABI executables. The finite real-PTY
fixture input check and diff check passed again after the native edge extension.
Review identified that protocol coordinates clamp at the last column while copy
needs an exclusive endpoint one cell beyond it. Copy positions are now tracked
independently of suppressed/clamped reports, and a real rightmost `Z` in the child
screen must copy correctly without changing the expected application stream.
The fixture now adds seven direct-copy/lifecycle cases to the original 41.

Repair source `cf36588620d6f16431b09664ef74697e8fb4d0d7` is committed and pushed
to main; full workflow [40123955568](https://github.com/ashutoshpw/zmux/actions/runs/40123955568)
is running. Personal read-only post-commit review traced native/menu routing,
selection and snapshot lifetime, admitted/refused pointer gestures, right-edge
coordinate bounds, modifier changes, queue failure, paste/IME input, focus and
viewport invalidation. No actionable defect was found in this reviewed slice.
The seven added cases protect direct copy and candidate retirement while retaining
the complete prior pointer stream; native execution is still the outstanding gate.

Pending: final-source native verification and the verified universal artifact
download. Physical-user-Mac and actual Claude dogfood remain separate evidence.

## Accepted direct-copy build

Full Mac workflow [40123955568](https://github.com/ashutoshpw/zmux/actions/runs/40123955568)
completed successfully for source `cf36588620d6f16431b09664ef74697e8fb4d0d7`.
Both Swift architectures, both Linux helpers, full Mac Rust/C ABI validation,
universal build, actual terminal-selection/browser-link suite, packaged native
app/rendering/scheduler, sidebar and system-status-menu checks passed. Separate
dispatch-only jobs are skipped; the listed runtime steps executed in development.

All **48 native selection cases** passed. The seven new cases confirm forward,
reverse, coalesced and rightmost-character drags copy the expected literal through
Cmd+C without Shift, and typing Ctrl+C, clicking elsewhere and actual Cmd+V retire
the old candidate. The real raw PTY child captured the exact complete expected
mouse stream, literal Ctrl+C byte 3 and paste data. Original Shift selection,
Unicode/wrapped selection and browser-link contracts remain passing. All **16
packaged scheduler checks** also passed, including inspector retention and receipts.

Download [artifact 12572638113](https://github.com/ashutoshpw/zmux/actions/runs/40123955568/artifacts/12572638113):
`zmux-dev-universal-cf36588.zip`, 24,417,153 bytes, SHA-256
`c750d953919b0e028782b5e311e91f3270f685d8b56c2b74584fc4fec9dda8a6`.
Root downloaded it to `/tmp/zmux-direct-copy-delivery-40123955568/`, extracted to
`/tmp/zmux-direct-copy-app-40123955568/`, and verified archive/sidecar checksum,
matching bundled/outer build metadata and Info source/minimum OS, all four universal
Mach-O binaries, all three helper manifest hashes/modes, scheduler acceptance,
five Applied receipts and CAS protection. Verification also binds the completed
selection receipt/native report/raw capture, all seven direct-copy results, exact
new raw input prefix, and **360 retained source hashes** to the actual Git commit.
Receipt: `download-verification.json` in the downloaded artifact directory.

Universal Apple Silicon/Intel, macOS 14+, ad-hoc signed and not notarized. Quit the
older Zmux before opening this build. Normal dragging followed by Cmd+C now copies
visible text while application mouse input remains active; Shift remains the
explicit local-selection override. This does not implement OSC 52 or arbitrary
offscreen application-buffer access. Hosted protocol/native acceptance does not
claim physical-Mac or actual Claude-session dogfood. Final delivery receipt commit
changes documentation only; the app artifact remains tied to `cf36588`.
