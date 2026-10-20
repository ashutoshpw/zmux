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
- [x] Commit and deliver the change with accurate native validation evidence (`ddc5373161bd3d0b2f31a63ee3eec528fbc65c3c`).

## Acceptance criteria
- [x] Forward/backward navigation follows visual tab order and wraps in the focused pane.
- [x] A single tab remains selected; ordinary Tab and existing bracket shortcuts retain behavior.
- [x] Shortcut handling does not send Ctrl+Tab to the terminal application.
- [x] Native behavior, source checks and unavailable/manual checks are reported accurately.

## References
The replacement macOS workflow [39419255495](https://github.com/ashutoshpw/zmux/actions/runs/39419255495)
passed on source `ddc5373161bd3d0b2f31a63ee3eec528fbc65c3c`. The packaged artifact and detailed
verification receipt are recorded in [artifact-verification.json](references/artifact-verification.json).

The run verified the three Ctrl+Tab smoke receipts, the existing terminal-tab, close-tab, reopen-tab,
and shared-navigation-action receipts, three universal app binaries, 340 retained source files,
and strict code-signature validation. The app targets macOS 14+, is ad-hoc signed, and is not
notarized. Native save-dialog/accessibility coverage was user-deferred, browser-native downloads
remain excluded, and no physical user-Mac verification was performed.
