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

macOS 38751027370 at 700648d passed Rust/ABI and universal Swift compilation,
then reproduced the strict native drag failure through both launch paths. The
diagnostics showed correct divider hit testing and a 264-point resulting width,
but AppKit promoted the terminal responder to the divider during mouse-down.
The first `SidebarFocusWindow` repair rejected `makeFirstResponder(divider)`
inside pointer-down dispatch. macOS 38752187218 at 4cc3d55 passed universal
compilation, but both packaged launch paths then retained terminal focus while
leaving width/preference at 200 points. Rejecting the promotion also prevented
normal drag dispatch; this was not a successful repair.

The revised window scopes the divider's first-responder eligibility to false
only during that divider's pointer-down dispatch, restoring its prior value in
defer for nested events. NSWindow now owns the complete event-dispatch path with
no `makeFirstResponder` override. No retained responder is restored and no other
control's eligibility changes; keyboard traversal and explicit AX focus outside
pointer dispatch retain normal behavior. The same strict NSWindow-event smoke
remains unchanged.

macOS 38753451286 at 5f5005e passed Rust/ABI verification, universal native
compilation and packaged execution. The executed smoke receipt reports
`sidebar_resize: true`, `sidebar_toggle: true` and `success: true`, establishing
the strict native event, key-view, AX-action, preference, collapse and window-size
checks described above. Physical pointer/keyboard and VoiceOver acceptance remain
separate; the hosted smoke does not claim those manual OS interactions.
