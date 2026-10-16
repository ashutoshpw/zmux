# T01 terminal pointer reporting

## Discovered gap and owner

At main46422ef TerminalView mouseDown/mouseDragged always select text and there
is no terminal mouseUp or motion reporting. Wheel already reaches the session
worker through Command::Wheel and uses authoritative vt100 modes. Reuse that
bounded semantic input route; never send encoded pointer bytes from AppKit.
This is independent MIT work from public xterm protocol behavior, not a translation
of cmux or Ghostty implementation.

## Implementation sequence

1. Add semantic pointer button/action/cell/modifier input to the existing terminal,
   session and desktop owners. At processing time resolve X10 press-only, VT200
   press/release, button-motion and any-motion modes, plus default/UTF8/SGR encoding.
   Untracked input emits no bytes; completed/read-only sessions reject application
   input. Share wheel/report encoding rather than maintaining two encoders.
2. Publish optional tracking metadata for native selection-vs-application gesture
   routing. The worker remains byte/mode authority. Shift forces selection for the
   entire gesture; remember per-button routing until release. Right/middle presses,
   release and motion must preserve button identity. Suppress same-cell motion,
   bound queues, and handle leaving/reentering the view. No main-thread waits.
3. Connect native events through shared UICommand/NativeBridge dispatch. Preserve
   existing selection tests and history behavior. Do not claim optional-engine
   support until its worker has the same semantic contract.
4. Verify actual raw-mode child receives exact reports through desktop input and
   that application mode changes are honored without using the frame for encoding.
   Then verify actual AppKit event handlers, coordinate mapping and Shift selection
   using a focused packaged native fixture. Physical-device behavior remains a
   distinct acceptance gate.

## Behavioral verification design

Primary protocol regression belongs at the actual desktop/session/raw PTY boundary:
start a child that enables tracking, independently captures bytes and changes mode,
and submit semantic input through the public command route. Before implementation,
that route rejects unknown pointer commands; after it, the child must observe exact
public-protocol bytes. Cover mode-off no-input, press-only release suppression,
SGR release/motion, legacy/UTF8 bounds, and completed-owner refusal. Existing wheel
and keyboard tests cannot catch missing pointer dispatch. No test-only production
API or encoded-byte injection is required. Native fixture covers the distinct
AppKit routing/coordinate/selection risk rather than duplicating encoding tables.

## Status

Rust protocol/worker core committed as `1b6c0cd` after red regression `c53e150`;
parent verification passed 388 tests, strict Clippy, notices and C ABI checks.
Native fixture-only cohort now passes portable `scripts/check` and selection
input/AST validation; actual packaged native red and native repair remain pending.

## Worker dispatch delivered

Actual raw-child regression c53e150 fails because the public native command parser
rejects `pointer`, after the real child enables tracking. Original log:
`/tmp/zmux-pointer-red.log`. The repair adds a bounded semantic Pointer event to
the existing desktop/session queue, resolves modes at worker processing time and
shares mouse report encoding with wheel input. Completed sessions and the existing
completed-workload fence reject input; queued events after exit cannot write.
Optional Ghostty pointer reporting is explicitly refused before queue admission.

The same actual PTY fixture passes SGR press/motion/release, disabled mode, X10
press-only suppression, VT200, any-motion/no-button and UTF8 cases. Child-issued
mode changes are observed before submission; an ordinary trailing byte fences
suppressed reports. Existing wheel coverage preserves large and non-UTF8 cells.
Full scripts/check passes388 tests,1 existing ignored, strict Clippy/notices and
both ABI checks (`/tmp/zmux-pointer-check-r2.log`). The first full run passed tests
but Clippy requested an equivalent collapsed if in fixture polling; repaired.
Independent session-storage review found no scoped protocol/lifecycle blocker.
Native event wiring, published tracking hints and actual AppKit acceptance remain
pending. This worker slice alone does not enable native mouse interaction.

## Native regression design

The existing selection fixture gains a final `t` phase after all copy cases. The
actual raw child enables 1002+1006 and captures input until native Ctrl-X. Mounted
AppKit down/same-cell drag/moved drag/up must yield independently specified SGR
bytes; Shift-down followed by unshifted drag/up must copy `pointer` without adding
application bytes. Exact child PID/parent/nonce/size and raw capture are retained
separately. No tracking metadata or native pointer repair is included in this red
fixture cohort; the expected red is an empty capture from missing native routing.

## Native intended red and repair cohort

Fixture-only `fdc8d03` ran in macOS Actions run38857212985. Packaged app,
strict Swift6 metadata compilation and33 existing copy/Shift cases passed;
`application_pointer_capture` expected SGR reports and observed an empty capture.
Independent child16521/parent16517, nonce1f42f54c-abe8-4df6-9266-099cc556dfd8,
31x100 grid, Ctrl-X sentinel18 and empty trailing bytes match the native report.
All268 retained source hashes, packaged plist SHA, archive digest and archived
linked bridge/probe digest were verified. Local evidence:
`/tmp/zmux-pointer-native-red-verification-20261016.json` and
`/tmp/zmux-pointer-native-red-38857212985/`.

The native repair publishes optional typed pointer_tracking routing hints from
vt100, with nil for the unsupported optional engine. AppKit submits semantic
button/action/cell/modifier values through the shared bounded queue; only the
worker encodes bytes against its authoritative current mode. Each button keeps
its press-time selection/application/refused route. Shift selection stays local
through release; application gestures retain button identity across modifiers
and leaving/reentering the view. Duplicate motion compares cell and modifiers.
Responder/window/app focus loss attempts semantic releases through the same queue.

The synchronous callback reports UI queue admission only, never PTY delivery.
A refused press remains refused through release; failed motion does not advance
its suppression sample. Queue saturation/refused release uses the existing
visible command error, with no byte fallback, unbounded tasks, automatic replay
or delivery claim. Sustained native queue-saturation and physical-device
acceptance remain separate gates. AnyMotion requests native move delivery only
while a visible terminal needs it; a per-window owner set restores the previous
setting when the last terminal releases it.

The repair fixture preserves Shift copy and the real child's Ctrl-X delimiter,
and adds simultaneous right/middle identities, same-cell modifier changes,
outside/reentry clamping, actual responder cancellation and1003 hover. Portable scripts/check passes388 tests/72 groups,1 ignored, strict Clippy,
notices and both C ABI checks (`/tmp/zmux-pointer-native-repair-check-final.log`).
Native repair compilation/execution is still pending; portable checks are not
AppKit or physical-mouse evidence.
