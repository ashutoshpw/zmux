# Status

- **State:** in-progress
- **Updated:** 2026-10-20

## Notes
- The test-only completion marker now writes the original wrapper exit status to a temporary file
  and atomically renames it. Independent review found no issues; the exact focused test passed
  all five scenarios (1 test), and `RUST_TEST_THREADS=2 scripts/check` exited 0 at
  `/tmp/zmux-ctrl-tab-status-publication-check.log`. The exact `0` assertion remains unchanged.
- Hosted run `39417663909` passed native Swift source checks and helper checks, then failed the
  unchanged CLI agent-message fixture before app packaging: wrapper exit status was empty rather
  than `0`. The producer creates `wrapper-status` before its printf writes, while the consumer
  waits only for file existence. A scoped test-only repair will publish the completed status via
  temporary-file rename; the strict exit-code assertion remains unchanged. No native shortcut
  execution has occurred on this run.
- Source `301fe9d3b012ffc3b0e672719525d43e72c71352` is pushed to `origin/main` with remote
  parity confirmed. Hosted macOS run
  [39417663909](https://github.com/ashutoshpw/zmux/actions/runs/39417663909) is queued on this SHA.
  Native validation and a source-bound universal artifact remain pending.
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
