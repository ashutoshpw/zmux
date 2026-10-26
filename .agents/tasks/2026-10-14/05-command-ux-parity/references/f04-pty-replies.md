# F04 ordered kernel PTY replies

2026-10-15. Approved next isolated slice after metadata acceptance. No production
engine switch, native app change, new parser or modification of the accepted
terminal/metadata implementation inputs. A separately documented subprocess-runner
lifecycle repair changes only the shared verification helper and its tests.
New standalone tooling lives in
`tools/terminal-pty-feasibility/`; its lock records the accepted owner input hashes.

## Owner and observable contract

One original transport owns an actual kernel PTY master, one exec'd Python child,
and two fixed control/result pipes. The child uses the raw, no-echo slave as its
stdin/stdout. It remains in the fixture subprocess group so outer deadline cleanup
also reaches it; no controlling-terminal, shell/job-control or production session
claim is made. No shell command or source text is injected into a terminal.

The accepted Resource remains the only terminal/Probe/allocator owner. The Rust
coordinator reuses the accepted metadata normalization and staged export/validation;
it reserves bounded reply capacity before reading at most 512 PTY output bytes.
It never reads bytes it cannot admit or parses terminal escapes outside the engine.
Fixed readiness/results are a separate fixture protocol, not another VT parser.

Validated export seals the current generation. Replies are copied into fixed input
storage and advanced using the actual nonblocking write result. The coordinator
defers exact C commit and frame/metadata slot swap until all replies for that
generation have been accepted by the kernel. While a prefix is pending, further
PTY reads, engine operations and user/resize admission pause or return explicit
Busy. A 250-ms experiment deadline bounds each sealed reply lifetime; deadline or
explicit cancellation quarantines the owner and stops/reaps only its child.

Kernel acceptance does not mean the child consumed the bytes. Normal cases require
the actual child to independently compare `PRE + literal replies + POST sentinel`
in order; the sentinel detects missing/duplicated/reordered replies. A failure after
a partial write cannot retract that external prefix. The candidate frame/metadata
remain unpublished, the last committed generation remains readable and no retry,
replay, fallback, hidden reset or consumed-byte receipt is claimed.

Resize is admitted only while idle: stage/export/validate the engine resize, perform
kernel `TIOCSWINSZ`, then nonallocating exact commit/swap. An ioctl failure quarantines
the candidate; it does not claim rollback of in-place engine/kernel changes. A
resize attempt during a blocked reply must leave both owners unchanged and be
retried explicitly after progress, rather than being silently coalesced or lost.

## Acceptance shape

- Small child journey: actual PRE bytes precede generated cursor/status replies;
  POST follows them; child checks literal response order, raw PTY identity and
  resized kernel geometry. Metadata and frame are the same committed generation.
- Paused-reader flood: actual kernel EAGAIN, retained bytes/offset, resumed exact
  child stream and explicit Busy/retry resize. Count real short writes separately
  from a deterministic small syscall budget that exercises partial progress.
- Stalled-reader deadline and explicit cancel: finite reply lifetime, immutable
  last-good publication, no resumed/replayed input, physical child/FD cleanup.
- Child consumes a confirmed short prefix then closes: detect the real peer loss,
  discard the unpublished candidate and preserve irreversible-prefix accounting.
- Real resize error and engine/normalization failure: no candidate metadata/frame
  commit, no generated reply sent from a failed operation, quarantine and cleanup.
- Temporary original-fixture controls duplicate/reorder actual reply bytes or
  publish while blocked; they must fail their named independent assertions.

The runner reuses verified archive/patch/build/containment helpers and records
every base/new source, command, result and mutation hash. Linux and macOS execute
their own host architecture; no universal/native/UI or total-RSS claim follows.
Bounded transport storage, engine mappings, host publication slots and child/kernel
resources are reported separately. Independent review and hosted execution precede
any production adapter integration or measured native workload decision.

## Local acceptance and cleanup repair

The first seven-input run and independent parent run both passed before review
found a partial-construction cleanup defect: a control-pipe failure could enter
cleanup while the report pipe was still blocking and its writer remained open.
Saving a report then waited indefinitely. Those successful PTY receipts are
predecessor evidence, not acceptance of the repair.

The original `construction.c` regression uses the public transport constructor in
a separate bounded subprocess. It sets `RLIMIT_NOFILE=15` and occupies descriptors
3..9; the PTY relocation and report pipe leave only one free slot, so creation of
the control pipe fails with real `EMFILE` before child creation. The predecessor
timed out after 2.080 seconds and was killed by outer process-group containment.
Guarding report preservation on completed nonblocking initialization makes the
same regression return promptly and checks descriptors 10..14 are all physically
closed while caller-owned 3..9 remain intact. Red/green logs and phase receipts:
`/tmp/zmux-pty-construction-20261015/`; repaired execution took 0.084 seconds.
Cleanup wait/syscall failures remain explicit errors; exercised successful cleanup
does not establish reaping under every possible operating-system failure.

The transport-cleanup eight-input freeze, before the later runner-only repair, is
`/tmp/zmux-f04-pty-cleanup-frozen-manifest-20261015.json`, SHA-256
`a574932787d376983764724e101b5d7a4f04e899c19730a579d7a821462e6fc4`.
It adds `construction.c` and freezes the repaired transport, runner and contract.
All 29 accepted base hashes remain unchanged.

The fresh-source Linux x86_64 transport-cleanup receipt, also before the later
runner-only repair, is
`/tmp/zmux-f04-pty-cleanup-final-20261015/receipt.json`, SHA-256
`393951390bae42b50edded868adbc1137f6f3c484ffbc9d0109be310594c38ed`.
It matches all eight final hashes, verifies fresh source/archive/license/patch
inputs, explicitly reuses only the labeled dependency cache, rebuilds the engine
and C shims and runs the construction regression and all seven PTY modes. Engine
library SHA-256:
`3c9a2240ef05e50e3af24b951819ffff1d478b8f2a457af35581e30e427ce504`.
Construction passed in 0.061 seconds; the normal PTY journey passed in 1.319
seconds. Flood/stall observed 1/235 actual EAGAIN returns, zero kernel short-write
returns and a 768-byte observed queue high-water. The syscall-capped three-byte
prefix is separately confirmed consumed by the real child before cancel/close.
The stalled queue hit its 250-ms deadline; TERM-resistant cancellation required
SIGKILL and checked reaped status 137. All exercised teardown paths checked zero
live mappings, no live child and physical closure of all owned descriptors.

Both temporary original-coordinator controls exited 101 without timeout at their
intended assertions: `child exact input order differs` for duplicate reply data,
and `blocked/failing replies must not publish candidate` for early publication.
No base or upstream source was mutated by these controls. A focused author
rerun using preserved accepted engine objects/library also passed:
`/tmp/zmux-f04-pty-author-cleanup-focused-20261015/receipt.json`.

## Shared verification-helper repair

Subsequent independent review found a separate process ownership defect in the
shared verification runner: `Popen.poll()` could reap the leader before final
`killpg`, releasing the PID/group identity before its last signal. Earlier
successful kernel PTY results remain predecessor observations; they do not prove
the repaired containment lifecycle.

The runner-only repair shares original `scripts/process_group.py` between the
terminal and browser callers. It uses public `waitid(...WNOWAIT)` to observe exit
while retaining the unreaped leader through all group signals, then reaps it.
Capture-setup failure also stops/reaps the already-created actual child and closes
its descriptors. Darwin EPERM is accepted only with the retained exited leader
and a bounded public process snapshot proving every remaining group member is a
zombie; a live/unknown group remains an error. This is verification tooling,
not a terminal-engine change.

The actual ownership-boundary regression failed before the repair because the
leader was no longer a waitable child at the signal boundary; the setup-failure
case likewise failed for both callers with an unreaped actual child. Both focused
checks and all 11 lifecycle tests passed after repair. Evidence is
`/tmp/zmux-shared-process-group-20261015/`; its four-source manifest SHA-256 is
`8547c2dffe89ca4cd4002770b0c0bd293f100a7fd40e1f8e649a7f1f260967fe`.

The final PTY lock now verifies 31 inputs: 28 original terminal/metadata inputs
unchanged, the revised runner, its regression tests and the shared helper.
`runner_revision` records the original runner hash and exact revised hashes.
The eight-input PTY freeze is
`/tmp/zmux-f04-pty-shared-helper-frozen-manifest-20261015.json`, SHA-256
`c42a57fa7642fc68d4ee3285483175d481db2569bab16cab3b65b5d165d205d6`.
Intermediate lifecycle/doc freezes were superseded; an interrupted build and a
source-drift failure are not final acceptance evidence.

The final fresh-source Linux x86_64 acceptance is
`/tmp/zmux-f04-pty-shared-helper-final-20261015/receipt.json`, SHA-256
`6956cbfc0d1b4c44a0387b277dbd5899964b68fd6988e99f9324f7a4b74b6520`.
It matches all eight PTY input hashes and all 31 locked input hashes before/after
execution. Source/license/archive checks, unchanged resource/metadata patch
application, a fresh engine/C rebuild, actual construction exhaustion and all
seven PTY modes pass with the repaired shared helper. Only the explicit labeled
dependency cache is reused. Fresh engine library SHA-256:
`70b20e3c7b426307f288f1eb025a7b1adabe90ea3506c2c348f508cf3b69d93f`.
The construction case passes in 0.063 seconds, normal PTY in 1.310 seconds and both
controls fail their intended named assertions with exit 101 and no timeout.
Flood/stall observe 1/234 actual EAGAIN returns, zero kernel short-return writes
and 768 pending bytes at high-water; fixed capacities remain as listed below.
This is local native-host Linux evidence, separate from hosted Darwin execution.

## Hosted result and direct-child cleanup follow-up

Workflow [38784165438](https://github.com/ashutoshpw/zmux/actions/runs/38784165438)
at `16cf61aef74e1d95dda266c6c737ff8716a02eba` passes Linux x86_64. Its receipt
SHA-256 is `1ac9bae214f3f300bfdd19adf312acedeef329d09e0ede242b12e60c8b0f150d`.
Darwin arm64 on macOS 15.7.9 passes the small ordering/resize journey and flood
journey, observing one actual EAGAIN and one actual kernel short-write return,
then fails with the generic `pty_stop == -1` assertion. The Darwin receipt SHA-256
is `a22186ebc23415d6330e8964b9cc521ec0c14a19e767b0a4c4a1b56c17888df9`.
Downloaded artifacts live under `/tmp/zmux-pty-hosted-38784165438/`. No cleanup
stage/errno was recorded by that source, so its exact Darwin cause is unresolved;
the earlier successful flood does not establish complete Darwin acceptance.

Source review separately proves cleanup attempted SIGTERM before checking whether
its own child had already exited. The original `cleanup.c` syscall-boundary
regression creates a real PTY/Python child, uses public `waitid(...WNOWAIT)` to
hold its actual exited/unreaped state and observes/forwards the real signal call.
Before repair it exits 6 at `cleanup attempted a signal to an exited/released owned
child`; after repair it reaps without any signal and checks physical FD closure.
This Linux regression proves unnecessary signalling, not the unrecorded Darwin
errno. Red/green evidence is `/tmp/zmux-pty-cleanup-signal-20261015/`.

The repair first checks/reaps its direct child with `waitpid(WNOHANG)`. A wait
error never grants signalling authority; signal EPERM/ESRCH is accepted only if
that exact child is subsequently reaped. First cleanup failure retains operation
and errno, printed with full transport stats by the Rust failure assertion.
A second case intentionally reaps only the fixture's child through the kernel,
then verifies real ECHILD is reported at the wait operation without signalling a
released PID. It does not pretend that this error path successfully owns/reaps the
child. TERM/50-ms and KILL/200-ms bounds and existing construction cleanup remain.

The new nine-input freeze is
`/tmp/zmux-f04-pty-direct-child-cleanup-frozen-manifest-20261015.json`, SHA-256
`f061c612cea234e3cf9ae7994a5c364671139727d92ab5ea70eae8d26a64ce4f`.
All 31 base inputs remain fixed. A focused Linux rebuild of the new transport,
construction/cleanup fixtures and Rust normal/controls passes against the hashed
preserved accepted engine library/C objects, without another engine rebuild:
`/tmp/zmux-f04-pty-direct-child-cleanup-author-20261015/receipt.json`, SHA-256
`f00f65fee4c5f92003b9ee8a73e28fffae03887a3756864c67f0b9f6dbd1bfea`.
All seven PTY modes, construction exhaustion and both new cleanup cases pass;
both controls still fail their named assertions with exit 101 and no timeout.
Observed flood/stall EAGAIN counts are 1/230 and kernel short writes zero in this
Linux run. Hosted Darwin execution of this exact follow-up remains required.
Parent independent source review finds no blocking issue and its separate focused
run matches the same nine hashes, passing construction/cleanup/all PTY modes and
both intended controls. Independent receipt:
`/tmp/zmux-f04-pty-direct-child-cleanup-parent-20261015/receipt.json`, SHA-256
`150d735dc04752c10d16068a63183d0d3879da6d539cd465a672d7163d9bce98`.

## Darwin reaping deadline and master-close repair

Workflow [38786746778](https://github.com/ashutoshpw/zmux/actions/runs/38786746778)
at `c01c3bc2774d7be7d49fb813aa0739ff3a3f188e` passes Linux again (receipt SHA-256
`0733134d1e6a195c0e54bff328784416c07d9206fe6f873948be1effc560cc4e`).
Darwin passes construction exhaustion, both direct-child cleanup cases, basic
ordering/resize and flood; flood observes one EAGAIN and one kernel short write.
The stalled-reader cleanup then fails at `child cleanup deadline`, errno 60
(ETIMEDOUT), with one live/unreaped child, zero owned descriptor slots and no
preceding wait/signal error. Darwin receipt SHA-256:
`c7946c6b3b42236f4cef841162f0bbd996066827214fcb5b9887ae2b5cba0438`.
Artifacts are under `/tmp/zmux-pty-hosted-38786746778-{macos,linux}/`.

The observed failure is not an EPERM case. The transport waited for the child
before closing its PTY master; on Darwin an output-blocked child can remain in
exit until that master closes. The original production session worker already
documents and handles this lifecycle ordering. The fixture now signals its live
owned child, closes its own master, then performs the same 50/200-ms bounded waits.
It retains the report descriptor until afterward. No deadline was increased and
last-good frame/metadata publication remains unchanged.

The additional `cleanup-master-first` case uses the actual existing stalled Python
peer, observes pending terminal output and identifies the owned master through
public descriptor/TTY checks. At the forwarded actual waitpid boundary it requires
the master to be physically closed after signalling, then checks child reaping
and all descriptor closure. Before repair it exits 10 at `cleanup waited with
owned PTY master still open`; afterward it passes. Local red/green receipts live
in `/tmp/zmux-pty-master-first-20261015/`. Linux verifies physical ordering, not
Darwin's blocked-exit kernel behavior; the hosted stall is the actual Darwin red.

The final nine-input freeze is
`/tmp/zmux-f04-pty-master-first-frozen-manifest-20261015.json`, SHA-256
`b2be5b3b5de1dc5c45507b1219b5df20398af8ce680c7b0e229de82e2b9410c2`.
All 31 base inputs remain fixed. Author focused acceptance passes construction,
all three cleanup cases, all seven PTY modes and both intended controls without
another engine build:
`/tmp/zmux-f04-pty-master-first-author-20261015/receipt.json`, SHA-256
`1cfdfadddc21b0dc3bf23e49319bf9284c0c3ce0fcdc4457de99aeb095f048ea`.
The independent parent run matches this freeze and passes the same assertions:
`/tmp/zmux-f04-pty-master-first-parent-20261015/receipt.json`, SHA-256
`b9040642bb17c57a74a0397ac0c2262fb2535ad10f1d829853a14bf1cb618122`.
Independent source review finds no blocking issue. Current fixed storage remains
296 C bytes, 12,096 inline coordinator bytes and the same 24,576-byte reply/input
subset. Exact-source hosted Darwin acceptance of this ordering repair remains open.

## Darwin normal acceptance and negative-control specificity

Workflow [38787910315](https://github.com/ashutoshpw/zmux/actions/runs/38787910315)
at `11258fead00c71dc24652280504b82def45307b9` passes Linux. Darwin arm64 now
passes construction, all three cleanup probes and **all seven normal PTY modes**,
including stalled-reader timeout/reaping. Flood observes one actual EAGAIN and one
kernel short write; stalled input observes 38 EAGAIN and one kernel short write.
The master-close ordering repair therefore has actual Darwin normal-path evidence.
Darwin receipt SHA-256 `c197160da8972d1bb8cc823422b5dfc5d8e4614c97e5f39494f55e2c714e86af`
matches all nine frozen inputs and the complete 31-input base lock.

The overall Darwin job still fails: the intentionally broken early-publication
control reaches a later `mode == "stall"` assertion in the flood path instead of
its required publication-invariant assertion. This is a negative-control coverage
gap, not a repeated transport cleanup failure. A successful kernel short write
could publish the broken candidate before the next EAGAIN or commit; the fixture
only checked at those later boundaries.

The strengthened fixture checks unchanged publication whenever a pump leaves a
staged reply suffix, including after a positive partial write. The basic journey
uses a three-byte syscall cap and requires observed partial reply writes so the
control cannot depend on a particular kernel's buffer segmentation. That cap is
physical syscall fragmentation, not a claim of a kernel-returned short write.
The flood journey retains its actual kernel short-write/EAGAIN observation. Basic
resize waits until staged/pending input is empty, preserving admission rules even
when the duplicate control appends an extra byte after the child's cursor receipt.
No transport, engine, lifetime, cleanup deadline or negative assertion was relaxed.

The previous accepted binary/C objects were no longer present, so one pinned
reconstruction was needed for this actual regression. Reusable source, toolchain,
patched library, objects and receipts are now retained in ignored
`target/terminal-engine-artifacts/20261015-publication-boundary/`. A nested Git root
isolates patch application from the parent checkout, and header/symbol postconditions
are checked. Producer receipt SHA-256:
`cefd9ea053b59abf33ddc6433ba759be3e374ece27fa7b6f5dad2924a9e73aaf`.
Actual library SHA-256:
`bdc3fc3c6ec69875fed19c220082412b2299bb2fce75cc4e1f49b7fc8673d891`.
This is a new concrete build, not a claim that equal source pins reproduce the
lost binary's path-dependent archive bytes.

Focused red/green receipt is `focused-v2/receipt.json` in that retained directory,
SHA-256 `383607bed1de7b1fe69ac5457743999dd55381b5d771ae60daaa6ca61d190a44`.
The pre-invariant cap-three early control fails at the later basic-transport
assertion (red specificity check). The strengthened normal journey passes with
four physical partial reply writes; both controls fail their exact named assertions
with exit 101/no timeout. Construction and all three cleanup cases pass. All 31
base inputs remain fixed. Final nine-input freeze:
`/tmp/zmux-f04-pty-partial-publication-frozen-manifest-20261015.json`, SHA-256
`a8715b5e615d6b13ee601a37f0c8d26c63840231a3ed12a91196e93d58c9a27b`.
Parent independent source review finds no blocker, and a separate focused run
passes the strengthened normal invariant and both exact controls without rebuilding
the engine. Parent receipt `parent-focused/receipt.json` in the retained directory,
SHA-256 `7a71517e626a5f6dfac1534588627024526df93f72d3520d4b535b2cb267738c`.
Full current-main `scripts/check` also passes before delivery; log:
`/tmp/zmux-pty-publication-integrated-check.log`.

## Final exact-source hosted PTY acceptance

Workflow [38791084172](https://github.com/ashutoshpw/zmux/actions/runs/38791084172)
at `1abc98d1fc778083f1d19cd4a78ff93fa0a5c39a` passes both Linux x86_64 and
Darwin arm64/macOS 15.7.9. Downloaded receipts match the complete final nine-input
freeze and unchanged 31-input base lock. Each executes construction exhaustion,
all three physical cleanup probes, all seven normal PTY modes and both controls.
Both controls exit 101 without timeout at their exact named physical-order and
publication-boundary assertions. These are executed fixtures, not compile/skipped
job inference.

- Darwin receipt SHA-256:
  `37b9f3a449149d4824451edadd274567ae7857b49061d480cadb1b74245111d1`.
  Four deterministic partial reply writes preserve publication; actual flood
  EAGAIN/short-write counts are 1/1, stalled-reader counts 43/1. Stalled input
  times out as intended and cleanup reaps/closes owned resources successfully.
- Linux receipt SHA-256:
  `28f470a68113b63330bc59c00d59b02bf118ecc5f500a01701e9c57938134fab`.
  Four deterministic partial reply writes preserve publication; actual flood
  EAGAIN/short-write counts are 1/0, stalled-reader counts 232/0.

Downloaded evidence is under `/tmp/zmux-pty-hosted-38791084172-{macos,linux}/`.
Darwin RLIMIT_AS remains unavailable with ValueError; CPU/core limits were applied.
Neither the configured allocator cap nor these fixture limits establish total RSS.
This closes the exact-source isolated PTY/negative-control gate on the two runner
architectures. It does not establish a controlling terminal, native input/shell/job
control, AppKit integration, a default-engine decision or named-Mac workloads.

## Capacity ledger and remaining boundary

These are allocated/reserved capacities or configured limits, separately from
the observed 768-byte pending high-water. Linux sizes from the frozen cleanup follow-up:

| Storage | Bytes / bound | Accounting |
| --- | --- | --- |
| Engine mappings | 16,777,216 combined configured maximum | Same accepted bounded allocator; all mappings, including temporary/cached storage, charged until release. |
| Accepted fixed C owner/collector/metadata sidecar | 201,056 | Same accepted fixed charge; includes Probe/collector reply payloads and fixed event/raw metadata capture. |
| Accepted Rust backing allocations | 1,693,696 | Both preallocated frame/effect/metadata slots and operation scratch; no coordinator duplication. |
| Complete new Rust coordinator inline storage | 12,096 | Includes the accepted metadata owner's 3,064 inline bytes, transport handle, 8,192-byte input, 512-byte read storage and 256 report flags; do not add accepted inline bytes again. |
| New fixed C transport owner | 296 | Descriptor/status fields, first cleanup operation/errno and 128-byte saved report; separate from accepted fixed C charge. The predecessor owner was 280 bytes. |
| Reserved reply/input payload subset | 24,576 | Probe 4,096 + collector 4,096 + two publication slots 8,192 + transport input 8,192; already included in rows above, never additive. |

The original Probe's reply storage remains reserved even though the active
collector handles callbacks. Engine transient reply allocations stay charged to
the engine quota. Report scratch, call stacks, fixture-created observation copies,
child heap/threads, runtime/allocator overhead and kernel PTY/pipe buffers are
separate and excluded from a total-RSS claim. No per-operation coordinator payload
allocation is needed for prepare/validate/exact commit/slot swap.

The final nine-input publication follow-up has both focused and exact-source
hosted Linux/Darwin acceptance, including the intended negative controls. The
metadata workflow alone does not prove transport; the separate PTY run above does
so within its isolated runner/fixture scope.
Native input/shell/job-control, production adapter integration and
representative workloads remain open gates; the application engine is unchanged.
