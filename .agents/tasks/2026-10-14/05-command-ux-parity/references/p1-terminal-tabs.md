# P1 first slice: terminal tabs and close history

## Delivered contracts

- Model v2: split panes own ordered stable surfaces and selected surface IDs. Legacy pane CLI
  operations project the selected terminal. The function ABI and request envelope remain v1.
- Native ⌘T/plus creates a sibling terminal, ⌘W closes one tab, ⇧⌘T restores the latest item,
  ⇧⌘[/] traverses tabs. Tab context menus rename, close and confirm close-other; drag moves
  live tabs between panes. UI and CLI use the same Rust actions.
- Close history is capped at 32 in-process entries. Surface, pane, batch and workspace closures
  record descriptors. Grouped workspace restoration retains split axes/order and selected pane.
  Restore uses new IDs and fresh shells at saved cwds; terminal input is never stored/replayed.
- History is consumed after real launch success. Failure rolls back the entire provisional
  group and retains history for retry; double restore and mutation of restoring tabs reject.
  Related histories share remapped recovered containers; unrelated live objects are untouched.
- Resource admission retains the eight-terminal limit, counting hidden, pending and retiring
  workers. Metadata copies do not clone inactive terminal output. Blocking launch stays off UI
  and coordinator threads. Late aborted-group completion is retired.
- CLI additions: new-surface, list-pane-surfaces, select-surface, close-surface, rename-tab,
  reorder-surface, move-surface, close-other-tabs, close-workspace, reopen-closed, recently-closed.
  Positional positive-integer syntax is documented; this does not claim upstream option parity.

## Verification

- `scripts/check`: passed on Linux, including model, real-PTY lifecycle, actual CLI/socket,
  Clippy, formatting, license inventory and foreign C ABI checks.
- New model tests own ordering, movement, atomic rejection/capacity, grouped restore, history
  eviction, ID collisions, failed-group rollback, retry and duplicate/late completion.
- Real PTY tests prove hidden process survival, input isolation, move PID preservation, cleanup,
  fresh-shell restoration, saved cwd, missing-directory failure and retry after recreation.
- The missing-directory test initially failed: portable-pty silently selected HOME. Explicit
  cwd validation in the session owner fixes the tested preexisting-directory failure case.
  A concurrent filesystem change between validation and spawn remains a dependency limitation.
- Packaged macOS smoke now exercises native menu shortcuts and sibling PID preservation.
  Hosted native compilation/smoke and interactive drag/IME/accessibility evidence are pending.

## Remaining P1 and P0 gates

Removed individual panes currently restore grouped tabs into a new workspace; original split
anchors, richer focus history, workspace groups, no-focus policies, UUID/typed-ref targeting,
multiple windows and window recovery remain open. History age/byte budgets and persistence
remain follow-ups. Browser descriptors join this model in P3. No browser, attention inbox or
sidebar collapse is claimed by this slice. Exhaustive P0 contract audit and measured Mac resource
budgets are still open; no concurrency limit increase was made.
