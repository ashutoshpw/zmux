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
