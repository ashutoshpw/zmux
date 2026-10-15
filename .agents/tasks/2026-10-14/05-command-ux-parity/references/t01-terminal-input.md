# T01a legacy special-key input

Current native input handles arrows and basic navigation but omits F1–F12 and
drops modifiers on Home/End/PageUp/PageDown/Delete. Implement an original
legacy xterm encoder shared by those native key paths. Keep Command shortcuts,
scrollback navigation, marked-text composition and ordinary text on their existing
paths. This does not claim extended keyboard protocol or complete terminal parity.

The acceptance owner is the actual packaged TerminalView feeding a real PTY.
Dispatch native key events and inspect exact independently specified bytes at the
child process; do not mirror a private encoder helper in tests. Cover function-key
families, modifiers and application-cursor behavior while preserving existing
shortcut/IME ordering. Physical keyboard layouts and IME remain separate gates.

F04 engine comparison and measured resource acceptance remain open. This slice
adds no dependencies, changes no parser, and imports no reference implementation.

The independent encoder now emits F1–F12 and modified cursor/tilde sequences.
The existing native Command/history/marked-text guards execute first; ordinary
text, control characters, Tab, Return, Backspace and Escape keep their routes.
Fresh source review found no blocker. Parent full checks pass181 Rust tests plus
formatting, strict Clippy, notices and C ABI; these do not compile Swift.
The real PTY fixture protocol was exercised on Linux, while native event delivery
and the pre-fix/post-fix byte comparison remain hosted macOS gates.

The attempted hosted pre-fix run (38746667499, da33cea) stopped at an unrelated
sidebar Swift API spelling error before executing the key-byte journey. It does
not establish the intended native red result; that evidence remains absent.

Hosted receipt: Terminal special-key byte acceptance executed successfully at5f5005e in
macOS38753451286 (universal build and packaged smoke). Parent downloaded and checked
archive/source/required flags. This closes that subset native gate only; full package
parity and applicable physical/external-host gates remain open.

The primary LaunchServices log from macOS 38757351964 at d3d5f05 contains a
separate setup failure before any special key was sent, despite the run later
being cancelled by a push. The exact fixture read-screen identifies mode 0,
surface 41/session 42, the mounted terminal as first responder, a key window,
normal cursor mode and zero scrollback offset. Python rejected corrupted source
at line 28 with an unterminated-string SyntaxError; the expected `finally:` area
contained repeated `fina` fragments. The original 1244-byte multiline command
crossed the native bridge's 1024-byte input boundary. This proves corrupted setup
source reached the child, but does not identify the shell, line discipline or
Zmux input writer as its cause. The inactive direct diagnostic's sidebar failure
is separate evidence.

The key smoke now creates its unchanged Python capture source in an owner-private
0700 temporary directory and 0600 file off MainActor, sends a short quoted launch
command, and awaits source cleanup on success and failure. Actual NSWindow key
events, the ordered NUL sentinel, exact independent xterm byte assertions and
all readiness/capture deadlines remain unchanged. The file-backed raw PTY
protocol passed on Linux for both cursor modes and exact captured hex; this
does not establish Darwin event acceptance.

Multiline transport remains an open investigation rather than a repaired
production input claim. A separate session-owner test sends a multiline argument
through two 1024-byte admissions to real interactive Bash and Zsh, then compares
the Python child's captured file bytes with the independent original payload.
Both installed shells pass on Linux with the private build target:

```sh
cargo test --locked -p zmux-session-runtime --test lifecycle \
  interactive_shell_preserves_exact_multiline_input_across_native_chunk_boundaries \
  --target-dir /tmp/zmux-special-key-transport-target -- --nocapture
```

This is a Linux pass, not a reproduction or closure of the observed Darwin
corruption. macOS execution of that transport test and the revised packaged key
smoke remain required. No production session-input behavior was changed.

The unchanged multiline setup and exact special-key assertions subsequently passed
at 948e922 in macOS 38759082621. The downloaded smoke/build/resource receipts name
that exact SHA, and the archive SHA-256 matches its published checksum. Its
`terminal_special_keys: true` flag is emitted after both cursor-mode hex assertions;
raw per-key hex is not separately archived. This intermittent baseline pass does
not close the earlier observed corruption or validate the later file-backed setup.
The latter remains a separate native gate at its own source SHA.

Native528be92 run38764217483 failed Swift warnings-as-errors in the nested fixture
cleanup's inferred detached-task expression. The repair explicitly types the task as
Task<Void, Error> and awaits its value separately. Removal still runs off MainActor
and cleanup is awaited; hosted compile remains required before accepting this fix.

The explicit-task attempt did not fix native38765601658: Swift still reports both
no-async/no-throw diagnostics at the nested function's task.value access. The next
candidate removes that nested function and uses a nonisolated member helper,
matching working fixture helper boundaries. A first-step standalone Swift diagnostic
reports whether the minimal nested form reproduces, then requires the extracted
form to compile and await off-main deletion/error propagation. This probe cannot
substitute for full App.swift compile or establish the compiler's root cause.
