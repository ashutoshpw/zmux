# Terminal scrolling and attention repair

Baseline: cmux `3ba653b9e5afb11d0bc578031142950d6136697e`, zmux `2e14276`.
Behavior reference only: cmux GhosttyTerminalView.scrollWheel forwards precision/momentum
input to its terminal engine; TerminalNotificationStore separates focused/read signals
from unread attention; Workspace and notification dismissal hosts scope acknowledgement
by surface. No implementation is copied or translated.

## Corrective slice

- Worker-owned bounded history navigation, visible offset/extent in snapshots, redraw
  even without output, cursor/selection handling, trackpad accumulation and wheel input.
- Shift-PageUp/PageDown and Command-Home/End navigate history. Ordinary PageUp/PageDown
  remain application keys. Wheel input in alternate screen routes to the application;
  mouse tracking uses negotiated encoding and pointer coordinates. Shift overrides mouse
  reporting for history. Typing/paste returns to live output. Exited sessions retain history.
- Preserve history anchoring during output and bound history to 1000 lines. Resize and
  alternate-screen transitions must not leak history between sessions or corrupt the parser.
- Selecting/focusing a surface or activating its window acknowledges existing attention
  only for that focused surface. Opening a notification still acknowledges its exact ID;
  other event generations survive. Output is activity, not an attention event.
- Verify hidden-tab OSC/BEL, focused/inactive delivery, closed destinations, badges and
  acknowledgement through owner tests and the native Actions smoke run.

## CLI parity repair plan

Full help parity is still open; the current help accurately describes only shipped behavior.
The 428-entry inventory is discovery, not implementation evidence. Do not add successful
stubs or advertise missing commands as supported to make help appear complete.

1. Expand the frozen ledger into leaf commands, aliases and every option from the supplied
   help and CLI dispatcher, including wildcard families. Each row records syntax, defaults,
   environment fallbacks, target resolver, shared owner, output/error contract, dependency,
   behavior test and status. Audit flags as well as command names.
2. Build shared target resolution (UUID/ref/index, caller environment, windows, panes,
   surfaces/tab aliases), focus/no-focus policy, JSON/ref/UUID output and socket auth.
   Preserve explicit unsupported-platform/dependency errors and socket-free help.
3. Navigation first: open/path launch, new-window/select/close, workspace actions/order,
   new-pane/new-split/split-off, surface/tab actions/order/moves and rename aliases. Route
   CLI, menu and shortcut mutations to the same owner. Validate each advertised option.
4. Attention/automation: notify reply/desktop flags, read/open/dismiss/clear scopes,
   jump-to-unread/last-prompt, send/send-key/paste/panel aliases, status/progress/log,
   events/cursors, todo, hooks and rules. Test exact bytes and stale-event races.
5. Inspection/customization: identify/current/tree/top/memory/health/selection/scrollback,
   config/settings/reload/docs/guide/shortcuts/themes/import/sidebar, captures/diff/markdown,
   reviews/vault/checkpoints, session restore/fork/export and local tmux/zellij.
6. Browser: implement native effect/reply transport then navigation, selectors, snapshot,
   eval/repl/wait/input, frames/dialogs/downloads, profiles/cookies/storage/tabs, console,
   screenshots/state/scripts/styles and advanced modes. Public WebKit limitations stay open.
7. Agents/remote/platform: messaging/inbox/hibernation/provider hooks and team wrappers;
   auth/team/cloud/VM, remotes, SSH/mosh/reconnect/session moves; simulator/iOS, sudo,
   diagnostics and all tmux compatibility/advanced leaves. Cloud depends on the deferred
   host/database decision; deployment stays Actions-only at zmux.w3dev.app.
8. Generate grouped top-level/task help and command help from the validated command registry
   as each family ships; add Targets, Environment, Socket Auth and agent guidance when
   their contracts exist. Completion requires behavioral evidence for every ledger row,
   native checks and a downloaded Actions artifact, not matching help text length.

Commit each working family separately after scripts/check, retaining open phase status.

## Local evidence (2026-10-15)

The desktop scroll regression failed on baseline with unknown command `scroll`; the
attention regression failed with `activation must acknowledge the focused surface`.
Both now pass through real PTYs. Parser coverage checks anchored/bounded history,
resize, alternate-screen isolation and exact legacy/UTF8/SGR mouse bytes. Session coverage
checks scroll-after-exit, preserved exit status and rejection of further process input.
Mouse protocol routing runs on the parser worker, using its current mode; UI snapshots
cannot cause stale-mode routing or UTF8 corruption of legacy mouse coordinates.
Native Actions smoke now exercises wheel, Command-Home/End, input return to live output,
selection-based acknowledgement and a later hidden-tab OSC arrival. Hosted gate pending.

Still open: physical trackpad feel/momentum, comprehensive terminal-engine parity,
focused-read indicators, desktop OS notification delivery/preferences, persistence,
agent hooks and command-completion notifications. Current attention source contracts
are BEL, OSC 9, OSC 777 and explicit local notify. Plain output is never guessed as
agent completion. The full CLI backlog above remains in progress.

First hosted run 38692498847 passed Rust/ABI checks but rejected an unused Swift binding
under warnings-as-errors. Remove the obsolete binding; rerun the unchanged native gates.
