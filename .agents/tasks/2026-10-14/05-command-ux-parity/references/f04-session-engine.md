# F04 explicit Session engine integration

2026-10-15. Original MIT production worker integration; no default/native switch.

`Session::spawn_with_engine` takes explicit `EngineOptions`: an engine kind,
caller-selected mapping caps and a positive finite reply lifetime <=30s. The
`ghostty` Cargo feature links the accepted optional engine. Session owns its
selection enum so dependency feature unification cannot implicitly enable a worker
backend; adapter-only and Session-enabled composition are checked separately. Ordinary constructors
retain the current direct vt100 parser, publication cadence and input/wheel path;
no product mapping/deadline policy is inferred from fixture settings.

The same worker constructs the sole terminal and initial complete viewport before
PTY child launch, then owns every mutation/export/physical write/commit/destructor.
A staged output batch is copied and validated in the safe adapter. The worker
finishes its already admitted user chunk before writing the causal reply through
the same nonblocking PTY descriptor. It advances by actual write counts, retains
partial suffixes, and commits the exact generation only after every reply byte is
physically accepted and cancellation/deadline has been rechecked. Zero-reply stages
still commit explicitly. Snapshot byte counters may advance while the viewport,
metadata and notices remain at their previous committed generation.

API queue acceptance is not worker admission. The existing bounded16-item external
queue may accept chunks while a reply is staged; those chunks retain relative
order and receipts but are not dequeued until reply commit. No new PTY output read,
engine resize/scroll or worker input admission occurs during that stage. POLLIN is
suppressed while physical writes block; the worker retains POLLOUT, cancellation,
reply/input deadlines and child-exit observation. This avoids starving protocol
replies behind a permanently arriving external queue.

The explicit reply lifetime includes time blocked behind the current user suffix.
Its deadline, a blocking user deadline, write failure, zero write or child exit
aborts the owner without replay/fallback and preserves the prior committed frame.
Input receipts finish once with known accepted prefixes; incomplete/unknown results
never authorize replay. Cancellation aborts any staged generation before the
existing Kill or Graceful flow. Graceful output drain remains read-only, and the
shared earliest close deadline still reserves the existing2s post-exit drain;
successful shutdown must confirm cleanup; join/is_finished alone report worker
termination. No engine budget renews that
root deadline. Failed worker/resize/constructor paths retain their existing owned
resource and physical child cleanup boundaries.

Coalesced engine resize stages first, kernel PTY resize follows, and exact commit
follows successful kernel resize and any causal reply write. Kernel failure is
visible and aborts the session; rollback atomicity is not claimed. While a causal
reply is pending the coalesced resize stays queued. Completed history can scroll
without a live PTY; an unexpected reply there fails visibly rather than dropping it.

Raw input remains exact. Explicit history-wheel navigation is supported for the
candidate. Application wheel encoding is refused before queue admission until its
authoritative engine encoder is integrated; it is never silently downgraded to
history. Typed keys/paste/native modes, richer styles, selection/IME, actual Zellij
launch wiring, Darwin Session execution, named workloads and default policy remain
gates. The existing real Zellij fixture's DA/DSR responder is not copied into the
product: the selected engine owns production protocol replies.

## Scoped evidence

Four real public Session/PTY tests exercise literal child-observed DSR replies,
raw PRE/POST bytes, actual kernel resize, stalled causal publication with deferred
kernel resize, full worker-admitted input suffix before reply and queued POST after
it, deadline/cancellation last-frame retention and one-time receipts, and initial
engine allocation failure before child launch with worker-owned resource cleanup.
The blocked fixture first waits for observable PTY input receipt quiescence while
the child does not read; emitting output immediately after API enqueue is not a
valid worker-admission/backpressure precondition. No production test injection or
fixture VT responder is used. Default Session tests and strict default/linked
Clippy pass locally. Independent review/full repository and hosted native evidence
are recorded separately before landing/acceptance; none is implied by source alone.

## Optional Session Darwin acceptance lane

The retained-linkage workflow now offers `include_session_runtime` (defaulttrue)
without changing any product engine default. Its `--session-runtime` driver mode
retains the original adapter checks, freezes Session and vendored Darwin wrapper
sources, and compiles the actual public Session engine fixture for both Apple
targets. Exact Cargo package/test messages select each executable; each Session
must reuse the same hash-checked host shim object already inspected for its target.
Raw Mach-O architecture/minOS14/SDK15.5 and runtime-load inspection happen before
only the ARM executable runs all4 real PTY cases, serialized with a60s owned phase
limit. Intel is compile/link/inspection only. Original engine binaries and producer
receipts remain unchanged, and Zig is never invoked.

Session requires libSystem and may additionally load only `/usr/lib/libproc.dylib`
and `/usr/lib/libiconv.2.dylib`. The Cargo.lock-pinned `darwin-libproc-sys0.2.0`
build script explicitly links `proc`; pinned `libc0.2.190` links `iconv` from
`src/unix/bsd/apple/mod.rs` outside `rustc-dep-of-std`. The linker may omit unused
dylibs. The adapter remains libSystem-only. Unknown libraries, duplicate/missing
required loads and runtime search paths refuse. Receipts separate adapter from
Session execution and preserve hashes and logs. Candidate executables are retained
with hashes before inspection so a rejected candidate remains inspectable; only
successful inspection permits execution.

Local policy tests pass29 with1 native-only skip, including exact Session artifact
selection and the narrow dynamic-load policy. A real Linux Cargo no-run build
selects the expected Session executable and host shim through the same message
parser. YAML and shell syntax checks plus the Session source/input gate pass.
These are tooling checks; actual Darwin Session execution remains unaccepted until
the committed lane is dispatched and its artifact receives independent review.

The first optional Session attempt, run `38809109753` at
`11671a48ddb484f8730ddf4df43e6a5394d7e886`, compiled ARM Session successfully but
refused its runtime closure (`libSystem` plus system `libiconv.2`) before execution.
ARM adapter4 passed; Intel was not reached. Artifact `12007776524`, ZIP SHA256
`e4c736b4ccfa1e5e1ab267ea3bf2a8c5313e32adb5c962ae08670e96330ddc0b`, retains
that failed receipt and compile log; its Session binary was not retained by the
old driver. No Darwin Session execution is accepted from that attempt.

Source inspection and `cargo tree --locked -p zmux-session-runtime --features
ghostty --target aarch64-apple-darwin -i libc` establish the pinned Session
dependency path. The existing load-policy table now reproduces this exact refusal
on the old policy and passes with only the justified system path added. It also
rejects duplicate iconv loads, alternate iconv paths, adapter iconv loads and
runtime search paths. The focused command is `python3 -B -m unittest discover -s
tools/terminal-engine-artifacts -p test_linkage.py -v`:8 tests, red on the missing
iconv allowance, then green after repair. No dependency pin or default changed.

Run `38809909034` at `94667fba5f60440836efb7f52bbe254891e4d100` passed ARM
Session compile/link/minOS/SDK/runtime closure inspection and executed the fixture.
The child reply/resize/raw-input case and constructor-failure case passed. Both
backpressure cases failed in setup: no complete4096-byte input receipt was observed
before the unchanged3s quiescence bound. This does not distinguish an initial
partial write from zero accepted bytes. Intel was not reached; no complete Darwin
Session acceptance follows. Artifact `12008600494`, ZIP SHA256
`0d51b8dab0c2cb9cf84a304dd5ae104a3f18fc9a8136161b4feeb9c8146fc4ac`, now
retains the actual candidate Session binary as well as the failure log.

The next diagnostic attempt preserves the fixture's existing acceptance condition
and deadlines. A separate bounded raw `openpty` probe records each4096-byte write
return, exact accepted-byte readback, EAGAIN and zero-write evidence. Its finite
stop reason is `would_block`, `zero_write` or `attempt_limit`; these observations
do not gate Session execution or establish backpressure acceptance. Unexpected
OS errors or readback corruption still fail. On fixture
quiescence failure, explicit Session cancellation collects public input receipts
with confirmed bytes/status before retaining the same failure. This establishes
native capacity and first partial/zero-write evidence before any fixture repair;
it changes no Session production source. All4 linked Session cases still pass on
Linux; the29-test policy suite retains its1 native-linker skip and193-entry source
gate passes. Native diagnostic results remain pending.

Run `38811818507` at `0ffc1105da85b51568172cb7946665d67923ff1e` establishes
the native cause: Darwin24.6 ARM raw PTY accepted1022 of4096 bytes, then EAGAIN,
with exact1022-byte readback and no zero write. Both public Session backpressure
setups independently reported zero completed chunks, receipt0 `Partial` with
1022/4096 confirmed bytes and15 queued `NotWritten` receipts after explicit close.
The same original quiescence assertion failed. Artifact `12007814726`, ZIP SHA256
`dd2a740150ded3950b9f5114b1be0017da7151706177e16ab12c07a0a34d27ac`, retains
the raw probe, actual receipts and candidate binary. Exact uploaded bytes,193
source hashes and raw ARM Mach-O outputs were independently verified in
`/tmp/zmux-darwin-session-failed-review-38811818507.json` (SHA256
`0abbc1f4c649314bf0d4ffc02c9c3a7b1a22affe155e97611266e4792df2cf0d`).

The fixture repair allows stable zero completed chunks, preserving the same100ms
stability window and3s setup bound. The child now verifies an exact prefix of all
previously completed4096-byte chunks plus the entire current chunk before the
literal causal reply. Cancellation and deadline cases require a positive confirmed
accepted prefix from completed chunks plus public receipt bytes, and every still
pending receipt must be incomplete. Receipt evidence is printed for both variants.
Session uses libtest `--show-output` to retain these diagnostics without disrupting
the exact per-case success lines. No production Session source or deadline changes.
All4 linked cases pass on Linux:13824 accepted bytes before cancellation/deadline,
and16384 exact child-observed bytes before the reply on release. Focused8 policy
tests and193-entry input gate pass. Darwin execution of this repair remains pending.

Run `38812468146` at `a2e3d09bfbee326076dffe926ff472b5244d0aa3` executed
three Session cases successfully, including the repaired release path: zero
completed chunks before staging and exactly4096 child-observed user bytes before
the reply. The deadline variant failed because its published failure did not
retain the causal deadline text. The log separately reports process-group cleanup
`EPERM`; source inspection shows cleanup overwrites the primary worker failure,
then the guard retries cleanup during Drop. Artifact `12009411485`, ZIP SHA256
`3e1187c11b1cfaa5c8e2b03376f1400cfa77a558984878a49d3b64763a2617b2`,
retains the attempt. Intel was not reached and complete Session acceptance remains
open. This is no longer the4096-byte fixture setup failure.

The next narrow diagnostic repair preserves the primary failure and appends a
separately labeled `PTY cleanup failed:` error. Unexpected process-group signal
errors retain signal, physically owned child PID, observed group and nonreaping
owned-child job/exit status. Signal/reap decisions remain unchanged: no permission
error is suppressed and physical cleanup is not claimed successful. The public
Session case explicitly rejects cleanup failure after shutdown, so retaining the
deadline text cannot turn a cleanup failure into acceptance. Local default
Session18 and linked4 cases pass; the actual Darwin cleanup cause remains pending
the retained primary/error-state evidence.

Diagnostic run `38813623938` at `b831facf4bf30838705944d4813a81a7445ca948`
preserved the primary causal deadline and correctly failed its explicit physical
cleanup assertion. The unexpected `SIGKILL` group error was `EPERM`, while
`getpgid` for the physically retained child returned `ESRCH` and the nonreaping
owned-child observation returned `None`. Three cases passed, including the exact
4096-byte release prefix. Artifact `12010016250`, ZIP SHA256
`bf2ccdca16de95cc42db3026d69a481b8f01dffd1ecceb2c9d717fa171e9affc`, retains
this evidence; it does not establish successful cleanup.

The scoped repair handles only that `EPERM` plus `getpgid=ESRCH` combination by
asking the actual retained child handle to `try_wait`. Only `Some` (actual reap)
sets the guard's `reaped` flag immediately and succeeds; `None` or error retains
the original signal failure. No later signal can target the released PID. The
rare actual-reap event records the child status. This proves direct-child reap,
not extinction of every descendant/group member; it does not authorize signalling
unrelated processes or broad permission-error suppression.

A sticky private cleanup-failure flag is set before fallible snapshot publication.
Both shutdown policies check it after joining, and repeated shutdown calls retain
the error. `is_finished` documents worker termination only. A new public regression
uses a separately owned subprocess whose Python launcher sets `SIGCHLD=SIG_IGN`
before exec; its actual PTY child is auto-reaped by the kernel, inducing real
`ECHILD` cleanup failure without changing the test runner's signals or adding a
production seam. Exact old `b831fac` source returns false `Ok(())` for Kill and
fails this regression. The repair returns the retained cleanup error for both
policies and repeated calls. Red evidence is retained under
`target/agent-validation/session-cleanup-outcome-negative-control/old-false-success-red.log`.
The focused regression and full linked Session23 cases pass locally, including
strict all-targets Clippy. Actual Darwin reap and optional Session acceptance
remain pending the repaired lane.
