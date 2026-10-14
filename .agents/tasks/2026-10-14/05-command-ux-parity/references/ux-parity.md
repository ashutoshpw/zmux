# Full interaction parity, beyond CLI help

The user clarified that ⌘T was an example, not the boundary of requested parity.
The CLI inventory alone cannot establish product parity. Audit all application menus,
context menus, shortcut settings, command palette entries, titlebar/sidebar controls,
drag/drop routes, terminal-engine bindings, browser controls and session/recovery flows.
Unbound shortcut actions are in scope too. No feature is excluded merely because it is
absent from `cmux --help` or was not explicitly named by the user.

## Source evidence added in this review

Pinned cmux SHA remains `3ba653b9e5afb11d0bc578031142950d6136697e`.

- `Sources/KeyboardShortcutSettings.swift:579–582`: dedicated closed-workspace action
  plus ⇧⌘T for the action labeled Reopen Last Closed. Its legacy internal browser name
  does not mean the behavior is browser-only.
- `Sources/cmuxApp+HistoryMenu.swift`: focus back/forward/last, recently focused,
  reopen closed workspace/item, recently closed and restore previous launch are menu flows.
- `Sources/ClosedItemHistory.swift`: distinct panel/workspace/window records with
  placement and snapshots; `Sources/AppDelegate+ClosedItemHistory.swift` coordinates restore.
- `Sources/TabManager.swift:5085–5148`: reopen chooses restorable history, selected history
  item can be restored, failed restore retains its history record, and focus is restored.
- `Sources/Workspace+ClosedPanelLayoutRestore.swift` and
  `Sources/Workspace+SessionRestoreIdentity.swift`: layout and identity restoration are
  responsibilities beyond creating a new empty terminal.
- `Sources/WorkspaceCloseTabsBatching.swift`: close-other-tabs has a confirmation path.
- `Sources/TabItemView+WorkspaceContextMenu.swift`: pin/unpin, read/unread, copy workspace
  ID/link, move to window, reconnect/disconnect and close-other-workspaces entrypoints.
- `Sources/KeyboardShortcutSettings.swift:85–240`: discovery includes focus history,
  workspace groups, pane traversal/resize/equalization/zoom, canvas layout, file/search,
  terminal copy/input and browser interactions. Exact enabled states/defaults require P0.

`ui-action-inventory.json` records every action identifier in the shortcut enum, with a
source pointer and initial phase. It is discovery coverage, not implementation evidence.
P0 must reconcile it with non-shortcut entrypoints and the command ledger; add missing rows.
Use source only to derive independently authored behavior contracts, never copy code/tests.

## Required UX contracts

| ID | Behavior to inventory and implement | Phase | Observable acceptance |
| --- | --- | --- | --- |
| UX-01 | New terminal tab, select/cycle/by-number, rename and reorder | P1 | ⌘T adds a sibling surface without splitting; state and input target remain correct. |
| UX-02 | Close tab, close others, batch close and confirmation/cancellation | P1 | Only intended resources close; cancellation leaves topology/history untouched. |
| UX-03 | Reopen last closed terminal/browser tab via ⇧⌘T and History menu | P1/P3 | Correct descriptor, placement and focus return; repeated undo follows history order. |
| UX-04 | Recently Closed selection; reopen closed workspace/window | P1 | Group closure restores the grouped layout; individual history item can be selected. |
| UX-05 | Previous-launch/session restoration and recovery | P4 | Distinct from close undo; versioned persistence handles invalid/missing state explicitly. |
| UX-06 | Focus back/forward/last and recently focused history | P1 | Correct window/workspace/surface activates; deleted targets do not misroute focus. |
| UX-07 | Move tab within/between panes/workspaces/windows; split off; drag/drop | P1 | Live session identity survives moves; old containers collapse correctly. |
| UX-08 | Pane directional focus, resize, equalize, zoom/unzoom and auto layout | P1 | Geometry and PTY size agree; zoom restores the prior layout without losing tabs. |
| UX-09 | Workspace pinning, groups, multiselect, reorder, collapse, metadata/status | P1/P2 | Menu/shortcut/drag outcomes agree; selected/unread state survives regrouping. |
| UX-10 | Sidebar visibility, width, keyboard focus and right-sidebar tool modes | P2/P4 | Toggle is always reachable; layout/focus/preferences restore correctly. |
| UX-11 | Attention inbox, manual unread, next unread and prompt navigation | P2/P5 | Notifications resolve to their exact live target; attention differs from activity. |
| UX-12 | Terminal scrollback, find/next/previous/selection search, copy mode and clear variants | P5 | Search and copy act on intended history; clear-screen does not silently erase retained scrollback. |
| UX-13 | Font zoom/reset, keyboard customization/conflicts and reload | P4 | Scope/defaults match contract; changed bindings route through the same actions. |
| UX-14 | Browser address focus, navigation/history, tabs/popups, find, zoom and profiles | P3 | Native browser UX works independently of CLI automation and preserves profile boundaries. |
| UX-15 | Command palette, global/workspace/file search and file explorer actions | P4/P5 | Keyboard discovery, selection, disabled states and focus restoration behave correctly. |
| UX-16 | Canvas overview/zoom/tidy/alignment/distribution | P8 | Audit layout mode and commands; retain as a gap until implemented and measured. |
| UX-17 | Diff/markdown/editor navigation, file attachment, screenshot paste and text input tools | P5/P6 | Real content actions, clipboard and file permissions; no success-shaped placeholders. |
| UX-18 | Window lifecycle, full screen, hide/show, multiwindow routing and links | P1/P4 | Correct window receives actions; last-window behavior and recovery are specified. |
| UX-19 | Remote reconnect/disconnect and resumed-session UX | P7 | Connection failures never recreate the session locally or falsely report recovery. |
| UX-20 | Simulator/device and other platform-only controls | P8 | Audit separately from command names; platform dependencies remain tracked gaps. |

Each row must enumerate gestures, shortcuts, menu/context/palette entries, enable/disable
conditions, focus behavior, persistence, errors and native evidence. Phase assignments are
initial ownership; conflicts must be reconciled in P0 rather than silently dropping actions.

## Reopen closed terminal tabs: required design and tests

Ship close/reopen together with P1 terminal tabs; do not postpone basic undo-close to the
later cloud or generic persistence phases. Browser descriptors join the same history in P3.

- Shared Rust-owned closed-item history represents terminal/browser surface, workspace and
  window closures. Capture restore descriptors and placement before destructive cleanup;
  history cannot depend on native view objects remaining alive. Bound count, bytes and age.
- ⇧⌘T and History → Reopen Last Closed invoke one restore action. Offer Recently Closed and
  an explicit closed-workspace action. The history is ordered, handles mixed item types and
  grouped closes, and avoids duplicate parent/child entries from one cascade.
- Preserve original cwd, title, supported session configuration, tab order and pane/layout
  anchors; browser entries preserve URL/profile and supported navigation restoration data.
  Specify what scrollback is retained, its limits/privacy and how restored text is identified.
- When the original pane/window is gone, use a documented deterministic fallback. Resolve
  stable-ID collisions against live objects; never steal another session or notification target.
  Restore focus to the reopened item and keep focus-history traversal coherent.
- Reopening a closed local PTY creates a new shell using the saved descriptor; it cannot
  resurrect a killed process. Do not automatically replay arbitrary terminal input or commands.
  Live remote/tmux sessions may be reattached only when their identity/availability is verified.
  Record the upstream observed behavior and any unavoidable semantic gap explicitly.
- Commit history consumption only after successful restoration. Failed spawn, missing cwd,
  corrupt descriptor, unavailable browser profile or disconnected remote leaves a retriable
  entry with a visible failure. Close/restore races cannot double-spawn or leak workers.
- P1 supports in-process close history. P4 adds persistence/privacy/clear-history behavior
  after checking the upstream contract; undo-close, session restore and crash recovery stay
  separate acceptance scenarios.

Required tests: close A/B then restore B/A; select a specific history item; terminal and
browser mixed order; last tab/pane/workspace/window; close-other-tabs confirmation and
cancel; missing original container; renamed/moved neighbors; identity collision; failed
restore/retry; double invocation; restore during pending close; bounded history eviction;
missing cwd; remote unavailable; local job is not silently rerun. Native smoke must use
actual ⌘T/⌘W/⇧⌘T/menu input and verify selected tab, cwd and independent surviving PTYs.

## Completion gate

Maintain two linked ledgers: CLI contracts and UI interactions. Every menu/context/shortcut/
palette/gesture path must map to an implemented shared action and a behavioral test, or stay
explicitly open. P0 discovery is not complete until all those surfaces have been traversed;
this static source review does not claim that the entire cmux UX has already been audited.
