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
