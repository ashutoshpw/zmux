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

Read-only discovery complete; isolated worktree `feat/terminal-pointer` created
from46422ef. Implementation and behavioral red/green evidence remain pending.
