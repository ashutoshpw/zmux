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
- Original split placement now survives individual pane closure and last-tab pane removal.
  Bounded private descriptors retain ancestor axes/order and sibling IDs, with no topology or
  terminal-output snapshots. Restore wraps the nearest surviving original/remapped sibling
  subtree; missing anchors fall back safely within the recovered workspace, or create a new
  workspace when the owner disappeared. Surviving IDs/sessions and subtree layout are retained.
  Split depth/pane capacity rejection is atomic; grouped launch failure removes provisional
  surfaces and leaves history retryable, including chained remaps after failed launches.
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
  Hosted universal compilation and packaged smoke passed; see [native receipt](native-delivery.md).
  Interactive drag/IME/accessibility evidence remains open.

## Current split-anchor verification (2026-10-15 working tree)

- `cargo test -p zmux-workspace-core --test restore_anchors`: initial four behavioral
  regressions failed before the repair and passed afterward. Seven final cases cover original
  axis/order and surviving subtree identity, selected grouped tabs, last-tab pane removal,
  ancestor/missing-anchor fallback, recovered containers, launch rollback/retry, chained remap
  retry and atomic split-depth rejection with retry after freeing capacity.
- The chained remap retry case independently failed on the first implementation, then passed
  after alias repair. `cargo test -p zmux-workspace-core` passes 33 tests; scoped Clippy passes.
- Existing runtime launch ownership is unchanged; these are core owner checks. Native macOS
  reopen interaction and the full product parity gates still require separate evidence.

## Remaining P1 and P0 gates

Richer focus history, workspace groups, no-focus policies, UUID/typed-ref targeting,
multiple windows and window recovery remain open. History age/byte budgets and persistence
remain follow-ups. Browser descriptors join this model in P3. No browser, attention inbox or
sidebar collapse is claimed by this slice. Exhaustive P0 contract audit and measured Mac resource
budgets are still open; no concurrency limit increase was made.
