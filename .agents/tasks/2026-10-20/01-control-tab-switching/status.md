# Status

- **State:** in-progress
- **Updated:** 2026-10-20

## Notes
- Implementation and independent review are complete. Main-window callbacks reuse next/previous
  navigation; exact Control/Control+Shift matching preserves the existing bracket aliases.
  Unconfigured windows, marked text, sheets, and browser-owned automation events retain fallback
  or containment behavior.
- The packaged smoke now dispatches real NSApp events for forward/backward order and wraparound,
  bracket aliases, and focused-pane scoping. A bounded raw PTY reader verifies that x is the first
  received byte after both one-tab shortcuts, ordinary Tab is next, and y completes the probe;
  indexed receipts prevent delayed screen updates from satisfying a stale assertion. The reader
  restores termios before emitting its completion marker, including cancellation cleanup.
- `RUST_TEST_THREADS=2 scripts/check` passed with exit 0 at `/tmp/zmux-ctrl-tab-check.log`.
  Python syntax compilation and `git diff --check` pass. This Linux host has no Swift/AppKit
  compiler, so native compilation and mounted execution await the hosted macOS build.
- Review repaired fixture key-code mapping, unrelated close-history assumptions, and raw-reader
  ordering/cleanup before delivery. No production routing findings remain.
- User clarified the target is Zmux. Design accepted: main-window key-equivalent routing after
  owned browser-event containment, using the existing focused-pane next/previous actions.
- Existing menu validation disables those actions with one tab, so hidden menu aliases alone
  would allow Ctrl+Tab to reach terminal input. The main-window route will consume both shortcuts
  even when navigation is a no-op.
- Native regression extends the existing packaged smoke at NSApp dispatch and real PTY boundaries;
  it covers ordering, wraparound, pane scoping, bracket aliases, and a single-tab input probe.
- Base source is `64c3482`; unrelated dirty task/profile files are preserved.
