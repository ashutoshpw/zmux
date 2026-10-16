# B01 production destructive owner

Base `b5557cb9205dc7db7cc9f6a0fd448e11edafddd5`; isolated branch/worktree
`ci/browser-profile-destructive-owner`. Parent owns main integration/publication.
This is original MIT design and code. No cmux implementation/assets are imported.

## Accepted physical evidence

V11 no-removal baseline and V12 clear/removal isolation passed independently retained
macOS evidence. V12 source538ada1/run38849912294 verifies mounted removal rejection,
exact inactive-store all-data clearing independently observed before namespace removal,
known removal success after caller release, then fresh emptyA/intactB and both public
cleanup callbacks. All six source copies and archived binary identities match.
Receipt `/tmp/zmux-b01-clear-removal-result-review-v12-20261016.json`, SHA256
`4f95e676a359900032912b6655dbd952449de815aa6793c2b95287afd7cb9a44`.
No more exploratory diagnostic runs are required. This does not implement production
leases, catalog mutation, recovery or shared UI/CLI commands.

## Existing owners and gaps

- `profile_leases.rs`: bounded dedicated worker and stable private anchored lifetime
  lock; shared/exclusive/release only. No constructor admission, constructed ack,
  writer transition or durable unknown marker.
- `BrowserProfileStores`: cached stores return synchronously; shared tokens last the
  process. Cached-view construction must join admission too, not only first store creation.
- `profiles.rs` plus `PersistenceWorker`: only add/rename; current receipt16 is volatile
  and old runtime epochs cannot query an earlier operation after restart.
- `ApplicationCoordinator`: all persistent browser views use the store owner. The
  download owner also retains actual WKWebViews/conversion owners and must count as busy.
- CLI and manager already route metadata actions through `ProfileCommand`; extend this
  owner rather than adding direct UI/CLI native mutation implementations.

## Contract and operation phases

Clear retains the immutable selected-catalog UUID/name; delete removes only that
selected catalog membership. Both use one physical sequence: exact inactive store
all-data clear, caller/view release, public UUID removal. Catalog change/public success
comes only after both original callbacks are known successful. Unknown/missing profiles
never fall back to default/private storage. Live/hidden views, pending constructors,
downloads and conversions reject before mutation; do not close user tabs implicitly.

One intent UUID correlates profile UUID, runtime epoch/receipt ID, native-controller
epoch, action, anchored selected-catalog identity and a monotonic phase. Phases:
`reserved -> preflight -> writer_ready -> clear_dispatched -> clear_completed ->
remove_dispatched -> remove_completed -> catalog_committing -> completed`.
Distinct failures report busy/unavailable, known partial clear, or `outcome_unknown`.
A late/mismatched token cannot mutate a successor. The service, not wall-clock age,
decides whether native work was dispatched and whether its original receipt resolved.

The bounded lease worker owns writer authority and durable phase records; all I/O,
lock attempts and syncing remain off AppKit and the core executor. Core profile
admission/polling uses bounded nonblocking queues. Swift alone performs public WebKit
operations on MainActor after exact worker authorization. New native request/event
ABIs are trusted-only, bounded, capacity-reserved before mutation, and not generic
control methods. Callback acceptance remains available during logical shutdown.

## Construction and writer protocol

Retain the existing lifetime-lock inode/name for compatibility. Add a stable private
per-UUID admission lock. Every actual construction, including cached-store reuse,
gets admission shared before lifetime ownership and current-catalog revalidation;
keep it through the actual native construction/registration acknowledgment. Lost
acknowledgment retains permit/capacity; status queries the original owner, no reacquire.

A destructive intent first fences local profile opens and obtains native busy proof.
Acquire admission exclusive while old lifetime shares remain held. Under admission,
revalidate catalog/native state, temporarily surrender only this exact owner's shares,
and try lifetime exclusive. Foreign shares cause busy. Restore every surrendered
local share before releasing admission; failed restoration retains writer authority
and reports unknown. Never assume flock conversion is atomic. Other profiles remain
usable and their cached stores/tokens remain intact.

After durable intent is established, authorize one clear on the exact inactive store
held through its callback. Persist clear completion before authorizing caller-store
retirement and one UUID removal. Keep writer authority through native receipt and
correlated catalog commit. Only known completion retires constructed tokens and releases
authority. Known namespace error after clear is explicit partial mutation; restore
required shared lifetime authority under admission before allowing ordinary use.
No automatic native retry is introduced.

## Durable unknown and bounded status/recovery

Use one bounded per-UUID versioned intent record in the common OS-Library lease root,
not a configurable catalog directory. Publish intent/token/profile/phase atomically
with existing private-file, file-sync, atomic-rename and directory-sync helpers. Keep
lock files stable; reserve admission/record names from session export. Records contain
no cookie/site data. Bound serialized bytes, requests, active writers, native dispatch
and completion queues; unresolved operations are never evicted for new work.

Every constructor checks this record under admission. A record at a dispatched phase
after process restart becomes explicit `outcome_unknown` quarantine for that UUID.
It cannot silently reopen, expire or replay. Status accepts original profile+intent UUID
without requiring current catalog membership and reports original action/phase,
known completed substeps, partial/unknown outcome, `recovery_required`, and whether any
safe next step exists. Other profiles do not inherit this quarantine. First version
may honestly report that native reconciliation after restart is not yet available;
it must name the original operation and why retry/open is unsafe, not an unexplained lock.

Known native success plus a catalog failure resumes only the original correlated
catalog commit, after revalidating selected-catalog identity; no repeated WebKit call.
Known pre-dispatch failure can cancel without pretending mutation occurred. Live
original callbacks can resolve only their exact intent/controller epoch. A new process
cannot synthesize missing native success from weak refs, UUID listing or file absence.

Compatibility choice requiring review: an older binary checks lifetime-lock size0 but
cannot read the new quarantine record. Before any native dispatch, while holding
lifetime exclusive, persist a fixed nonzero quarantine sentinel in that same stable
lifetime inode and sync it, in addition to the atomic intent record. Old constructors
then fail closed through their existing unsafe-lock check after a crash. New code permits
only the exact bounded sentinel under writer authority and a matching validated record;
arbitrary lock contents remain unsafe. Dispatch follows both durable records. Clear the
sentinel only after known terminal state is durable, under admission/lifetime authority.
Any torn/missing-record/sentinel mismatch quarantines; no lock unlink or inode replacement.
If this compatibility guard is deferred, the limitation must be explicit before enabling
production destructive commands; a journal-only promise does not cover older constructors.

## Minimal connected implementation slices and ownership

1. Rust lease/admission/journal foundation: `profile_leases.rs` plus focused
   `profile_leases/` helpers and existing secure persistence helpers. Add constructed
   acknowledgment, cached-reuse permits, exact writer transition/restore, durable intent
   and status. Extend real process/ABI lease tests and export-name reservation coverage.
2. Shared profile operation state: `profiles.rs`, a focused native-operation module,
   `lib.rs`, `control.rs`, `control_policy.rs`, and `persistence.rs`. Add typed clear/delete
   admission and read-only original-intent status; native unavailable is a pre-admission
   error. Revalidate profile UUID under the writer gate. Add exact catalog commit that
   rereads under the existing metadata lock, preserves unrelated changes and never runs
   before known native success. Integrate pending physical ownership into shutdown.
3. Trusted bridge and native owner: native bridge profile module/header/foreign tests;
   `NativeBridge.swift`, `BrowserProfileLeases.swift`, focused native profile-operation
   owner, coordinator construction reconciliation and download owner busy queries.
   Return a one-use construction permit, register actual views weakly, retain exact store
   through clear callback, release it before UUID removal, fence late events and retain
   unknown operations through shutdown. Follow V11's returning loading-helper structure.
4. Entry points and acceptance: `apps/cli/src/browser.rs`/help and manager window consume
   the same typed action/receipt/status. Add clear/delete UI confirmation that identifies
   selected profile/data loss. Read-only policy denies mutations and permits status.
   Update frozen coverage only after executable owner/native evidence. Production native
   smoke exercises the shared owner; standalone diagnostic is supporting physical evidence.

No unrelated Zellij/history/message files are owned. Do not revert parent WIP or copy
untracked diagnostic files from main. New build output stays under target/agent-validation.

## Regression first and meaningful verification

First compiling regression uses the real control socket, durable catalog and runtime:
valid clear/delete requests on a portable Desktop must report
`native_profile_owner_unavailable`, not invalid grammar or successful metadata work.
Catalog bytes, model and receipt history must remain unchanged; read-only denial still
precedes native admission. Reuse the existing profile fixture and its real shutdown/reap.
No test-only production seam or skipped assertion. The first red is classification of a
known unsupported native owner, not evidence of native data loss.

Next owner tests are required before their fixes:
- Deterministic two-process barrier during own-share surrender: neither foreign writer
  nor constructor crosses admission; foreign lifetime owner forces busy and original
  shared authority is restored before admission unlock. Unrelated UUID still succeeds.
- Worker acknowledgment loss/status/cancel retains exact permits and capacity. Forged,
  old-epoch, duplicate/out-of-order callbacks cannot clear authority or advance phases.
- Persist dispatch marker, kill process, restart with copied catalog: same UUID remains
  quarantined and original-intent status is readable; unrelated UUID constructs. Test an
  old zero-size-only constructor against the durable sentinel if that guard is selected.
- Actual persistence worker shows byte-identical catalog before native success, clear
  retaining ID/name, delete removing only selected membership, unrelated concurrent
  metadata preserved, and catalog retry never dispatching native work again.
- Real C ABI checks malformed input/capacity-before-mutation and native-only callback
  separation. Real macOS owner smoke proves mounted/hidden/download busy, one exact
  clear/remove, emptyA/intactB, manager/CLI agreement and real physical cleanup.

Run focused red/green commands, then repository scripts/check per cohesive slice.
Native execution, permission-dependent download gates and broader B01/V01 remain distinct.

## Initial limits and receipt retention

Proposed initial bounds:8 active destructive owners,32 construction permits,8 native
effect requests,32 reserved callback slots,16 snapshot receipts, and16KiB per durable
UUID intent record. Exhaustion rejects before dispatch and never evicts an unresolved
owner. These complement the existing32-profile catalog and64 lifetime-lease bounds;
verify aggregate descriptors/accounting rather than silently treating them as separate
unlimited pools. One latest record per UUID survives restart. It is never overwritten
while nonterminal/unknown or catalog commit unresolved. After known terminal completion,
a later explicit operation may replace it; an older intent status returns receipt-expired,
never a successor's outcome. No time-based eviction or automatic unknown recovery.

First actual focused command compiled, then failed the intended public-boundary
assertion: current response is `invalid_request` / unknown variant clear, expected
`native_profile_owner_unavailable`. This is a feature-boundary red, not native execution:

```sh
CARGO_TARGET_DIR=target/agent-validation/profile-destructive-owner/cargo cargo test --locked -p zmux --test browser destructive_profile_requests_require_native_owner_before_any_catalog_change -- --nocapture
```

Log: `/tmp/zmux-b01-production-native-owner-red-20261016.log`. Existing fixture startup
was extracted into a helper so the new real-owner regression reuses cleanup and startup
behavior. No production implementation, commit, push or native run is part of this
initial plan/test freeze; parent review precedes implementation.

Parent approved the connected implementation and exact durable sentinel behavior.
The pre-commit repository `scripts/check` compiled the regression and stopped at
its intended red:9 existing browser CLI tests passed, only the new native-owner
classification test failed. Log:
`/tmp/zmux-b01-production-regression-repository-red-20261016.log`. This intentional
regression commit precedes production changes; it is not a green implementation.

## Foundation implementation evidence

The same focused public-control command now passes. The first full check found a
Clippy needless-borrow in the extracted CLI fixture; it was repaired without bypassing
checks. Final `CARGO_TARGET_DIR=target/agent-validation/profile-destructive-owner/cargo
scripts/check` passed (log `/tmp/zmux-b01-foundation-repository-check-final-20261016.log`).
This executes workspace tests, warnings-as-errors Clippy, license inventory and the
real foreign C bridge/lease processes. It is portable evidence, not native WebKit execution.

The dedicated lease worker now owns initial/cached construction admission, exact
controller/intent writer tokens, local-share surrender/restoration, durable bounded
phase records, a synced stable-inode compatibility sentinel and correlated catalog-only
commit. Physical writer capacity8 and construction admission capacity32 supplement the
64 lifetime-token limit; steady retained lock descriptors are bounded by64 lifetime,
32 constructor admission and16 writer admission/lifetime descriptors, plus bounded
transient catalog/journal I/O. A temporary destructive store gets a logical lifetime
owner before native execution, so known failure can restore a share even without an
original cached store. Lost acknowledgments retain the original maps and gates.

Six production-owner lease tests pass with two intentionally ignored subprocess entry
helpers invoked by those tests. They cover independent-process crash quarantine,
foreign lifetime rejection plus restored local authority, pending/cached constructors,
forged/out-of-order phases, byte-identical catalog until known removal, clear/delete
semantics with concurrent rename preservation, stable inode/sentinel release, known
partial failure and original catalog-only retry after directory-identity failure.
The restart status identifies original intent/action/phase without catalog membership;
no native replay or expiry recovery exists. Reserved names include admission and intent
files. Generic catalog commands refuse destructive mutation. Constructed native tokens
cannot be released through the unconstructed-release ABI.

This slice does not yet register a native destructive owner, issue public WebKit effects,
wire clear/delete CLI/manager actions, or establish native physical/shutdown acceptance.
The public native-unavailable behavior remains deliberate until the connected owner is ready.

## Connected source before native acceptance

Foundation commit `4547f8c` follows the compiling red `cc1c54c`. Its generic failure
string authority was tightened during connected review: native outcomes are now typed
`Completed`, `AbortedBeforeClear` (Reserved only), or `RemovalFailed` (only the original
RemoveDispatched callback). ClearDispatched cannot become FailedBeforeClear. A generic
timeout/abort attempted after dispatch is rejected with the sentinel and original owner
retained. Idempotent phase acknowledgment does not enqueue another native effect.

The connected Rust owner, trusted controller ABI, Swift owner and CLI/manager paths
are implemented. Every actual construction has namespace/browser-epoch/surface/profile
identity and an initial or reuse permit through actual weak WKWebView registration.
Mismatched prepared permits are explicitly acknowledged/retired without supplying an
old store; unknown accepted permits remain charged. All download jobs, including
retiring/conversion ownership, count as busy. Native preflight never closes user tabs.
A single reserved native job retains the exact inactive store through the public clear
callback, returns from its loading/clearing helper, retires controlled caller references,
then dispatches one public UUID removal. Rust publishes success only after known native
removal, durable correlated catalog commit, physical writer cleanup and ordinary catalog
refresh. Native callbacks and physical accounting remain live through logical shutdown.
No callback timeout is converted into failure or success, and no native effect is replayed.

The manager confirms selected UUID/name and data loss. `browser profiles clear|delete`
and the manager share `ProfileCommand`; `browser profiles status PROFILE_UUID INTENT_UUID`
is read-only, including after catalog deletion or restart. Preflight status is explicit
before journal admission; durable unknown status names original kind/phase and exposes
unsupported recovery honestly. Client deadline messages retain the original intent.

Portable verification passed:
`CARGO_TARGET_DIR=target/agent-validation/profile-destructive-owner/cargo scripts/check`,
log `/tmp/zmux-b01-connected-repository-check-isolated-final-20261016.log`.
This includes6 lease-owner tests,1 shared runtime/socket/shutdown test and the expanded
foreign C ABI harness, plus all existing workspace tests, Clippy and license checks.
Two ignored subprocess entry helpers are exercised by lease tests. The shared runtime
fixture now runs in its own test executable with reused catalog fixture support: adding
its PTY fork to the lease-test executable exposed transient inherited catalog-guard
contention in unrelated parallel lease cases. No production lock was retried or weakened.
Earlier full-check failures (a Clippy unit expression and fixture isolation) are retained
in logs, not treated as passing evidence. Native Swift source has not been compiled on
this Linux host; public WebKit behavior is still an outstanding gate.

Existing native main evidence is separate. A single generic bridge BUSY occurred in
main22550c1/run38851088437 immediately before the mounted lease smoke. Source cannot
attribute it to that expected lease refusal; the subprocess-only lease smoke uses no
NativeBridge.failure path. main69ff15c/run38852633869 had no such generic BUSY, which does
not establish a fix. NativeBridge errors now carry function/command/request/buffer context
without changing the existing generic BUSY classification/retry policy. The original
BUSY remains open until an attributable observation or repair establishes its origin.
The broad native Save-panel Accessibility/post_event gate also remains separate.

## Proposed focused macOS acceptance (review before dispatch)

First run source typechecking for both native slices, then build the actual app/CLI from
the reviewed frozen source. Do not dispatch a standalone WebKit diagnostic again. Add a
focused packaged-app fixture before the broad Save-panel suite, with one isolated app
process at a time, generated private catalog and generated UUIDs, and a public HTTP
loopback origin. Keep the production fixed OS-Library lease root and production owner.
No cookie replay, backup, private API, flush claim or extra retention delay is introduced.

1. Create A/B/C using production metadata commands; open and seed actual A/B/C WebViews
   with distinct cookies/localStorage through returning helpers. Verify each UUID/store
   mapping and B isolation. Mounted and hidden A must refuse destructive admission without
   catalog/data changes. Exercise a real pending download/conversion busy refusal, cancel
   its original job and require physical retirement; do not approve a Save destination or
   weaken the external Save-panel driver/TCC gate.
2. Close A/C explicitly and observe actual weak view release before mutation. Use a real
   independent lifetime-share holder to prove foreign busy, unchanged A and usable B.
   Reap that exact helper. Clear A once via CLI and C once through actual manager controls
   plus the data-loss confirmation. Require original-intent completed receipts/status,
   stable selected UUID/name, new real A/C views empty and B cookies/localStorage intact.
3. Reseed only generated fixture data where needed. Exercise confirmation cancellation
   through the real manager and verify unchanged catalog/data. Delete C once through CLI
   and A through manager confirmation. Require selected membership removal only, original
   durable status after membership is gone, and intact B. No retry of admitted mutation.
4. Persist a normal B layout, terminate the owned app normally, and verify the runner's
   original PID exit/reap. Start one fresh owned reader process without reseeding to check
   deleted membership, clear-result storage where applicable, and B data/UUID restoration.
   Clean only generated fixture UUIDs with the production clear/delete owner and known
   callbacks; record normal shutdown and exact owned process exit/reap again.
5. Keep portable forged/late/out-of-order/native-timeout rejection and dispatched-crash
   quarantine receipts alongside the native run. Missing native callbacks are never
   synthesized. A deterministic native missing-callback/constructor-during-restore race
   harness is a remaining gate if not directly exercised by this focused fixture; do not
   infer that acceptance from successful ordinary lifecycle restore or source inspection.

Freeze and review the fixture/runner before the hosted execution. Retain bounded logs,
source/build identities, original UUID/intent receipts, callback phases, actual weak-view
release checks and owned process cleanup. No broad B01/V01 parity or all-data-type seeding
claim follows from this limited cookie/localStorage fixture.

## Explicit reconciliation source

The production gap found in review is now connected, rather than relying on the lease
unit test's catalog retry. The CLI exposes `browser profiles reconcile PROFILE_UUID
INTENT_UUID`; the manager exposes “Finish Interrupted Operation” with exact original UUID/
operation details. Both use the shared controller's separately bounded8-receipt queue.
A pending explicit re-request re-delivers only the original metadata reconciliation;
MainActor coalesces its running receipt. Receipt UUIDs fence late completions from a
successor. Read-only sockets can query reconciliation but cannot request it.

Swift retains its original job/token, clear callback and removal result. Its reconciliation
function never constructs a store/view, calls all-data clear or calls UUID removal.
It only re-acknowledges that captured removal callback, commits the original catalog change,
or recovers a known terminal physical cleanup. It retires the job only after matching
durable terminal state and physical-cleanup proof. Lost finish acknowledgment after the
runtime queue entry retired uses durable service status; it does not require `request()`.
RemoveDispatched re-acknowledgment still requires that original queue entry, which cannot
retire before terminal physical proof. A broken invariant fails closed without native replay.
Unknown clear/removal callbacks or restarted unresolved operations remain unsupported and
quarantined. Shutdown continues draining metadata-only reconciliation independently.

Known FailedPartial/FailedBeforeClear phases remain failures even when optional diagnostic
text is absent; they never produce a completed destructive receipt or success copy. The
manager distinguishes known partial/before-clear failure from inability to confirm an
outcome. Terminal-sentinel status now reads and validates the stable inode and reports
pending physical cleanup honestly. Filesystem-only recovery requires the exact original
known terminal record, gate/lock identity and no active successor. Ordinary effect polling
is driven by pending snapshot counts; it adds no idle native IPC loop.

Review of caller lifetimes also extracted returning job reservation, clear and caller-store
retirement helpers. The outer effect path retains only the mutable job, so cache-entry or
store locals cannot accidentally span UUID removal, matching the accepted physical probe's
helper lifetime discipline.

The additional real-CLI boundary regression was committed first as `24aa46a`. Against a
trusted exported `5109f75` tree plus that test it compiled and failed the intended missing
`reconcile` grammar assertion; log `/tmp/zmux-b01-reconcile-ingress-red-20261016.log`.
The same focused command passes current implementation; log
`/tmp/zmux-b01-reconcile-ingress-green-20261016.log`. An initial missing test import was a
compile failure, repaired before the actual compiling red; it is not regression evidence.
The real foreign C ABI harness now exercises catalog failure followed by catalog-only
reconciliation, plus lost finish acknowledgment after original Desktop/operation retirement.

The process-global fork fixture isolation was tightened after further transient inherited
catalog-lock contention: test cases using subprocess/PTY fixtures serialize host setup,
while preserving their explicit independent-process lifetime/admission concurrency checks.
No production lock retry or weakened assertion was introduced. Three focused runtime/
recovery tests pass, including original failures without optional text, stale reconciliation
receipt rejection and exact terminal-sentinel cleanup. The owning boundaries remain distinct:
public CLI admission, physical worker/process exclusion, shared socket/shutdown and foreign
C ABI transport. There are no test-only production flags/exports or fabricated WebKit proof.
Native Swift compilation and the focused physical happy-path fixture are still pending.

Final recovery portable verification passed on frozen Rust/C source:
`CARGO_TARGET_DIR=target/agent-validation/profile-destructive-owner/cargo scripts/check`,
log `/tmp/zmux-b01-reconciliation-frozen-source-check-20261016.log`, including all3 runtime/
recovery cases,6 lease cases, actual compiling public ingress regression, all workspace
checks and the foreign C reconciliation harness. Swift helper/UI source remains a separate
uncompiled macOS gate; these portable results do not establish physical native acceptance.

## Focused production native acceptance source

The accepted recovery freeze remains unchanged at64b4a9e, manifest
`/tmp/zmux-b01-production-recovery-freeze-20261016T053434Z/manifest.json`, SHA256
`556d8dc08333727b2ea7ce827b38a2fc4aa09223f4adf0a3a09119ca89cc9ffa`.
The next slice adds `BrowserProfileAcceptance.swift`, the isolated
`check-native-profile-operations` runner and the existing dispatch workflow's
`profile-operations` suite. It must pass both Swift source slices before a universal
package or real native run. Parent reviews the frozen source before dispatch.

The strongest new boundary is the packaged app plus bundled public CLI and actual
AppKit manager controls/confirmation sheets. Its independent oracles are cookie and
localStorage observations in new real WebKit views, private catalog UUID/name membership,
durable original-intent status, persisted restored surfaces and independent-process
flock exclusion. No completion, store, native effect or callback is fabricated. Existing
portable workers/C ABI coverage cannot observe WebKit caller lifetimes, physical removal,
real manager routing or data visible after an original app PID exits. The launch flag
only selects this generated private-catalog journey; it changes no production owner.

One runner owns three successive original packaged-executable child PIDs. Prepare
generates A/B/C, refuses mounted A/hidden C and an actual download conversion, cancels
the actual Save sheet, closes A, observes a live independent shared inode owner block
clear, then clears A via CLI/C through manager confirmation. Cancellation must preserve
C's data/catalog. New A/C data written after clear must survive explicit CLI/manager
original-intent reconciliation, detecting native effect replay. Further production
clears leave empty A/C and untouched B for persistence/restore.

Only after successful shared shutdown and actual original-child normal exit/wait/reap,
plus exclusive zero-sentinel proof for each generated lifetime inode, does a fresh reader
restore the saved A/B/C surfaces without reseeding. It verifies A/C empty, B intact and
terminal status/reconciliation after restart; deletes C by CLI/A through real manager
confirmation; queries/reconciles original intents without membership; and shuts down.
A third fresh process verifies A/C membership and model surfaces absent, deleted original
status/reconciliation available and B intact. Only then does public deletion clean B.
Failure/timeout retains the generated profile/logs, never launches another diagnostic app
and never deletes unknown website storage or signals the user's running application.

Download coverage deliberately means actual WKDownload conversion, refusal while the
live tab/model guard also exists, actual Save Cancel and physical cleanup. It does not
isolate a download/conversion-only refusal or claim Save acceptance/TCC authorization.
Missing callbacks, interrupted nonterminal native recovery and deterministic constructor
racing restore remain separate unexercised gates. Ordinary terminal reconciliation is
exercised, not a lost-callback claim. Native compilation/execution is still pending at
this source-freeze stage.

Parent fixture review accepted the scope and requested one launch-isolation repair:
the runner drops every inherited `ZMUX_`/`DYLD_` value (and Rust compiler flags),
then supplies only its exact private `ZMUX_SOCKET` and `ZMUX_SOCKET_PATH` to the
owned helper/app processes. Inherited engine, socket or credential overrides cannot
redirect this generated fixture to another service. The repaired freeze is authorized
for branch push and the isolated `profile-operations` dispatch, with compilation first.

The final pre-dispatch owner audit found that Swift's shared profile mutation helper
required a name even for UUID-only clear/delete commands. The real manager fixture was
already committed in5ff0028 before this repair, but could not execute its native red on
Linux. Name validation is now scoped to add/rename; clear/delete reach the same Rust
admission used by the manager. No native effect/ABI changed. The hosted fixture must still
establish actual compilation and manager behavior; source reasoning is not native red proof.

First focused hosted attempt38857634808 on425fd59 passed both Swift source slices,
strict concurrency/warnings-as-errors and both C helpers. Its11 browser CLI cases passed,
including destructive admission/reconciliation. It stopped before package or native launch
because this branch's older base lacked main22550c1's Darwin socket fixture repair:
all5 existing execution-admission cases read inherited nonblocking accepted streams and
failed WouldBlock35. Raw logs are retained at
`/tmp/zmux-b01-native-38857634808-complete.log` and
`/tmp/zmux-b01-native-38857634808-failed.log`; artifact12028415276. All29 uploaded
changed compilation-source files matched the frozen hashes/lengths exactly.

Only the existing3-line `apps/cli/tests/execution_admission.rs` repair from
22550c1db7a26df12b462405c072538652860a44 is applied here: set the accepted fixture
stream blocking before its existing bounded3-second frame timeouts. Its concurrent R03
and status documentation is not imported. No production retry or assertion was changed.
The focused5-case command passes locally; macOS verification remains the next run's gate.
