# A02b: native Agent Inbox and reply plan

Reviewed design and implementation in progress, 2026-10-16. Base: `8c54b39e797d826ccde8d181211d6c00e7df4cc1`.
Isolated worktree: `~/.worktrees/ashutoshpw/zmux/feat/native-agent-inbox`, tracking
`origin/main`. The dedicated regression was committed before production repair; actual Darwin missing-entrypoint red is now verified. Production inbox work is in progress and native green remains required.

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

## First Darwin setup result and narrow repair

Run [38854068017](https://github.com/ashutoshpw/zmux/actions/runs/38854068017), exact
`54f6bf67a607cfe30ef9fa346dce515a9159329e`, did **not** establish behavioral red.
Both Swift 6 slices passed (93.598 s); the actual arm64 package built (226.101 s).
The owned app exited 1 without timeout after two-window/PTY, exact CLI messages,
closed-recipient history and independent zero-input prerequisites passed. It then
timed out making the sibling window really key, before the entrypoint assertion.
The runner also incorrectly compared uppercase Swift UUID text with lowercase child
UUID text. Raw child identities agree semantically and all three input counts are 0.

All 267 retained source hashes match the local freeze and retained source bytes.
Retained archive SHA256: `26a8b97c34ee08234d3c61bb016b473286fe4b41ac03f477431939650c7d3798`.
Local artifacts: `/tmp/zmux-agent-inbox-red-38854068017/`.
Parent approved only two setup repairs before another baseline run: exact-owned
LaunchServices activation (existing native fixture pattern, PID checked, substitution
forbidden) and semantic UUID comparison. Keep the actual key/responder checks and
all original prerequisites. No production inbox code has been added.

## True native red and production repair in progress

Run [38854940772](https://github.com/ashutoshpw/zmux/actions/runs/38854940772), exact
`63e6ab9fc8a26d946397a08debd13084d38ca2ca`, establishes the intended red. Both
Swift 6 source slices passed (94.877 s); the arm64 package built (254.369 s).
The owned app exited 1 without timeout (5.638 s) after all prerequisites passed,
with exact failure `Native agent inbox: Agent Inbox entrypoint unavailable` at
stage `entrypoint`. Activation PID equals owned app PID 11866; real source/other
windows are 5/6; three independent children report zero input bytes. Raw source,
phase logs and artifacts are in `/tmp/zmux-agent-inbox-red-38854940772/`.
All 267 retained source hashes equal the frozen manifest and retained bytes.
Archive SHA256: `3b26899cb02d1d445d51c979c57fff7dbc1097f763f35064ef5c4cdaf65fef85`.
Parent authorized production repair immediately after this true red.

Implemented owners, pending native compile/green:

- `agent_messages/native.rs`: narrow List/Reply/exact-ID MarkRead/Settings/Status,
  exact namespace/window checked on the executor. Status returns before selected
  sender resolution. Only Reply derives sender from that window's selected model
  terminal; no selected terminal means external local user. Explicit settings
  targets prevent another window's ambient context becoming a fallback.
- C ABI reserves output capacity before mutation; generic desktop deserialization
  cannot construct the trusted command. NativeBridge keeps its handle private,
  executes one transport call off the UI thread and never automatically replays it.
- Typed models/client, bounded shared draft owner and one controller per exact
  window. Menu/sidebar/keyboard share `showAgentInbox`. SF Symbol toolbar buttons
  have explicit accessible labels; selection is visible; First Page returns from
  pagination. Unchanged UUID rows retain their actual AppKit controls.
- One pending mutation captures original UUID and parent/body/revision. Saved
  clears only that unchanged draft. Status transport refusal cannot discharge an
  already accepted uncertain mutation; only its authoritative receipt can do so.
  Receiving settings capture scope/target/selection generation and refetch before
  enabling controls if the selected recipient changed during the save.
- Provider attempt metadata is optional read projection only. A registration is
  not liveness evidence; no provider invocation/wake, retry, Ack or ReadTurn added.

The same native case now extends through actual text editing, CLI new arrival and
Refresh, hide/reopen with sibling focus, button plus Command-Return submission,
newer draft editing during Save, exact parent/thread/sender/recipient and one saved
reply, explicit selected-ID read leaving the arrival queued, global receiving off,
refused native send with draft retention, and on without resurrecting failures.
An independent runner-owned process then holds the actual journal flock: pending
submission retains its original UUID; Check Status cannot send again. Real global
response-buffer exhaustion refuses a status transport while the original remains
uncertain. After releasing capacity and the actual lock, that same UUID resolves
one durable reply and clears only its unchanged draft. Fresh child progress cycles
establish zero terminal input after all UI actions. A mounted AppKit PNG is retained.

A separate foreign C caller protects the distinct transport risks: exact UUID
receipts; strict host-only method set; stale origin refusal; and output capacity
refusal leaves durable global settings Off and creates no operation for the refused
On request. Its first failure was fixture setup (zero-revision unchanged snapshot),
not another product red; setup was corrected and the foreign test passes. Existing
CLI/worker tests remain the domain owner; they are not copied into Swift/C tests.
A real Linux process probe verified the independent flock holder's exclusion,
identity, unlock and subsequent reacquisition. Source inventory is now 273 files.
Native compilation, the expanded actual journey, PNG review and final same-source
shutdown remain pending. Do not claim feature completion from portable checks.

Portable implementation verification: `scripts/check` passes with 379 Rust tests,
1 existing ignored, Clippy, license inventory and all three foreign C ABI callers.
Log `/tmp/zmux-native-agent-inbox-implementation-check-20261016.log` SHA256
`f8643a796b359df6dde26a84df7a86a25c682f9aaca54e8c80c6aa6864faa32f`.
Runner `--check-inputs` and `git diff --check` pass. These are not native compile or
UI-green evidence. Parent review findings on settings target/generation, early
Status, first-page routing, stable rows and accessible icon actions are addressed.

## Implementation source-check budget and Unicode regression gate

Run [38857507008](https://github.com/ashutoshpw/zmux/actions/runs/38857507008), exact
`34becf7f336af275d0c10f334b7d59576a643c1c`, did not compile or execute the native
feature: dual-slice Swift source checking hit the old 120 s fixture deadline with
zero diagnostic bytes (120.094 s, timed out, exit -9). No package or app phase ran.
All 273 retained source hashes match the freeze and retained bytes. This is a
compile-budget failure, not a behavioral result. The next fixture-only revision
raises the bounded source-check budget to 300 s and prints architecture progress.

Self-review and parent review identified Swift String equality's Unicode canonical
equivalence. `renderSelection` may retain the previous record's text bytes when a
new selected record has visually equivalent but differently encoded text. The
send/draft-clear receipt guards must also compare UTF-8 bytes. Parent approved a
fixture-first actual red before repairing those production comparisons.

The four authoring questions for this extension: (1) preserve exact bytes in the
actual selected body document/editor/saved reply; (2) selecting decomposed then
precomposed independent CLI records fails current `body.string != message.body`;
(3) CLI/domain tests cannot inspect the mounted AppKit document or editor; (4) real
CLI and UUID row actions need no private exports, fake draft owner or injected
receipt. The fixture seeds independent literals, verifies distinct CLI bytes,
selects both real rows, and labels only the exact selected-body byte failure at
`unicode_body` as Unicode behavioral red. Compile/setup/other journey failures do
not qualify. Production String equality remains unchanged in this fixture revision.
The continued journey also replaces an actual decomposed editor draft with an
independent precomposed literal using NSTextView.insertText and verifies saved bytes.

## Unicode fixture prerequisite diagnosis

Run [38858063145](https://github.com/ashutoshpw/zmux/actions/runs/38858063145), exact
`41be7acd4ec26d4be064a56977db712cc4ff42e5`, passed both Swift 6 slices (87.353 s)
and the arm64 package (259.030 s). Owned app PID 7158 exited 1 without timeout
(5.080 s), after all three prerequisites and actual inbox/body/reply responder.
A new Unicode CLI send failed the generic exact attribution/body/durability
assertion before the independent Unicode oracle; stage remains `entrypoint`,
`unicodeBytes` is empty, and `unicode_behavioral_red` is false. This is not the
intended product regression and authorizes no Unicode production repair.

All 273 retained sources match frozen SHA256 hashes and bytes. Local artifacts:
`/tmp/zmux-agent-inbox-unicode-red-38858063145/`. Archive SHA256:
`1247d2265e69f49f0ccc25294f4c4da9f4daabc3b077255e7583896238da5f90`.
The fixture now retains each bounded CLI send receipt with independently expected
body bytes, sender and recipient plus decoded observed bytes. This distinguishes
actual transport/state/attribution changes from a native fixture decoding issue
without weakening an assertion or changing production comparisons.

Run [38859020493](https://github.com/ashutoshpw/zmux/actions/runs/38859020493), exact
`effba260a5a8412a37f14ffc9b85fa4cb7494f4c`, passed Swift 6 slices (125.898 s)
and the package (337.365 s), then owned app PID 9452 failed without timeout
(5.263 s) at the same prerequisite. The retained precomposed seed expects UTF-8
`195,169` but the actual CLI saved receipt and decoded body contain `101,204,129`.
State is queued, durability saved and exact sender/recipient match. Decomposed
seed bytes match their independent literal. All 273 retained source hashes match
the freeze. Archive SHA256:
`77985737376cb203139a57a5cadedbf07ce61214a00a066cf1ce0d93b8cafce6`.
This remains setup evidence, not the intended selected-body behavioral red.

The fixture now uses supported `agent message TARGET -- -` and writes exact
`Data(body.utf8)` through a private CLI stdin pipe off the UI thread, closing it
before reading the bounded response. This avoids Darwin argv string bridging.
The exact seed assertions and selected-body oracle are unchanged. Each receipt
records `inputTransport: stdin_utf8`; production Unicode comparisons remain unfixed.

## Qualified native Unicode red and byte-preserving repair

Run [38860066126](https://github.com/ashutoshpw/zmux/actions/runs/38860066126), exact
`f0ac7a070ab05110ac4999823bca8f0e09ab7fe0`, establishes intended Unicode red.
Both Swift 6 source slices passed (101.035 s), arm64 package built (263.564 s),
and owned app PID 10176 exited 1 without timeout (5.986 s). All five preceding
checks passed. Stage is `unicode_body`; failure is exactly
`Native agent inbox: Selected canonical-equivalent message did not preserve exact UTF-8 body`.
Runner independently qualifies `unicode_behavioral_red: true`.

Both retained stdin CLI receipts preserve their independent exact literals:
decomposed `101,204,129` and precomposed `195,169`. Selecting the actual decomposed
row renders its exact bytes. Selecting the actual precomposed row commits selection
but the mounted body still has the preceding decomposed bytes. Three real PTY
children retain zero input; owned activation PID equals app PID; windows are 5/6.
All 273 retained source hashes match frozen manifest and retained bytes. Artifacts:
`/tmp/zmux-agent-inbox-stdin-red-38860066126/`. Archive actual SHA256:
`6271906a6c8f3e9cf74c0b0558306fcf036815316f9ceed81dd8b5652325f80a`.

The narrow production repair changes body replacement, draft capture, saved receipt
body validation and unchanged editor clearing to exact UTF-8 comparison. The
existing UUID, origin and revision guards remain. Queued rows with latest optional
`delivery_unknown` retain Submission unconfirmed even when retry is authorized;
authorization does not resolve previous exposure. Full same-command native green,
continued journey, mounted PNG review and shutdown remain required.

## Unicode repaired; uncertainty fixture assumption exposed

Run [38860897860](https://github.com/ashutoshpw/zmux/actions/runs/38860897860), exact
`0d2f2e3dc360296dab2aef770ce345227ed79125`, passes both Swift 6 slices
(106.486 s), package (325.234 s), and the first ten actual journey checks.
All four mounted/editor/saved UTF-8 proofs equal their independent expected bytes.
The actual button/Command-Return creates exactly one queued reply, preserving the
later draft edit and parent/thread/sender/recipient. Explicit read leaves later
arrival queued; receiving Off rejects send with draft retention and On never
resurrects failures. Owned app PID 10922 fails without process timeout (18.903 s)
at `Timed out awaiting real journal wait becomes explicit Check Status`.
All 273 retained hashes match the freeze; archive SHA256:
`a28929ae04750406061654883073ddd179c0ed135cb180c98559e0f99e41fcdb`.

The existing storage owner bounds lock acquisition at 2 s and reports precommit
lock refusal authoritatively; the UI's pending observer polls for 4 s. Therefore a
single held-lock job cannot reliably establish the expected uncertain state. Do
not change production deadlines solely for the fixture or claim an uncertainty
product regression. Parent requested preserving the authoritative refusal contract
and reporting public-owner alternatives before acceptance-scope changes. Failure
reports now retain actual mounted operation status, reply bytes and Send/Check
Status control state to avoid another generic timeout with no presentation proof.

Parent approved two separate native cases without production timing changes:
(1) exact known lock refusal, Send enabled/Check Status disabled, exact draft kept
and no extra reply; (2) independently queued public reads behind a second flock,
with actual pending admission/status receipts before asserting native uncertainty.
The helper queues exactly five List requests via the shipped public control wire,
checks the socket peer PID against the exact owned app, and retains all envelopes.
Five reads plus one native operation leave two of eight worker slots for bounded
GUI reads. Once the actual UI exposes its original UUID, the independent helper
requires that public Status reports it pending and at least two queued control
requests remain pending. Actual status capacity refusal, original draft/UUID,
lock release, one late Saved result and shared shutdown remain native gates.
Mounted known/capacity refusal states and exact draft bytes are also retained.

A real Linux C ABI/private-profile/control-server probe confirms this public
backlog route without fake receipts or sleeps as proof. It waits for two observed
preceding lock refusals, verifies the original pending via the independent helper,
then waits for four observed preceding refusals and verifies the original remains
pending. Releasing the actual flock yields its exact original Completed receipt.
Five unique control admissions, actual socket peer PID 245051, control states
`failed, failed, failed, failed, pending`, completed cleanup status 0. Probe script:
`/tmp/zmux-inbox-real-queue-probe.py`; raw receipts:
`/tmp/zmux-inbox-queue-oxd48pdk/`. This proves actual storage/public control setup
only; AppKit execution, capacity refusal, PNG and native shutdown remain open.

## Native background read clobbers pending operation status

Run [38862689598](https://github.com/ashutoshpw/zmux/actions/runs/38862689598), exact
`1d69fa2cc1cefc6f8dc43a2e61426828532e03c8`, passes both Swift 6 slices
(96.499 s), arm64 package (275.964 s), and the first ten actual checks again.
Owned app PID 9569 exits 1 without timeout (19.590 s) awaiting known refusal.
Mounted diagnostics now establish an actual presentation failure: Send disabled,
Check Status enabled and exact later draft retained, but status text is
`Refresh failed: another instance is reserving session generations; retry` and
the pending operation UUID is gone. The list refresh catch unconditionally wrote
to the mutation status channel. All 273 retained source hashes match the freeze;
archive SHA256 `9de3eb0767d8f06da74e1fac43fb1691a33f04d7e0f5ef9a0c2c31f32c4d9309`.

Parent authorized the narrow feedback ownership repair after these actual mounted
red receipts: auxiliary list-refresh and editor-validation feedback writes only
when no mutation is pending. Receiving queries already write only their separate
settings label. Receipt-validation uncertainty branches also retain the original
UUID with Check Status; origin-close invalidation remains authoritative. No timing
or storage behavior changes. The known-refusal case now observes actual pending
controls when present, verifies a recovery UUID and explicitly invokes the existing
Check Status action before asserting authoritative refusal and retained draft.
The independent public-owner accepted-pending journey remains a separate gate.

## Known refusal passes; uncertainty label assertion repaired

Run [38863733082](https://github.com/ashutoshpw/zmux/actions/runs/38863733082), exact
`4a56321636c8aa77842f104dde4d9921c0138abb`, passes both Swift 6 slices
(146.266 s), arm64 package (384.192 s), and eleven actual checks including exact
known refusal/draft retention. Mounted known status equals the storage refusal,
Send enabled and Check Status disabled; no extra reply is created. The second
independent helper admits five public queued List controls to exact peer app PID
16987, with all pending admission receipts retained. The native operation later
shows Check Status enabled, Send disabled, exact draft and original UUID, with
`Message acknowledgement failed (-4). Check Status: c5e7f94a-6611-4ee0-bc34-1ecd1a603502`.
This is a truthful status-read Busy refusal after accepted admission; an initial
refusal would discharge pending and cannot produce those controls. The fixture
incorrectly required literal Submission unconfirmed, so it never asked the helper
to verify the original pending operation. App exits 1 without timeout (35.549 s).
All 273 retained hashes match the freeze; archive SHA256:
`1dfcbd17d1caa420b0d23d2723b9c0d363d4b04b752bd359e2256fba2e564599`.

Fixture-only correction accepts actual uncertainty controls, exact draft and
parseable recovery UUID; independent public original/control pending receipts
remain mandatory. Bounded explicit Check Status actions may repeat after a Busy
read for known refusal, actual capacity -5 proof and final original Saved recovery.
Each action requires the same UUID/draft and Send disabled; exact -5 and late Saved
remain mandatory. No Send is replayed, no automatic native retry or production
timing changes are added. Explicit status action counts are retained and bounded.

## All native behavior passes; shared shutdown callback remains

Run [38866008965](https://github.com/ashutoshpw/zmux/actions/runs/38866008965), exact
`fc432b7d7a036d8d7776d95d4863dd0cd834771b`, passes both Swift 6 slices
(107.071 s), arm64 package (308.903 s), and all 15 actual behavioral checks.
The independent public socket owner proves original request
`92c93cc6-a822-43c2-8156-1cdec62b2a35` pending and four queued controls still pending.
Actual capacity -5 preserves that UUID and exact 66-byte draft, Send disabled and
Check Status enabled. One explicit capacity read and one recovery read yield
exactly one late Saved reply; the unchanged editor clears. Known status needs
zero explicit reads in this run and retains the authoritative refusal/draft.
Two saved reply IDs are retained, all original parent/thread/sender/recipient and
queued state checks pass, and fresh independent PTY progress proves zero input.
Owned app PID 9283, independent helper PID 9282 exits 0. All 273 retained hashes
match the freeze; archive SHA256:
`7c91c9ac2c99afffd9d789e89a2048b2346224c87bb9470a678737ea439b99b3`.

The app then exits 1 without timeout (24.204 s) during shared shutdown with
`The application desktop is closed; the command was not accepted`. Source shows
continuation.finish precedes closing the key inbox; synchronous main-window key
callbacks can enqueue after closure and become fatal shutdownNativeIssues. Parent
owns coordination of the shared repair; do not claim full native green yet.

The mounted content PNG is retained, but NSView cacheDisplay leaves the NSWindow
background transparent. The fixture now composites those actual native pixels
over that actual window's background color in its effective drawing appearance
and writes opaque RGB PNG, so text/icons remain readable in default viewers.
Runner verifies opaque PNG encoding. Shutdown-failure diagnostics retain zero
input without demanding heartbeat progress after the shared owner stopped PTYs;
the mandatory fresh pre-shutdown progress check remains unchanged.

Current main `76e8114` was inspected read-only and has the same missing key-event
shutdown admission guard (alongside newer pointer cancellation on resign). The
feature branch adds only `shutdownTask == nil` to the existing nativeReady/token
guard in `windowKeyChanged`, after presentation/gesture cleanup. This prevents
system key callbacks from enqueueing into the deliberately finished command
stream. It preserves user-command refusal reporting, startup/bridge ownership,
native cleanup tickets and current main's pointer path. Other owners were informed;
no other worktree or main edit occurred. The committed actual full-behavior/failed-
shutdown receipt precedes this narrow production repair.

Run [38867612703](https://github.com/ashutoshpw/zmux/actions/runs/38867612703), exact
`684bedaf87f5906fca00153202ece5427da476ff`, passes both Swift 6 slices
(72.613 s), package (207.973 s), and all 15 native behavior checks again.
Two explicit status reads establish capacity -5, one read recovers original Saved,
all exact text/draft/public-pending/PTY checks pass. App PID 7654 exits 1 without
timeout (18.939 s) only because AppKit cannot create a 24-bit/three-sample drawing
context for the opaque capture; shared shutdown is not reached. All 273 retained
hashes match frozen source. Archive SHA256:
`b72744b85fd84badc7641a3c2ebdac04f18c57ff6a55d06ebb5a1ee87baedb9e`.

Capture-only repair uses CoreGraphics' supported 32-bit RGBx CGContext
(`noneSkipLast`), draws the same actual background/native pixels, and exports
the resulting opaque CGImage through NSBitmapImageRep. No production change or
acceptance weakening: opaque PNG, shared shutdown and final native green remain.

## Background read must not erase a completed mutation receipt

Run [38868618382](https://github.com/ashutoshpw/zmux/actions/runs/38868618382), exact
`148d70fa9883926fd7b4419b1627191084629181`, passes Swift 6 slices (132.193 s)
and package (385.734 s), but exposes an earlier related feedback bug before capture.
The native reply has already been found in the durable CLI inbox with exact
body/parent/thread/sender/recipient and one reply; the newer 66-byte draft remains.
Mounted Send is enabled and Check Status disabled, but a background read replaces
Saved with `Refresh failed: Message acknowledgement failed (-4).` App PID 10676
exits 1 without timeout (17.199 s) awaiting saved receipt presentation. All 273
retained hashes match source; archive SHA256:
`4c70d8dfd534bab458b588354880501fb496702c6d46782935a88047d5187673`.

The preceding idleFeedback guard protected pending UUIDs but not completed Saved
or authoritative refusal messages. List reads now own an accessible refresh-status
label in the existing toolbar, with full error tooltip and truncation; successful
refresh clears only that label. Mutation feedback remains owned by mutations and
explicit idle editor validation. Settings already use their separate label. This
repairs shared feedback ownership across both pending and completed states without
retries/timing changes. Native saved reply ID/body proof is retained before waiting
for its UI status, improving failure diagnostics without weakening the assertion.
