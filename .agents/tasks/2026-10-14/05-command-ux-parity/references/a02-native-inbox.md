# A02b: native Agent Inbox and reply plan

Reviewed design and fixture-only implementation, 2026-10-16. Base: `8c54b39e797d826ccde8d181211d6c00e7df4cc1`.
Isolated worktree: `~/.worktrees/ashutoshpw/zmux/feat/native-agent-inbox`, tracking
`origin/main`. Only the dedicated regression fixture and its startup/workflow plumbing have changed; production inbox behavior remains absent pending actual native red.

## Scope and existing evidence

A02a now supplies one durable Rust message owner, the CLI, original request UUID
status, replies, exact-ID mark-read, settings, and neutral consumer leases. Native
presentation must use that owner. It must not call notification reply: the existing
`MainWindowHost.smokeNotificationReply`/`NativeBridge.notificationAction` route
intentionally writes a notification response to a PTY.

Inspected owners:

- `apps/macos/Sources/ApplicationCoordinator.swift`: one application-lifetime Rust
  Desktop, exact `MainWindowIdentity` presenters, async polling and physical shutdown.
- `App.swift`: one `MainWindowHost` owns each window's menus/sidebar; the current
  notification list is a distinct attention lifecycle. `MainWindowProjection.swift`
  validates canonical window/workspace membership.
- `NativeBridge.swift`: actor-isolated JSON decode; profile/notification receipt
  calls run on detached workers. Generic `send` returns queue acceptance only and
  retries Busy, so it cannot be the message mutation/outcome channel.
- `BrowserProfilesWindow.swift`/`BrowserDownloadsWindow.swift`: focused AppKit
  controller patterns. Do not copy the profile controller's unconditional pending
  reset: an uncertain message must retain its original UUID and draft.
- `crates/native-bridge/src/lib.rs` and `include/zmux.h`: result buffers are reserved
  before mutation. `desktop-runtime::Desktop::execute_native` is the existing trusted
  executor acknowledgement path, but messaging currently exposes no native receipt.
- Existing native selection/history runners retain exact compiled source/bridge and
  package hashes, own the child app PID, and test mounted AppKit state. Portable
  C ABI checks do not prove native first-responder or window behavior.

Public-contract inputs are the existing `audit-agents-remote.md` agent ownership
section, A02a design/implementation notes, and frozen cmux
`3ba653b9e5afb11d0bc578031142950d6136697e:docs/agent-messages.md`. The audit records
window-resolved quick-view, reply-field ownership, and duplicate-submit gating.
A focused routing check also confirms the scope: frozen
`Sources/ContentView.swift:10174` obtains installation-store messages and workspace
labels across all main-window contexts, while `AgentInboxProjection.swift:200`
projects those input messages without a window/workspace predicate. The window
resolved by `AppDelegate.swift:6191,15509` owns quick-view presentation, not a hidden
message filter. Search in the projection is textual. This records observable
scope only; no reference implementation, tests, UI prose, or assets are copied.

## Proposed architecture

1. Add a narrow trusted native message receipt API, separate from generic dispatch.
   One typed request/status family accepts only native UI operations: list, Send/
   reply, exact-ID mark-read, and settings. Consumer register/claim/ack are not UI
   methods. Reserve the C response buffer before enqueueing. Bound complete JSON
   input to the existing 16 KiB cap and responses to existing inbox limits.
2. The native request carries the expected namespace and exact invoking core window.
   Rust validates that origin on the executor, captures its model publication, and
   delegates to the existing `agent_messages::Runtime`. New admission refuses a
   missing/stale origin; it never borrows global focus. Status uses the original
   request UUID and returns the same receipt/uncertainty semantics as the CLI.
3. Resolve sender attribution in Rust from that exact window's selected live terminal
   at submission. If it has no live terminal, use explicitly labelled external local
   user attribution; never borrow a terminal from another window. The recipient is
   always the selected stored message's original `reply_to` UUID, with thread
   inheritance and exact sender-surface resolution already owned by Rust.
4. Add focused `AgentMessageModels.swift`, `AgentInboxCoordinator.swift`, and
   `AgentInboxWindow.swift` files. AppKit owns presentation/drafts and delegates typed
   operations to `NativeBridge`; it stores no independent inbox or delivery state.
   Keep the existing coordinator/bridge integration to small property, update,
   shutdown, and receipt-call hooks. `NativeBridge` owns its private handle; do not
   export that handle merely to allow an extension in another file.
5. One inbox controller belongs to each exact `MainWindowIdentity` (existing native
   capacity is two main windows). Its menu, sidebar button, and keyboard action route
   through one `showAgentInbox` path on the invoking host. A non-key window's explicit
   invocation must not open or retarget the key window's controller. Closing the
   origin disables its UI rather than silently attaching it to another host.

Following the observed reference scope, the first view shows the installation
inbox for the current namespace, anchored to the exact invoking window; rows retain explicit sender/recipient workspace labels
and immutable message IDs. An optional current-tab filter can reuse existing Rust
`List.surface`. Do not invent per-window message ownership from admission workspace
labels after a live move. If per-window record filtering is later requested, add
that projection at the Rust owner before pagination rather than duplicating target
routing in Swift. Closed recipients remain visible as retained queued/failed/read history with their
stored admission labels and failure reason; they are not reassigned to the current
window or to another live surface. Reply availability depends on the original
sender, separately from recipient closure. Rust remains the final authority and a
missing sender rejects reply without losing the draft. The native regression must
seed a second-window message and a closed-recipient message so the global list
scope and unavailable-history behavior are independently checked.

## Presentation and pending-state rules

- Show queued/delivered/read/failed truthfully. A queued reply is not labelled
  delivered and does not wake a provider. Plain selectable text preserves intentional
  Unicode, whitespace and line breaks; no HTML/script interpretation.
- Lists paginate through the existing bounded owner. Selecting/focusing a row does
  not mark future arrivals read. Explicit Mark Read captures only exact selected IDs.
- Drafts are keyed by namespace, invoking window, and parent message UUID, with at
  most 128 drafts of at most 4 KiB. Refresh, selection changes, hide/reopen, and focus
  changes preserve them. Cross-application-restart draft persistence is not implied
  by this first native slice; the durable messages themselves already survive restart.
- Reply submission captures parent ID, sender origin, text, draft revision, and a
  freshly generated request UUID once. Both button and submit-key paths enter the
  same mutation function. One pending reply owns that draft; duplicate input cannot
  create another send. Text can remain editable, but Saved clears it only if the
  current draft revision is the submitted revision, preserving later edits.
- Pending and uncertain outcomes keep the draft and original UUID. Status polling is
  bounded and cancellation/timeout does not issue another Send. The UI offers Check
  Status for that exact operation. `unknown_or_expired` remains unknown and never
  becomes a success or an automatic retry. Definite refusal allows deliberate edit
  and a new submission; an uncertain submission is not silently unlocked for replay.
- Surface/workspace/global settings all call the same Rust owner. Controls distinguish
  configured from effective receiving state and remain pending until durable commit.
  Turning on cannot resurrect failed messages. No UI-only boolean or automatic
  notification read/focus acknowledgement substitutes for message state.
- Future provider attempt metadata must remain distinct from the four message states.
  If the Rust owner later reports `delivery_unknown`, present queued with a
  Submission unconfirmed detail, never Delivered from transport submission. Reserve
  optional typed attempt details (provider/version/session, attempt UUID, exact IDs,
  digest/time; never bearer token) without implementing provider-specific UI or
  automatic retry in this slice. Explicit mark-read still uses immutable message IDs.
- Disable new requests during origin loss/shutdown. Cancel native polling/tasks while
  retaining the original mutation identity; Rust still owns accepted-write drain.

## First behavioral regression and authoring gate

Primary regression: actual packaged native app, two real main windows and PTYs,
private persistence/socket, and an independent input-byte counter. Seed a queued
message using the shipped CLI with exact sender/recipient identities. Make the
other window key, then invoke Agent Inbox from the explicit source host/menu route.

The fixture must compile on the baseline without new production inbox types: find
and invoke the public menu action dynamically, then inspect mounted AppKit controls
through their real accessibility labels/identities. The intended baseline red is
"Agent Inbox entrypoint unavailable" after the two-window, CLI, queued-message,
and PTY-ready prerequisites pass. A compiler failure or fixture/setup failure is
not the behavioral red. Commit that failing fixture before production repair.

Extend that same case after the entrypoint exists to verify:

- exact owning window, displayed immutable message/body, and reply first responder;
- a draft survives a real list refresh/new arrival and a focus/window switch;
- button plus submit-key double activation produces exactly one durable reply with
  exact body, parent/thread and original sender target despite changed global focus;
- Mark Read affects only exact displayed/selected IDs, leaving a later arrival queued;
- disabled settings fail queued work and refuse new sends; restoring on does not
  resurrect failures; no terminal input counter changes anywhere.

The four test-authoring questions are answered by this boundary:

1. Contract: actual mounted UI routes an exact durable message action through Rust,
   preserving draft and mutation identity without PTY input or focus substitution.
2. Credible regressions: missing native entrypoint, key-window fallback, row-index
   reply, refresh wiping text, duplicate click/key submission, queue-acceptance Saved,
   or reusing the notification PTY reply path.
3. Existing A02a CLI/worker tests cannot observe AppKit parent window, first responder,
   editable draft ownership, menu/key action sharing, or the typed Swift/C ABI path.
   Add a separate C ABI test only for distinct buffer capacity/ownership and receipt
   transport risks, not another copy of UI/domain behavior.
4. No production-only test export/injection is needed: use production menu actions,
   mounted controls/accessibility, the real ABI and CLI, actual worker/filesystem,
   and owned child processes. Fixture dispatch/source retention belongs to existing
   native runner plumbing and does not implement the asserted message behavior.

## Delivery and coordination

First land the isolated native fixture and obtain an actual macOS red through the
parent-owned native gate. Then implement trusted ABI/client, focused presentation,
and minimal shared integration; run the identical native case to green. Follow with
real failure/unknown-status and close/move cases at their strongest owner boundary.
Induce pending/uncertainty using the real journal lock and bounded native deadline,
not a fake successful receipt. Preserve original request-ID status through late commit.

Local checks: `scripts/check`, focused C ABI owner checks, and native runner input/
source-closure validation. On macOS: full Swift 6 strict-concurrency type check,
packaged-app run, screenshot/artifact, and actual shared shutdown. No native success
is inferred from Linux, a fixture build, or a green source inventory alone.

The browser owner is editing profile construction/mutation integration on
`ci/browser-profile-destructive-owner`. It expects narrow `NativeBridge.swift`
configuration/ABI calls and `ApplicationCoordinator.swift` construction/poll/shutdown
hooks. Our new APIs use agent-specific names and focused files; the parent integrates
shared-file deltas independently. Do not overwrite profile edits or fold provider
hook/wake work into this native gate. Parent owns the root plan/status link.

## Fixture freeze preparation (2026-10-16)

Parent approved the architecture, installation scope, authoring gate and first native
fixture. Added `AgentInboxSmoke.swift`, `tools/native-agent-inbox/run.py`, a dedicated
`agent-inbox` workflow choice/job, and narrow startup/storage/failure cleanup routing.
No inbox, message ABI, coordinator, bridge or provider implementation was added.

The case admits two actual window/PTY owners, seeds exact sender/recipient messages
using the bundled CLI, retains a closed third recipient, makes the sibling key, and
invokes the captured source NSMenu action. (The two live terminals remain distinct.)
It records missing Agent Inbox as behavioral red only after all prerequisites pass,
with app exit 1, no timeout, full logs and unchanged frozen sources. Setup/typecheck/
build failures do not qualify. After the entrypoint exists the same case selects the
immutable UUID row and checks global/closed visibility, exact body, source-owned
window and actual reply first responder. The broader reply/draft/read/settings
journey remains the next fixture extension after native baseline red.

Input counters advance independent select-loop heartbeat sequences every 50 ms.
After each UI boundary, the fixture requires two new cycles from each live child
before accepting zero input; no PTY bytes serve as synchronization. The closed
child's retained count remains checked. Runner compares final independent child
records as well as in-app samples.

Portable validation passed: `scripts/check` (workspace tests, Clippy, license
inventory, C ABI and profile-lease C ABI), `git diff --check`, and runner
`--check-inputs` (267 retained sources; native execution false). An actual Linux
PTY exercised the child: two new no-input heartbeat cycles, then deliberate exact
three-byte input observed. This validates the independent oracle only. Swift 6
compilation, packaged execution and true missing-entrypoint red remain unavailable
on Linux and must be recorded by the parent-owned Darwin gate before production
repair. Native source, archive, owned PID, child registrations and logs are retained.

Provider-owner coordination confirms future optional `delivery_attempts` and
`retry_authorized` metadata. Preserve four states and show a latest
`delivery_unknown` attempt as queued / Submission unconfirmed. Do not implement a
provider wake or automatic retry in this slice.
