# F04 candidate adapter contract and migration gates

2026-10-15. Design only; no production engine, dependency, capacity or schema change.
F04/T01 remain open. Keep `vt100` as the production default until the gates below pass.

## Scope and evidence

This is the next bounded step from the [engine evaluation](f04-engine-evaluation.md),
not a production-selection decision. It covers the full F04/T01 requirements in
[work packages](closure-work-packages.md): protocol/render/input fidelity, ordered
PTY/resize/backpressure, metadata and effects, bounded resources, native selection,
IME/VoiceOver, and measured Mac behavior before expanding limits. Graphics, find,
links and richer styles remain required T01 work even when absent from the first
experimental exporter; disabling them does not close their audit rows.

Source review used Zmux `d3d5f05bb05849e8f8f270e692e5913309db3e88`:
`crates/terminal-adapter/src/lib.rs`, `crates/session-runtime/src/lib.rs`,
`apps/macos/Sources/TerminalView.swift`, and `NativeBridge.swift`. Upstream review
used the checksum-verified Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be`
archive already recorded by the feasibility harness. No cmux implementation or assets
were used.

**Executed:** workflow [38756429963](https://github.com/ashutoshpw/zmux/actions/runs/38756429963)
at `0a5d52e` passes isolated Linux x86_64 and Mac arm64 text/reflow/query/snapshot
assertions and 128 create/free iterations. Both Mac architectures compile and both
universal library/executable slices verify. Mac x86_64 execution is absent. The
Unicode observations concern cursor columns, not AppKit glyph/selection correctness.

The isolated compatibility-exporter follow-up now implements the basic style,
cursor/mode, text and scroll/history mappings below; its exact executed receipt is
recorded in the engine evaluation. It is not an application adapter. Mode-synchronized
key/mouse encoders, title/cwd/bell/notification effects, allocator handling, failure
recovery and session/native integration remain **source-derived only**. Their public
headers establish available interfaces; they have not been executed through a Zmux
candidate application adapter.

## Ownership and next executable slice

Keep one engine owner on each session worker. Only owned Rust frame/effect/input
values cross to desktop/native consumers; AppKit continues to draw, compose text,
select, manage focus and mediate user actions. Keep the same ownership for local,
SSH and local-tmux PTYs. No engine handle or render iterator enters Swift or the
core action executor.

The first slice is an **isolated compatibility exporter**, alongside the existing
feasibility tools, producing the existing Rust `Viewport` shape from pinned C state.
Use an original narrow C shim with opaque handles and explicit lengths/results;
Rust consumes Zmux-owned structs, not upstream enum layouts or borrowed unions.
Reuse the current public `Size`, `Run`, `Style`, `Color` and `Viewport` contracts
instead of creating a second application frame model. It is not linked into the app.
Run the fidelity matrix below against that exporter before changing session workers.
The initial executable subset covers text/basic styles/cursor/history; richer fields
are explicitly rejected or listed as unsupported, not declared compatible.

Later integration needs a deliberate startup change: `Session::spawn_with_resource`
currently constructs `Terminal` and its initial viewport **before** moving them into
the worker closure. A `PhantomData<Rc<()>>` Ghostty owner must instead be created,
used and destroyed on its worker. Pass validated size/configuration into the worker,
initialize the engine before launching its PTY child, and publish a bounded startup
result through the existing background launch coordinator. Initialization failure
must release partially created resources and leave a retryable launch failure;
never publish a running session or add an `unsafe Send` assertion to bypass this.

## Viewport compatibility and deliberate extensions

| Contract | Candidate mapping and required fidelity |
| --- | --- |
| Text and cell placement | Update render state once; copy only visible rows. Use raw cell wide tags plus full grapheme UTF-8. Emit a wide head once at its grid origin, omit its tail, and treat a soft-wrap spacer head separately. Emit blank cells with correct background. Coalesce only contiguous compatible ASCII runs, as today. Do not reuse the scratch dump, which inserts continuation blanks into text. |
| Styles/colors | Preserve default/indexed/RGB, bold/italic/underline/inverse for the current subset. Test inverse exactly once, empty-cell backgrounds, palette entries and resets. Engine-resolved RGB supports OSC palette/default overrides, but current AppKit has fixed default/palette/cursor colors: frame defaults/palette revision and cursor color need an explicit shared extension or all affected run colors must be resolved coherently. Faint/invisible/strike/overline/blink/underline variants/colors cannot silently become full-style parity through today's six-field `Style`. |
| Cursor/modes | Export cursor visibility, application-cursor, bracketed-paste and active screen from the same engine state. Current vt100 can report `cursor_col == cols` for pending wrap; Ghostty reports the last cell. Specify a common logical-versus-draw cursor contract and independently test wrap/newline/backspace before changing either producer or consumer. Do not fabricate pending-wrap state by watching input bytes. Cursor shape/blink and additional modes require versioned frame fields. |
| Capture/selection | Generate `contents` and runs from one snapshot with explicit trailing-space/newline rules, including soft wraps and wide graphemes. Retain current `read-screen` semantics deliberately. Native selection/copy currently reasons about runs and resets on resize/history movement; row identity, wrap flags and selection anchors need shared contracts before persistent/reflow-aware selection, links or find. Never export untracked engine pointers as anchors. |
| History/resize | Derive visible history offset from scrollbar total/offset/len with checked arithmetic; do not confuse offset from oldest history with distance from live bottom. Primary reflow and alternate-screen behavior differ. Verify width and height changes, scrolled anchors, history pruning and `RESIZE_PULL_SCROLLBACK` explicitly. Geometry remains 1–120 rows and 1–240 columns. |
| Metadata/attention | Copy title/cwd changes and bell/OSC notifications into current owned metadata and sequenced notices. Preserve display sanitization, title limits, notice-ring semantics and desktop deduplication. Validate cwd separately from display text: OSC 7 is a URI, while other upstream cwd forms can be bare paths. Decode once, preserve or reject the exact path, and keep remote observations from becoming a local launch/authority decision. Metadata never executes shell commands or reads files. |

For baseline compatibility the exporter must identify every unsupported field or
lossy conversion. A subsequent frame revision must be coordinated with native
decoding/rendering and other consumers; adding a Rust field alone is not acceptance.
Keep graphics and clipboard-read/write callbacks disabled initially. Terminal-reported
capabilities and `TERM` must match executed support; do not advertise a complete
Ghostty application or enable file/shared-memory image loading with a parser change.

## C ABI lifetimes, allocation and callback budgets

The pinned C API is unstable. Compile the shim against the exact pinned headers;
initialize every sized upstream struct, check every fallible C return, and validate
all output tags/lengths before conversion. A pinned boxed C context owns terminal,
render state, encoders, iterator handles, allocator state and callback buffers.
Free dependent iterators/render/encoder handles before the terminal; keep callback
and allocator contexts alive through all frees. Rust RAII handles every partial
constructor failure. No panic or exception may cross a C callback boundary.

Render row/cell views expire when render state updates; title/PWD strings expire
on terminal mutation; notification/unknown-sequence payloads expire on callback
return. Copy while valid, then publish immutable Rust-owned values. No `&'static`
casts, borrowed data in `Arc<Viewport>`, pointers in queued effects, or retained
untracked grid references. Read one coherent frame before the next mutation.

Callbacks run synchronously. They only copy bounded data or mark metadata dirty;
they do not write PTYs, lock UI/global state, invoke native APIs, allocate without
a checked budget, block, or reenter VT processing. Query and metadata callbacks
share one stable context. Collect ordered query bytes and notice events during
feed/resize, and drain after the C call returns. Title/cwd may coalesce to their
latest values; bell/notification sequence order must remain explicit.

Experimental starting bounds, **not approved product memory/performance budgets**:

- Keep current 120×240 geometry, 16-KiB control-string policy, 8-terminal capacity,
  4-KiB PTY read chunks/four-read quanta, and 16×4-KiB ordinary input admission.
- Retain the feasibility limits of 256 UTF-8 bytes per cell and 512 KiB copied
  text payload per frame. Bound cell/run counts independently by rows×columns and
  account for Rust vectors, structs, strings, metadata and serialized JSON as well;
  512 KiB of text is not a 512-KiB total allocation bound. On overflow retain the
  last good frame and report explicit session failure; never truncate a grapheme.
- Reserve 4 KiB of query-reply bytes per feed/resize operation, drained before
  further engine input when capacity is unavailable. Bound metadata titles to the
  existing 256 display characters and cwd to 4,096 UTF-8 bytes. Preserve at most
  32 sequenced notices with 128-character titles/2,048-character bodies; a 320-KiB
  payload ceiling allows their bounded UTF-8 representation. Record overflow/drop
  reasons explicitly instead of silently allocating or pretending every notice arrived.

`vt_write` returns void. Its sticky `VT_PROCESSING_ERROR` flag excludes gracefully
handled limits and external-effect/query failures and cannot be reset. Track shim
overflow, denied allocations, failed C calls and protocol-policy rejection separately.
After a fatal event, stop consumption and publish failure plus the last good frame;
do not replay the already partially consumed feed into vt100 or silently reset the
engine. Validate unsupported/malformed-sequence recovery separately from fatal caps.

The existing control-string guard is a resource policy, not a second terminal
parser. Preserve its boundary with fragmented escape/UTF-8/ST/BEL/CAN/SUB fixtures;
the engine's unknown-sequence buffer cap does not bound all known control strings.
Synchronized-output holds also need a bounded publication policy so an unfinished
hold cannot hide output forever or accumulate application frame copies.

## History and hard-memory decision blocker

`SCROLLBACK_MAX_LINES=1000` and a byte preference do **not** preserve today's exact
1,000-line retention guarantee. Upstream prunes complete pages; row overshoot varies
with content and width. Its approximate 400-KiB page description is not a contractual
maximum, and resize can reflow retained history. The reviewed terminal/screen C API
has no arbitrary oldest-N-row prune operation; clearing all history is not equivalent.

There is a second, source-derived limitation: [`PageList.pageAllocator`](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/PageList.zig#L566-L585)
uses the OS page allocator on native Linux and a tagged Mach page allocator on
Darwin. It uses the embedder allocator for those pages only on native freestanding
targets. A custom `GhosttyAllocator` quota **cannot alone cap native screen pages**.
Upstream memory-usage counters omit some bookkeeping; RSS/footprint and total engine
allocation are separate measures. Checking them after feed permits transient overshoot.
Mac hosted `RLIMIT_AS` was unavailable, and an in-process worker cannot be safely
cancelled midway through a stuck C call. No hard memory or time bound is established.

Recommended path: keep the candidate isolated while seeking a separately reviewed
upstream API for exact pruning and/or enforceable page-allocation budgets. Validate
all four allocator vtable operations, alignment exponent conversion, checked byte
arithmetic, temporary relocation peak and failure cleanup; audit allocation paths
outside the supplied allocator. Do not change pins or patch upstream in this design.

If an exact internal-history policy is intentionally relaxed later, make that a
separate explicit decision: expose at most 1,000 rows through every capture, scroll,
search and selection path, report actual retained pages/rows, and enforce a reviewed
total-memory envelope. Clamping a scrollbar alone does not hide old rows from search
or prevent reflow from restoring them. This is not the current policy and is not
approved here. Process isolation with a real OS-enforced resource budget is another
architecture option, with IPC/copy/latency costs to measure; the Mac fixture does
not demonstrate that budget. Do not switch production until this choice is resolved.

## Input, effects and PTY ordering

Preserve one worker-owned nonblocking PTY writer with explicit partial-write state.
Raw `send-text` bytes remain exact. A later typed key/pointer/paste/focus event path
must encode on that worker using authoritative modes, after AppKit has consumed
Command shortcuts and IME composition. Commit composed text once; key release/repeat
events are distinct from committed UTF-8. The key encoder's terminal-sync operation
resets option-as-alt, so apply the user's native option policy afterward.

Wheel/history overrides remain policy, not encoder output: Shift/history navigation
does not emit mouse bytes, active tracking uses current modes, and alternate-scroll
behavior is tested independently. Button/motion/release/focus paths are new T01 work.
Read current bracketed-paste mode at worker execution, rather than wrapping from a
stale native frame. Pasting and raw byte injection stay separate contracts.

Query replies generated by feed, paste or resize enter the same ordered write FIFO;
never write from the callback or create a second writer. The currently partial write
must finish before another byte sequence begins. Define FIFO order at worker
admission, preserve already accepted user-event order, and append each generated
reply batch at its causal feed/resize position. Reserve reply capacity and stop
reading more PTY output while it is full; use ordinary PTY backpressure. Specify
fair dispatch quanta and control admission rather than allowing query or user floods
to starve the other path. Overflow is visible failure, never dropped replies or an
automatic retry of partially written input. Cancellation/close fails pending receipts
once, discards undeliverable effects explicitly and frees the engine on its worker.

## Static build and notice closure

Retain the exact Ghostty/Zig/Rust/archive pins and current minimal VT features.
Keep build/download work explicit and off UI/core paths; no network download from
Cargo `build.rs`, no host-library discovery fallback and no app dependency change
in the exporter slice. Continue manual feasibility CI before any app linkage.

Before candidate app linkage, build static libraries for Linux x86_64 and separate
Mac arm64/x86_64 targets with matching SDK/minimum macOS target/C/Rust ABI; verify
slices, link actual owners, and execute each claimed host architecture. Package
an immutable artifact manifest containing source/archive/header/shim hashes, patches,
toolchains, flags/features, target/SDK/deployment version, link commands, dependency
hashes and output hashes. Consumers verify it; missing/mismatched artifacts fail
closed. Universal compilation remains distinct from Intel runtime acceptance.

Audit the actual static link closure, not the Ghostty application's whole manifest.
Record which fetched packages are build-only and which code/data ships; include
Ghostty MIT, applicable uucode/Unicode data, zlib, Zig/compiler-runtime and any other
linked components with their exact license texts/notices. Existing root-MIT checks
and the observed translate-c/aro fetch list are insufficient. Produce an inventory,
license texts and third-party-notice update with the artifact before distribution.
Do not distribute upstream binaries as ordinary log evidence or imply the current
feasibility workflow has completed this audit.

## Executable fidelity gates

Add independent expected inputs/outputs at their actual owner, not assertions that
mirror shim internals. Existing portable/native tests remain regression gates; no
new test-only production export is needed for the isolated C/Rust owner.

| Owner | Required bounded scenarios before migration |
| --- | --- |
| Isolated C/Rust exporter | Fragment every UTF-8/CSI/OSC boundary; independently expected rows/runs/colors/styles; blank backgrounds, wide tails/spacers, combining/ZWJ/VS16 with mode transitions, pending wrap, primary/alternate resize, height changes and retained snapshot lifetime. Compare current working subsets and record intentional corrections without treating baseline defects as the oracle. |
| Metadata/effects | Exact title/cwd/bell/OSC 9/777/reset sequences, invalid UTF-8/URI/controls, metadata floods and sequence retention; deny clipboard/file/image effects; assert enabled capability replies match support. Callback payload lifetime and oversized reply/cell/frame cases must fail visibly. |
| Allocator/C ABI | Failure at each constructor/export/feed/resize allocation boundary, including native page allocations once enforceable; all partial handles freed once, accounting returns to baseline, repeated create/drop, ASan/UBSan where supported and no escaped borrowed data. Feed/query effects may already have happened when failure is reported; test that no replay occurs. |
| Real session PTY | Child sends cursor/device/mode/size queries and receives exact bytes once; interleave partial user writes, replies, resize, paste, queue pressure, blocked readers, output floods, EOF, cancellation and close. Verify immutable frames, bounded queues, no extra PTY writer and inspectable exited history. Key/mouse tests read independent bytes at the child, not a private encoder helper. |
| Packaged AppKit | Preserve legacy special-key fixture, Command precedence, selection/copy of wide/reflowed text, bracketed paste, marked-text insertion, pointer/history/focus routing, title/cwd attention and exited-output accessibility. Exercise actual renderer/PTY; physical IME/layout/trackpad/VoiceOver and Mac x86_64 remain separate required receipts. |

Any new behavior repair needs the same focused regression red before fix and green
afterward. Fixture C/ABI tests establish that boundary only; source support or a green
fixture must never mark all T01/native/graphics rows verified.

## Controlled Mac workloads and migration decision

Extend the existing [external sampler](f04-native-resource-sampler.md) with explicit
phase ownership. Run the unchanged production baseline first on a named dedicated
Mac, then the candidate with identical shell fixtures/size/font/build options; record
SHA, engine pin, OS, architecture, hardware, RAM, display and power/thermal context.
Use five paired repetitions with 30-second warmup and 120-second measured phases:

1. One idle shell, then eight idle shells; report app versus owned child/helper cost.
2. One visible producer at fixed 1 MiB/s, then eight producers at the same per-session
   rate with seven hidden and a visible foreground probe. Record actual throughput,
   skipped/dropped frames and input/output fairness, not only CPU averages.
3. Eight terminals plus zero/one/four browser surfaces, with idle and fixed fixture
   browser activity separated. Attribute WebKit helpers explicitly; PPID-only totals
   do not establish browser cost or physical footprint.
4. Authenticated SSH output/input backpressure and a controlled remote stall while
   the local foreground probe continues. Disconnect/reconnect and close during the
   stall; record input delivery/unknown outcomes and cleanup. A later real two-host,
   multi-hour soak remains the broader F04/V01 stability gate.

Send unique foreground markers once at a fixed cadence, record send admission to
observed viewport/AX marker latency, failures and p50/p95/max. Record scheduled
main-actor observer lateness separately. Neither bundled CLI round-trip nor an AX
text observation is physical key-to-display latency; retain the physical/native gate.
Collect app CPU, RSS, physical footprint when available, engine page/counter values,
Rust frame/effect/queue peaks and process cleanup/reclaimed-resource observations.
Missing attribution/metrics are unavailable, never zero. Observe only the selected
diagnostic process; do not profile or signal the user's running cmux.

Freeze a versioned numeric budget record from the repeated baseline **before** the
candidate comparison: absolute CPU/RSS/footprint and p95 marker/main-actor thresholds,
output fairness, maximum queue/engine memory, and cleanup deadlines, with measurement
variance and the usability rationale. If the baseline itself hangs, preserve that
reproducer and classify it as a failed baseline rather than deriving a permissive
budget from the hang. This design invents no accepted thresholds from the existing
two startup samples or isolated build timings. Candidate migration is blocked until
those numeric budgets exist and the candidate passes them on named hardware.

The final decision combines compatibility/native fidelity, resolved memory/history
policy, stable repeated workloads, actual distribution notices and a reversible
engine selection for **new** sessions. Never swap a live terminal's engine or replay
its history/input on fallback. Keep F04/T01/V01 open until their whole applicable
contracts, including later graphics/selection/accessibility work, are verified.
