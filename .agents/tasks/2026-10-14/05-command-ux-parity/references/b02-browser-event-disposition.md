# Native browser event dispositions

Scope: isolated `ci/browser-event-disposition` at `f2fd5b6`; no main edits or push.
Original MIT runtime, foreign ABI and Swift mapping only; preserve concurrent work.

## Evidence and correction

Native diagnostic `9518593` / run38844359603 captured19 generic failures:
17 exact-owner detach events after their model/native targets disappeared, and2
terminal fail events with matching observed owner/generation fields. Publish
prunes registered hosts before Swift closes removed native views and emits detach.
Fail/Complete reconcile authoritative deadlines before requiring pending dispatch.
Before this correction the ABI collapses these rejections into INTERNAL and Swift
recovers all hosts.

Introduce an explicit rejected-obsolete outcome, never ordinary applied success.
Keep exact epoch/namespace/current-host/operation owner checks. Retain at most256
exact retired-owner receipts when logical removal prunes a registered host; reset
clears them and eviction fails closed. Only known exact retired detaches qualify.
An already-terminal retained operation can reject a matching late receipt without
changing its original outcome or redispatching. Unknown, malformed, wrong-owner,
not-yet-dispatched and size failures remain errors. Rejected detach must return
before downstream download cancellation. Swift recognizes only the typed outcome;
all genuine errors keep existing guards/recovery and no extra retries are added.

## Authoring gate and verification

Primary tests exercise real BrowserRuntime/WorkspaceStore and foreign ABI owners.
They protect canonical close-to-retirement ordering, immutable terminal outcomes,
unknown/foreign-owner rejection and no cross-host download effects. Existing tests
cover generic rejection but miss typed classification and downstream handling.
No test-only seam is needed. Native diagnostic is the original behavioral red;
focused red/green commands will be recorded. Do not weaken result/owner guards.

- [x] Focused owner red evidence (existing native/ABI generic-rejection red also retained)
- [x] Implement typed rejection with bounded retirement evidence
- [x] Focused owner/ABI green; formatting/Clippy as scoped
- [ ] Source freeze and parent review before native run or publication

Native compile/runtime validation remains separate and pending. No cookie changes,
production data replay, cleanup weakening or complete native parity claim.


## Regression evidence

`CARGO_TARGET_DIR=target/agent-validation/browser-event-disposition/cargo cargo test
--locked -p zmux-desktop-runtime --test browser obsolete_ -- --nocapture` compiles,
then fails both new owner regressions for the intended outcomes: exact retired
detach reports `stale_browser_owner`; late terminal receipt reports
`browser_not_dispatched`. Log: `/tmp/zmux-browser-event-owner-red.log`.
The foreign-download extension compiles before repair and exercises real canonical
closure, exact retired owner, forged/unknown/malformed controls and unchanged live-B
download state. Native diagnostic already establishes the actual foreign status red.

Retirement receipts retain exact epoch/namespace/surface/host/document/navigation
captured at prune. They are not consumed: duplicate exact obsolete events remain
unapplied until bounded oldest eviction or reset. Eviction and reset fail closed;
there is no authority revival, new deadline or reattachment from a receipt.


## Implementation and green evidence

`BrowserError::ObsoleteEvent` is an unapplied rejection. Only that exact runtime
error maps to method-specific `ZMUX_BROWSER_EVENT_OBSOLETE = 1`; Swift returns
`rejectedObsolete` immediately. Other statuses retain existing failure/recovery
handling and BUSY retains its bounded ordered retry. The rejected payload and
downstream effects do not apply. Independent deadline reconciliation may still
advance an expired operation to its authoritative timeout outcome before rejection.

Focused browser owner suite: 18 passed (`/tmp/zmux-browser-event-owner-green.log`):

```sh
CARGO_TARGET_DIR=target/agent-validation/browser-event-disposition/cargo cargo test --locked -p zmux-desktop-runtime --test browser
```

Real ABI download owner test passed (`/tmp/zmux-browser-event-abi-green.log`):

```sh
CARGO_TARGET_DIR=target/agent-validation/browser-event-disposition/cargo cargo test --locked -p zmux-native-bridge --test downloads
```

Scoped all-target Clippy and `scripts/check-native-bridge` passed; logs are
`/tmp/zmux-browser-event-clippy.log` and
`/tmp/zmux-browser-event-foreign-c-green.log`. The final repository check passed
exit 0 after the first-late-Fail deadline assertion and wording refinement:

```sh
CARGO_TARGET_DIR=target/agent-validation/browser-event-disposition/cargo scripts/check
```

Log: `/tmp/zmux-browser-event-repository-check.log`. This includes workspace tests,
all-target strict Clippy, formatting, notice inventory, foreign C ABI and independent
profile-lease checks. The notice inventory reports three upstream text-review
requirements; that existing inventory state passes its check. A formatting-only
`apps/cli/tests/local_zellij_owner/native.rs` hunk from main `a7f06dd` is the explicit
baseline repair needed to run this check and is separate from the browser patch.

Owner coverage includes repeated exact retired detach, every owner-generation
forgery, bounded eviction/reset, pending-undispatched rejection, immutable successful
and timed-out outcomes, unknown/foreign operations, and oversized late payloads.
The ABI controls include malformed/unknown/wrong-live/forged-retired events while
preserving the unrelated live host's download. Source review confirms the rejection
returns before `State` applies download detach effects. Existing Swift call sites
both consume the typed disposition; fixed smoke-only traces contain no payload.

macOS Swift/C import compilation and a frozen native run remain pending parent
source review. No push or native rerun was performed, and this portable evidence
does not establish native cleanup, cookie durability or complete parity.
