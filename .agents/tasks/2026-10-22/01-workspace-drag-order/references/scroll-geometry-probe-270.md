# Compact sidebar scroll geometry probe

The manually dispatched AppKit geometry probe passed on 2026-10-22 at workflow
source `d732587aad4f97757fd1087d09f6328c89b30e04` (run `39778466296`). The exact
JSON report is in [`scroll-geometry-probe-270.json`](scroll-geometry-probe-270.json).

At the 270-point compact content height, the flipped document measured 192
points and the scroll viewport measured 180 points, leaving 12 points of
overflow. A constrained reset stayed at origin 0; a constrained 8-point scroll
was observed at origin 8. At the 420-point minimum height, the 192-point
document fit within the 330-point viewport; a requested 8-point scroll was
constrained to origin 0. All six report checks passed.

This isolates valid scroll geometry for the compact fixture. It does not exercise
the app's drag destination callbacks, native drag-threshold handling, or
AppKit's operating-system drag loop, so it is not evidence that a physical drag
works. The one-off workflow used to run this probe was removed after preserving
its report here.
