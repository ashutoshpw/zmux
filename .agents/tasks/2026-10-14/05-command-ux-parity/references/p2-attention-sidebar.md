# P2 first slice: sidebar and in-app attention

## Delivered contracts

- Sidebar toggles from an icon or ⌘B; visibility is saved in zmux preferences. Terminal grids
  resize in place. Workspaces and Notifications are distinct sidebar tabs. Header bell remains
  reachable while collapsed and opens/expands the inbox. Cloud controls remain independent.
- Rust coordinator owns bounded notification state. BEL, visual bell, OSC 9 and OSC 777 signals
  carry per-session sequence numbers through parsing; ordinary output never creates attention.
  The parser retains 32 signals; coordinator retains 128 notifications and reports omissions.
- Explicit CLI notifications target a live surface. Titles/bodies are bounded and sanitized.
  Focused/key-window signals are recorded as read; hidden targets and inactive windows unread.
  Workspace, visible-tab and header badges project unread state. Closing/moving tabs cannot
  redirect a notification to another terminal.
- Repeat unread messages within one second coalesce into a new generation/ID. Stale read/open
  IDs reject; clear uses an observed upper ID so later arrivals survive. Open selects the exact
  live surface then marks only its notification read; missing targets fail without marking read.
- Native inbox offers open/read/dismiss and clear controls. Closed targets show unavailable.
  CLI adds notify, list-notifications, mark-notification-read, open-notification,
  dismiss-notification and clear-notifications. Positional/flag differences from cmux remain open.

## Verification

- `scripts/check` on Linux covers independent parser bytes, real hidden PTY signals, active/
  inactive policy, input-vs-attention separation, exact-target open, stale close destinations,
  history bounds, repeat generations and cursor-safe clear. Actual CLI/socket integration
  validates notification creation, listing, open acknowledgement and dismissal.
- Native smoke includes ⌘B resize/restore and actual OSC → inbox → target/read behavior.
  Hosted compilation/smoke is pending; manual VoiceOver, interactive focus/drag, macOS desktop
  permissions and measured resource/remote-stress budgets remain open.
- The preceding P1 macOS run failed on a /var versus /private/var temp-directory alias in the
  saved-cwd test. Canonicalizing that fixture resolves the platform identity mismatch; the
  cwd failure/retention checks remain intact.

## Remaining P2 work

Desktop OS notification delivery, agent/shell hook integration, configured completion policy,
manual unread controls, prompt/unread navigation, status/progress/log families, persisted
attention state, sidebar width resizing/per-window state and multiwindow behavior remain open.
This slice does not complete the whole P2 phase or the general CLI parity inventory.
