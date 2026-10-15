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
external-caller environment isolation was repaired; hosted execution remains pending.

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

N01d is implemented locally. The public CLI regression first failed at the missing
admitted UUID field; the six public CLI notification tests now pass. Parent scripts/check
passes 172 Rust tests, formatting, strict Clippy, notices and C ABI. Fresh review accepts
UUID ownership, deletion receipts and shared navigation; native gates remain open.

## Confirmed native callback crash

Hosted `2dcb038` (run38736550307) compiled universally but crashed during real
UserNotifications permission refresh. LLDB captured the UN service call-out queue
entering `_dispatch_assert_queue_fail`, Swift executor validation, and
`NotificationDelivery.refreshPermission`'s callback. This establishes inherited
main-actor isolation at the Objective-C completion boundary as the crash cause.

The repair makes all three framework completion blocks explicitly Sendable. Settings
become a Sendable enum and errors become bounded strings before the main-actor hop;
all delivery state remains actor-owned. No unsafe actor assumption or bypass is added.
The same packaged workflow is the native regression gate; portable checks cannot
establish the repair. Actual banners/reply behavior remain separate open gates.

## N01e reply delivery phases

1. Add optional once-only write receipts to the existing bounded session input queue.
   The PTY worker reports full local-PTY acceptance, no bytes written, or incomplete
   writes with uncertain effect. One absolute deadline covers queue delay and writes;
   shutdown, errors and dropped work finalize ownership. No input replay, side queue
   or UI/core executor waiting is permitted. Local PTY acceptance is not proof that a
   remote SSH program consumed the bytes.
2. Capture terminal-only reply opt-in at notification admission. A bounded desktop
   pending-reply owner polls receipts and acknowledges read only after full write and
   exact epoch/namespace/session validation. Replies cannot borrow focused terminals,
   and consumed generations cannot resend. Native completion waits off the UI thread.
3. Add a native text-input action only to eligible reply requests, with Sendable
   callback values, bounded UTF-8 text including terminator, and exact result handling.
   Empty/failure fallback must not resend input or falsely acknowledge delivery.

All three phases are now implemented; evidence and remaining native gates follow.

## Hosted acceptance of N01a-d integration

At `c9db69c9b75a79c7082b09108d7d894f88b2f319`, Foundation38739600484 and macOS
38739600592 pass. The latter executes universal compilation and the real packaged
permission-ordering, CLI metadata, workspace-target and unread-navigation journeys.
The previously failing UN callback path now completes. This verifies the callback repair
and packaged integration, not visible OS banner/reply acceptance or full parity.

Parent downloaded the diagnostic archive, checked source/evidence agreement, ZIP
integrity and SHA256 `744dd7853507e967566d82c0c509d3efa64079b6b8979e288ad567a54883acec`.
The AppKit rendering and separate WebKit content images were inspected. Full window
capture returned status1 on CI; the AppKit bitmap does not capture embedded WebKit
content, while the separate WebKit PNG shows the loaded CSP fixture. No full-window
compositor/physical UX claim or user-facing artifact handoff is made.

N01e phase1 now passes parent full checks (175 Rust tests plus formatting, strict
Clippy, notices and C ABI). Fresh review accepts queued cancellation/start CAS,
once-only results, bounded ordering and shutdown cleanup. Three real-PTY cases cover
FIFO delivery receipts, expired queued writes remaining absent after reader resume,
and active backpressure/deadline/shutdown results. The deadline case failed before
repair by returning Complete after expiry. Panic fallback is source-reviewed rather
than fault-injected. No desktop/native reply support is claimed by this phase.

Phase1 hosted follow-up: `6cea10a` passes Foundation38741118473 and macOS38741118480,
including real-PTY receipt tests on Darwin, universal compilation and packaged smoke.
This validates the session primitive across both hosts; reply UI/core integration
remains the next separate gate.

## N01e integrated reply acceptance

The CLI now admits explicit terminal-only `notify --reply`; the Rust owner claims
each exact notification generation once and polls up to eight write receipts without
blocking the workspace executor. Rust trims bounded UTF-8 text and appends one CR.
Complete local PTY writes acknowledge read without changing focus. Only Empty or
NotWritten may use a separately validated exact-target open fallback. Partial,
unknown and expired responses never resend input or acknowledge read. Consumed
generations withdraw native delivery rather than rescheduling a changed banner.

The native adapter exposes a text-input category without foreground activation and
validates exact-target, bounded byte-count receipts. The packaged smoke exercises
the real Swift bridge and PTY with surrounding whitespace, verifies canonical byte
counts and terminal output, and checks read state and preserved background focus.
Visible OS banner interaction remains a separate acceptance gate.

Parent `scripts/check` passes 180 Rust tests, formatting, strict Clippy, notices and
C ABI capacity/ownership checks. CLI opt-in failed before parser support; runtime
opt-in and canonical trimming have recorded red/green owner evidence. Hosted
compilation and packaged reply acceptance remain pending for this integrated slice.

Fresh review identified expired completion slots retained while a PTY receipt remained
empty. The owner now releases those slots as Unknown at the response deadline;
late input cannot trigger a second completion or acknowledgement. This exceptional
worker-stall path is source-reviewed; real PTY deadline/backpressure tests remain
the behavioral coverage, without introducing a thread-stall test seam.

Fresh independent review accepted the expiry repair with no remaining source blockers.
Parent full checks pass again after that repair.

Hosted `920a2ab` Foundation38743883285 passed and macOS38743883235 compiled
universally. Packaged acceptance failed in the reply smoke: it searched visible-only
native frames for a deliberately background terminal. The repair reads that exact
surface through the existing public `read-screen` command, validates surface/session
identity and output, and retains the read/no-focus assertions. No renderer exposure
or selection change is introduced. Native reply acceptance awaits this rerun.
