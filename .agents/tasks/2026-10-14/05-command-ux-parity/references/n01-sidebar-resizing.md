# N01 sidebar width

Original AppKit implementation; no cmux implementation or assets imported.

`SidebarLayout` remembers an expanded width in UserDefaults (`sidebarWidth`),
defaulting to 224 points and bounded to 200–480 points. The displayed sidebar leaves
at least 400 points for main content when the window permits it. Window resizing
only clamps presentation; growing the window restores the remembered width.
Collapse/expand uses the same geometry path and preserves the remembered width.

An eight-point divider supports pointer dragging, Left/Right Arrow while focused,
and accessibility increment/decrement. All three use one width mutation. The divider
participates in the native key-view loop and exposes its current width as an
accessibility splitter value. Mouse/AX resizing preserves the existing responder;
keyboard resizing retains divider focus. Geometry changes issue no target selection.
Collapsing while a sidebar control/divider has focus moves the responder to the
already selected terminal or browser; existing content focus stays unchanged.

The existing packaged sidebar smoke journey now dispatches mouse and arrow events
through NSWindow, traverses the key-view loop, exercises AX actions, reloads persisted
width, collapses/reopens, and shrinks/grows the window. It checks visible widths,
main-content space, responder retention, collapse from divider focus, and unchanged
workspace/pane/tab selection.
Its defer restores window geometry, responder and original preference values.

This extends the real native boundary rather than adding helper tests or test-only
production seams. Existing toggle-only coverage did not exercise resizing or persistence.
Linux source/diff inspection cannot establish native compilation, event routing or
VoiceOver behavior; hosted Swift 6 compilation and packaged execution remain required,
with physical pointer/keyboard/VoiceOver acceptance still separate.

macOS38746667499 atda33cea stopped at compilation: the C spelling
`NSSetFocusRingStyle` is unavailable to Swift. The repair uses the SDK-designated
`NSFocusRingPlacement.only.set()` API. No native resizing acceptance is claimed
from that run; the same packaged journey must execute on the repaired source.
