# Control-Tab switching

**Date:** 2026-10-20
**Task:** 01-control-tab-switching

## Objective
Ctrl+Tab selects the next tab in the focused pane; Ctrl+Shift+Tab selects the previous tab.
Preserve Cmd+Shift+bracket shortcuts and share the existing tab-navigation action.

## Context
The user clarified that the target is Zmux, not cmux. Implement original MIT code in this
repository. Preserve unrelated task/profile work and the user's running applications.

## Approach
Route exact Control+Tab and Control+Shift+Tab events through the main window's key-equivalent
handler after browser-owned event containment, then call the existing next/previous owner.
Extend the existing mounted native smoke through NSApp event dispatch, including a real PTY
input probe. Keep ordinary input and other modified-key fallbacks intact. Verify source and
portable checks separately from hosted native execution.

## Steps
- [x] Inspect native routing and accept a scoped design and behavioral regression.
- [x] Implement the aliases and meaningful native regression coverage.
- [x] Obtain independent review and pass `RUST_TEST_THREADS=2 scripts/check`.
- [ ] Commit and deliver the change with accurate native validation evidence.

## Acceptance criteria
- [ ] Forward/backward navigation follows visual tab order and wraps in the focused pane.
- [ ] A single tab remains selected; ordinary Tab and existing bracket shortcuts retain behavior.
- [ ] Shortcut handling does not send Ctrl+Tab to the terminal application.
- [ ] Native behavior, source checks and unavailable/manual checks are reported accurately.
