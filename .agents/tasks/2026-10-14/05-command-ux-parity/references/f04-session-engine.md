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
cleanup is proved only by completed join/is_finished. No engine budget renews that
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

Session requires libSystem and may additionally load only `/usr/lib/libproc.dylib`:
the Cargo.lock-pinned `darwin-libproc-sys0.2.0` build script explicitly links `proc`,
which the linker may strip if unused. The adapter remains libSystem-only. Unknown
libraries, duplicate/missing required loads and runtime search paths refuse.
Receipts separate adapter from Session execution and preserve hashes and logs.

Local policy tests pass29 with1 native-only skip, including exact Session artifact
selection and the narrow dynamic-load policy. A real Linux Cargo no-run build
selects the expected Session executable and host shim through the same message
parser. YAML and shell syntax checks plus the Session source/input gate pass.
These are tooling checks; actual Darwin Session execution remains unaccepted until
the committed lane is dispatched and its artifact receives independent review.
