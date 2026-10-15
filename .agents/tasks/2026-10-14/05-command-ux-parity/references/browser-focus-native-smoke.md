# Native browser document-focus fixture (2026-10-15)

Run 38820239578 at `1d8c3f9b0afc6c3887452c30d1b7ecf3ebaab7bf`
passed the existing sidebar resize/persistence journey and failed later with
`Terminal-to-browser navigation did not focus WebKit`. Browser keyboard smoke
was not reached. The original stderr did not describe the actual responder, so
the exact native focus failure remains unproven. Its separate diagnostic launch
failed earlier during sidebar pointer resizing with a non-key window; those
results are not interchangeable.

The retained T03 contract initializes a new blank browser with address intent,
then preserves the last actual native address or document responder through
layout and selection changes. The older broad fixture creates a blank browser,
loads/evaluates HTML programmatically and expects document focus after a tab and
pane roundtrip without ever establishing native document focus. Loading HTML or
mutating an input value does not imply a native document-focus request.

The repair explicitly makes the loaded WebKit view the first responder and
asserts that exact responder before the existing hidden-tab roundtrip. The final
terminal-to-browser assertion remains exact. Shared failure diagnostics describe
the actual responder type, key-window state, browser attachment and visibility.
Production focus policy is unchanged. WorkspaceNumberSmoke retains its separate
real Command-L/address-selection restoration checks and its new-blank-tab address
editor assertion; none is relaxed or removed.

Source/whitespace verification on Linux cannot establish AppKit/WebKit routing.
Strict native compilation and the same packaged journey remain required; another
native focus failure may remain after correcting this known setup mismatch.
Physical keyboard, full IME and browser keyboard delivery are separate gates.
