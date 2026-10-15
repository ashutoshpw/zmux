# N01a desktop notification delivery

## Scope and ownership

Original MIT implementation of native macOS notification delivery. Rust owns the inbox,
notification epoch, workspace namespace, surface/session identity, permission generation
and delivery eligibility. AppKit/UserNotifications adapts an authoritative bounded desired
snapshot into OS requests. Scheduling acceptance is not proof a banner was displayed.

- Attention owner: `crates/desktop-runtime/src/attention.rs`.
- Runtime integration: trusted native APIs and current-model reconciliation.
- Native owner: `NotificationDelivery.swift`, App lifecycle/menu and confirmed bridge.
- No socket or generic native JSON may forge permission or OS callback messages.

## Required behavior

- Captured epoch, namespace and session prevent stale notifications from opening a
  replacement terminal after restore/restart. Moved live surfaces remain addressable.
- Read, clear, coalesce and closed targets remove pending and delivered OS requests;
  late permission/scheduling callbacks cannot resurrect them.
- Permission denial keeps inbox entries and unread indicators. Explicit enable is
  user initiated; background processing never prompts. Activation refreshes permissions.
- Click/open selects the exact validated target before native window activation.
  Dismiss acknowledges only the original notification generation.
- Native queues, callbacks and timers remain bounded. Network/OS waits stay off Rust
  workspace execution and asynchronous native waits never block UI rendering.

## Verification and remaining gates

Owner-boundary and real-runtime tests must cover stale restore/session identity,
coalescing, permission generation and callback races, with meaningful red/green proof.
Native compile and packaged smoke establish integration only. Actual OS allow/deny,
revocation, visible banners and click/dismiss require a macOS user session; simulated
callbacks do not establish that acceptance.

Reply, scoped CLI metadata/bulk operations, hooks, persistence and multiwindow behavior
remain separate N01 gates. N01 and V01 stay open.

## Implementation evidence

- Parent full portable check passes: 150 Rust tests, formatting, strict Clippy, notices
  and foreign C ABI. Fresh Rust/ABI review accepted exact identity and native isolation.
- Existing inbox-open restore regression reproduced before repair, now passes against
  real persistence/runtime. Native callback tests cover confirmations and forged JSON.
- Fresh native review repaired permission transitions dropping an intervening denial
  and direct inbox clicks bypassing action limits. Shared action cap and ordered
  restriction barrier accepted on re-review. The permission smoke uses the production
  observation owner and actual Rust event API; hosted execution remains pending.
- Swift/UN compilation and visible OS permission/banner acceptance remain open.

## N01b metadata and scoped operations

- `notify` accepts default title, subtitle/body and explicit desktop preference; inbox
  presentation includes subtitle. A native CLI journey checks subtitle rendering and
  confirms desktop=false creates no native delivery desire.
- Shared executor handles scoped mark-read, dismiss/all-read and clear, including
  caller-workspace defaults and focus-independent global operations on empty desktops.
- Each row captures namespace/workspace identity; same-live-session moves update its
  workspace before closure. Scoped mutations return affected IDs/count and a captured
  cutoff. Listing exposes monotonic cursor and epoch; stale epoch guards are available
  on the typed mutation protocol.
- Real CLI/socket and runtime tests cover malformed grammar, cross-workspace safety,
  moved/closed history, empty-desktop operations, cutoff and permission policy.
- Workspace-level notification destinations, UUID notification selectors and reliable
  replies remain N01c/followup gates; a workspace selector alone still targets its
  focused surface. No full N01/V01 claim is made.

## N01c workspace destinations

Notification tokens gain a discriminated destination: workspace ID, or surface ID plus
captured session ID. Common epoch/namespace/notification ID remain immutable. Workspace
rows expose null surface_id; surface rows retain numeric IDs for compatibility. Surface
tokens follow their live session across moves; workspace tokens never borrow a focused
terminal. Native receipts identify the selected workspace as well as the selected surface.

Notification-only resolution preserves explicit scope separately from caller evidence.
A same-workspace live caller surface is accepted; a foreign caller cannot escape an
explicit workspace. External calls without surface evidence target a workspace. Opening
a workspace notification retains its selected pane/tab; closed owners and stale epochs
remain unavailable. Shared bulk scope includes both destinations for workspace operations
and excludes workspace-only rows for surface operations.

Runtime regressions failed at the real owner before implementation (borrowed numeric
surface instead of null). The public CLI regression independently failed because explicit
workspace parsing discarded a hidden caller surface (returned selected9 instead of8).
Parent full checks pass: 169 Rust tests, formatting, strict Clippy, notices and C ABI.
Fresh production review accepted ownership, scope and activation behavior. Native smoke
external-caller environment isolation is being repaired; hosted execution remains pending.

## N01d next slice: handles and unread navigation

Frozen cmux observable contracts accept bare notification UUID and notification:UUID
for open/read/dismiss, and navigate to the newest openable unread notification. Evidence:
CLI/cmux.swift:23020 and :24420; NotificationNavigationCoordinatorTests.swift:124 in
Packages/macOS/CmuxNotifications/Tests/CmuxNotificationsTests. Independent implementation
will capture UUID at admission from epoch plus a versioned notification identity name,
retain numeric compatibility, and resolve handles only within bounded attention history.
One shared executor action will skip unavailable rows, preserve unread on failed open,
and return opened:false without changing focus when no candidate can open. CLI, native
menu/inbox/shortcut must share it; window activation follows only an explicit confirmed
request. Epoch changes and coalescing must not reuse old handles.

Reply remains a later slice: queue admission is not PTY delivery. Exact opt-in terminal
replies require bounded text, once-only worker receipts, no replay after partial writes,
and read acknowledgement only after delivery. Workspace/browser targets cannot borrow
a focused terminal. Frozen behavior evidence: NotificationDeliveryCoordinatorTests.swift:565.
Last-prompt navigation requires an agent UserPromptSubmit source, not shell prompt or
keystroke recency (Sources/AppDelegate+JumpToLastPrompt.swift:5 and
cmuxTests/JumpToLastPromptTests.swift:20). Rich manual/restored unread state and badge
settings also remain open. No implementation for these followup items is claimed.
