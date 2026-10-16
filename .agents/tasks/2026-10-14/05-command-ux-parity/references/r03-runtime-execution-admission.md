# R03 execution admission runtime evidence

The isolated `ci/execution-admission` worktree implements the accepted V2 design
with original MIT Zmux code. No cmux implementation, tests, or assets were used.
The reviewed imported Darwin/bootstrap repairs are separate commits25a29c3,
07b81fa, and b798704.

## Contract

New Start uses admitted-lifetime V2 unless an explicit observer1/2 negotiation
selects legacy V1 before creation. A constructed V2 owner is never downgraded.
Observer3 and completion control2 carry the explicit V2 execution contract;
observer4 is reserved for the separate input-readiness design.

The actual direct Child starts as the original CLI helper. Its full birth image,
UID, parent, start, and boot identity are independently measured while it is
blocked on a private one-use admission channel. The starter persists the exact
binding before the supervisor releases Run. The complete bounded Run wrapper is
encoded before any write or released-state transition. After a write may have
delivered Run, the command is never replayed or terminated to resolve uncertainty.
The helper execs the exact authorized shell invocation in the same retained Child.

Live V2 validation pins PID/parent/start/boot and retains exact full-image checks
for server and supervisor. It permits legitimate arbitrary exec and does not
claim continuous workload-image or UID integrity. Legacy V1 remains full-image.
The supervisor publishes the actual retained Child wait, with an independently
correlated configured-shell exec error when exec returns. NotObserved does not
assert either successful shell exec or failed execution. ShellObserved is only a
diagnostic requiring the pinned lifetime; it is not sampled after Child reaping.

Malformed, duplicate, or truncated private error evidence never produces a
terminal receipt. The supervisor retains its actual Child through wait, then exits
with a bounded uncertainty error. Immutable registry/runtime evidence remains for
operator inspection; authenticated cleanup refuses missing terminal authority.
There is no automatic success, replay, adopted-process kill, or speculative cleanup.

## Source reviews and validation

The exact runtime cohort and hashes are frozen in
`/tmp/zmux-r03-runtime-v2-freeze-20261016/manifest-r2.json`.
Its SHA256 is
`e3820446ab80ad727c1ec65e30376a49d3808c14ef323f2953fdd511d8cc58fe`.
The first independent review found the full Run-wrapper bound issue, repaired in
revision2 before any production commit. Its no-bytes/unreleased guard passes.

Evidence logs are under
`target/agent-validation/r03-execution-admission/` in the main checkout:

- `prebound-red-sh.log`: public immediate0 returned1 after its once-only side
  effect. Registry already bound; this proves a completion/attach race, not a
  prebinding race. The separate real helper no-Run control proves admission order.
- `admission-mechanism-r2.log`:5 actual supervisor/helper controls pass: held helper
  cannot run user code; valid binding runs once and retains7; EOF and changed
  generation before Run cannot execute; removed configured shell yields explicit
  errno2 with raw Child1; post-binding bootstrap disconnect preserves actual7.
- `admission-channel-guards-r2.log`:4 channel-owner guards pass for full lifetime
  projection, duplicate complete error frame, truncated terminal drain, and full
  Run-wrapper size before any write.
- `public-native-v2-suite.log`:17 actual host conformance tests pass. They cover
  immediate0/7, selected-shell startup7 without command body, arbitrary exec plus
  detach/Attach with one input and actual7, concurrent consumption, native V2 live
  contract propagation, actual Saved finite0/7/signal15, persistence failures,
  namespace refusals, and preserved legacy observer1 behavior.
- `legacy-observer-two.log`: explicit observer2 actual finite completion passes
  with original shell-image validation and raw V1 wire/receipt.

The public immediate regression is resolved by bounded authenticated publisher
queries when Subscribe precedes an already-ended Child. Missing process state
never manufactures a terminal outcome. Commands are never retried.

Native input readiness, AppKit acceptance, Darwin process execution, and hosted
verification remain separate gates. Transport connection and fixture render
markers do not establish product input readiness. Publication remains blocked
until parent review.

Revision2 combined `scripts/check` passed format, workspace tests, strict workspace
all-target Clippy, license inventory, and actual native C ABI checks. Validation
receipt `/tmp/zmux-r03-runtime-v2-freeze-20261016/validation-r2.json` SHA256
`22babd5d09d08992297458a0cbd88d414d89c3b4d672d09df207ca3dbee9d826`
rechecks all21frozen runtime hashes.

Independent runtime revision2 source review accepted with no remaining findings:
`/tmp/zmux-r03-runtime-v2-independent-review-20261016.json` SHA256
`fdbbd93c0b2011e5f884a838c17dc2edca86fc4f08acee0904679e45a5f39db7`.
The test authoring audit retains actual process/transport/storage tests at their
production owners. Synthetic private-channel guards cover distinct malformed
frame, lifetime-drift, and prewrite-bound contracts unreachable through a correct
helper. No production-only seam was added solely for tests. No tests were removed
or weakened to obtain green results. The earlier native observer3 red was fixed
at negotiated completion-ACK version validation, retaining observer2/rawV1 proof.


## Darwin admission fixture socket mode

Main Foundation run `38850609492` at `b5557cb` exposed five immediate failures
at the first supervisor-frame read in `apps/cli/tests/execution_admission.rs`.
Darwin inherited the nonblocking listener flag on the accepted connection, so
its read returned `WouldBlock` before the configured timeout could apply.
The fixture now explicitly selects blocking mode before its existing three-second
read/write deadlines. Production supervisor and completion-control owners already
select this mode. Admission ordering, exact binding, actual child results, and
all five behavioral assertions remain unchanged; no new production seam is added.
Raw hosted failure is retained at `/tmp/zmux-main-foundation-38850609492.log`.
Local full verification is recorded at
`/tmp/zmux-admission-accepted-socket-check.log`; Darwin acceptance requires the
subsequent Foundation run and is not inferred from Linux.

Darwin Foundation verification for the socket-mode repair passed at `22550c1`
in run `38851088457`. The separate Linux DOM step failed before executing DOM
assertions because its browser version probe timed out with no output; that job
passed on its unchanged rerun (attempt 2). This does not alter the accepted Darwin
admission result.
