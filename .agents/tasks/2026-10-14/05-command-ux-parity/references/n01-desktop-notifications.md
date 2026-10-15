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
