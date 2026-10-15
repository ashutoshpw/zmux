# Native browser keyboard feasibility

Original MIT diagnostic tool. This builds one new, uniquely identified macOS app;
it does not launch or modify Zmux/cmux, post global input, change TCC, use private
WebKit APIs, or provide a production keyboard implementation.

```sh
python3 -B tools/native-fixtures/browser-keyboard/run.py --check
python3 -B tools/native-fixtures/browser-keyboard/run.py --output /absolute/new/output
python3 -B tools/native-fixtures/browser-keyboard/run.py --debug-native --output /absolute/new/debug-output
```

The manual `Browser keyboard feasibility` workflow compiles with Swift 6 strict
concurrency/warnings and executes on macOS 15. Parent maintainers dispatch it only
after reviewing/pushing the exact source. Linux `--check` checks input availability
and hashes; it does not establish Swift compilation or native behavior.
The optional manual `debug_native` input runs the same compiled source under
LLDB with the same 60-second native bound, retaining all-thread stacks and process
status on a crash before killing that exact debuggee. It is a diagnostic, and
does not establish ordinary non-debugger behavior. Without that input, direct
process-group ownership is unchanged. A crash retains its last native phase and
distinguishes invocation from completed receipt validation. Only probe-named IPS
reports matching the exact executable path, unique bundle ID and recorded native
PID are read into retained crash evidence; no other application's reports qualify.

The app compares a visible selected WebKit host with a never-attached container
holding a real `WKWebView`, matching current production hidden hosting. The latter
must keep `window == nil`; the probe never mounts it in a window or borrows the
unrelated window's responder. Both use nonpersistent storage and real loaded HTML.
The probe activates only its own app/windows; the detached cases keep its unrelated
text editor key with `UNRELATED` text and exact responder identity unchanged.
The directly owned process asks Launch Services to activate its exact unique
already-running bundle without creating another instance or allowing substitution.
The returned PID must equal the probe PID. Actual activation is awaited for at
most three seconds, and actual visible document focus for at most two seconds;
refusal retains native/app/frontmost-PID observations and fails case preconditions.
This does not rely on the deprecated ignoring-other-apps activation option.

Public in-process `CGEvent`-backed `NSEvent`s go directly to the exact web view's
`NSResponder.keyDown`, `keyUp` and `flagsChanged` boundary. The app does not enqueue
those events through application/window event delivery. It observes actual trusted
DOM events, text/backspace/caret defaults, Space checkbox activation, Enter form
submission, separated down/up and a Shift transition/release. It preconstructs
paired events before delivery and uses no synthetic DOM fallback for failed native
keys. Event observers and assertions use actual DOM state, not injected success.

Owned events carry bounded process-local tags. Public application/window/webview
interception contains a tagged event if WebKit re-enters or asynchronously resends
it toward app shortcuts. Command-W is an explicit probe; unexpected Close/Quit
menu actions only increment an escaped-shortcut counter. They never terminate
another app. The receipt distinguishes synchronous re-entry from a resend observed
after direct dispatch. Zero observed asynchronous resends does **not** prove that
containment path; that gate remains open and the workflow fails explicitly.
The gate requires an intercepted asynchronous Command-W key-down originating in
an unhandled-command case; unrelated asynchronous events cannot satisfy it. Each
record retains the originating phase, key code, command flag and interception route.

`native.json` is updated before each phase, so a crash/timeout retains the last
reached boundary. The runner retains source/tool hashes, compiler version, bounded
stdout/stderr, exact binary hash, all 16 native cases and observed results. Native
execution is bounded to 60 seconds; compiler work to 120. The existing process-group
owner cleans up only processes spawned for this probe. Receipt size is capped at
1 MiB and intercepted event logging at 128 rows.

A failed detached case is useful architecture evidence: public direct WebKit input
has not met the hidden-host contract. Do not silently focus/remount a user browser,
post global events, relax trusted/default assertions or label a synthetic fallback
as equivalent. Even a successful probe does not establish production integration,
complete keyboard mappings, modifier lifecycle across host/document replacement,
IME/physical input, snapshot ordering, or command/edit shortcut safety.

Test authoring gate: the independent contract is actual native delivery/defaults
on a detached host without unrelated focus changes. Current DOM tests cannot
establish `isTrusted`, native editing/default behavior or AppKit resend routing;
public CLI/runtime tests cannot execute WebKit. This isolated executable uses no
production test seam. Its baseline hypothesis may fail natively, and such a result
must be reported rather than fixed by replacing observations with synthetic events.
