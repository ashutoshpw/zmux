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
