# Isolated terminal feasibility

This manual experiment compares the current Zmux parser with a pinned development
`libghostty-vt`. It changes no application dependency, Cargo lockfile, native app,
control socket or running terminal. Read [provenance](NOTICE.md) and the
[F04 evaluation](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-engine-evaluation.md).

The opt-in [metadata extension](#opt-in-metadata-extension) adds a separately
hashed upstream patch after the unchanged resource patch. It remains isolated
from the application engine.

Requirements: Python 3.12+, Rust 1.95.0, a C compiler and network access to the
pinned public archives/dependencies. Mac universal compilation also needs Xcode
command-line tools, a macOS SDK, `lipo`, and both Rust Apple standard libraries.

```sh
python3 tools/terminal-feasibility/run.py --output /tmp/zmux-terminal-feasibility-results
# On a Mac, compile both architectures; execute only the host architecture:
python3 tools/terminal-feasibility/run.py --macos-universal \
  --output /tmp/zmux-terminal-feasibility-macos-results
```

The separate `Terminal engine feasibility` GitHub workflow uses `workflow_dispatch`
only. Its Linux job executes x86_64. Its Mac job compiles arm64 and x86_64 C shims,
Rust owners and libraries, and verifies universal library/executable slices with
`lipo`. Only the runner's architecture is executed; the other target is explicitly
`not_run_cross_architecture`. No Rosetta installation or cross-runtime claim occurs.

`receipt.json` records exact pins, source SHA, host, compiler version, phase results,
and observation output. A complete receipt means the requested compilation and
native fixture assertions completed; it does not mean product parity or acceptable
performance. Logs are drained while retaining at most 1 MiB per phase. Download
archives cap at 128 MiB, extraction at 1 GiB, and download reads use a 20-second
socket timeout plus a 120-second transfer window. Each build has a 600-second
deadline and two jobs. No retry silently changes a pin or substitutes another engine.
Use a fresh output directory so retained logs belong to one attempt. By default,
the Zig dependency cache is also fresh. An explicit `--dependency-cache PATH` may
reuse a task-owned content-addressed cache for local diagnosis; the receipt labels
that choice. Zig still checks its pinned package hashes. CI does not use this option,
and a cache-based pass does not establish fresh dependency-download availability.

Each fixture has a ten-second wall deadline. The execution helper requests three
CPU seconds, a 512-MiB address-space limit and disabled core dumps. It reports each
OS limit as configured or unavailable; macOS may reject or implement address-space
limits differently. These are experiment containment settings, not measured RSS,
physical footprint or product budgets. Every phase stops its newly created
subprocess group on completion or timeout, including descendants that closed
stdout. Filesystem cleanup still depends on ordinary OS I/O.

`probe.c` owns C handles, copies cell UTF-8 into caller buffers, and captures query
bytes without blocking or reentering the engine. The shared `owner.rs` uses RAII and
a non-Send/non-Sync owner. The original `probe.rs` independently asserts:

- 4×10 text survives a 4×5 resize and widening, with exact expected rows;
- an owned Rust snapshot survives later engine mutations;
- an empty-terminal cursor query returns exactly `ESC[1;1R`;
- 128 create/feed/snapshot/drop iterations finish correctly.

Combining/CJK/ZWJ/VS16 cursor observations are printed separately. The debug cell
dump includes blank continuation cells and is not a native run exporter. Limits
are 120×240 geometry, 16-KiB feed calls, a 512-KiB copied snapshot, 256 UTF-8 bytes
per cell and 4-KiB callback output; oversized output fails, rather than truncating
borrowed memory. Ghostty's history preferences are page-granular estimates.

`current-adapter.rs` records the same resize/Unicode inputs against the existing
adapter built with `--locked` into scratch output. It records known gaps; a zero
exit is not a claim that it implements reflow or grapheme-width parity. Assertions
derive from public engine behavior without a production test-only API.

`compatibility.rs` is a separate host-only fixture linked against the current
public `Viewport`, `Run`, `Style` and `Color` types. `frame.h` defines an original
bounded C interchange, checked for matching C/Rust struct sizes. One render-state
update supplies cell text/styles, logical pending-wrap cursor, mode flags, history
count and distance from the live viewport. The shim copies bytes; no borrowed row,
cell, string or handle enters the published Rust frame. Wide tails are omitted,
wide heads retain width two, and soft-wrap spacer heads retain their grid slot.

Its independent assertions cover fragmented CJK/combining and mode-2027 graphemes,
soft-wrap spacer placement, owned-frame survival after mutation/destruction,
default/indexed/RGB/bold/italic/underline/inverse styles, erased backgrounds,
pending-wrap and mode transitions, scroll direction/clamping, alternate-screen
isolation and exported reflow. Extra style attributes, hyperlinks, palette/default
color overrides and nonempty title/cwd metadata explicitly reject the compatibility
frame instead of silently losing data. Small buffers, ABI-size mismatches and a
cell exceeding the UTF-8 budget fail explicitly.

The pinned engine itself retains at most 64 suffix codepoints per cell, ignoring
further attached codepoints. The overflow fixture uses a base plus 64 four-byte
combining marks (257 UTF-8 bytes), so it exercises the export byte cap before that
separate engine retention limit. This is not unlimited grapheme fidelity.

The fixture does not implement title/cwd/bell/OSC notification export, key/mouse
encoding, selection, cursor shape or graphics. History reports actual retained rows
and remains page-granular; this does not preserve the current exact retention cap.
The native page allocator bypass described in the F04 design remains a migration
blocker. Compatibility is compiled/executed only for the host architecture; the
existing universal-slice claim covers the original raw-API fixture. The new
compatibility stage has its own receipt entry and source/header/owner hashes.

The owner-level regression risk is a wrong row/width/query contract or escaped
borrowed memory. Existing native key and parser fixtures do not cross this C ABI.
The expected rows and query bytes are independent constants, not output generated
by the engine under test. No benchmark, graphics, OOM/ASan or AppKit acceptance is
claimed. Do not select an engine or expand production limits from this experiment.

Local verification on 2026-10-15: the clean-cache runner verified direct archive
checksums, then failed on upstream dependency fetches (`NameServerFailure`; a
follow-up HTTPS check returned 403). The separate explicit-cache run completed all
Linux compile/fixture stages. At that point fresh-download and all Mac/hosted gates
remained open. Python syntax, pin JSON, workflow YAML parsing, Rust formatting and diff
checks pass. Four real-process regressions verify the 1-MiB retained-log cap,
deadlines after stdout EOF, cleanup of a descendant holding stdout after its
parent exits, and cleanup after a successful leader exits with a closed-stdout
descendant. Both workflow jobs run these checks before building upstream code:

```sh
python3 -B -m unittest discover -s tools/terminal-feasibility -p test_runner.py -v
```

Hosted workflow [38756429963](https://github.com/ashutoshpw/zmux/actions/runs/38756429963)
passes Linux x86_64 and macOS arm64 at `0a5d52e259b412b86399d1e87a79a9c437b33412`,
with fresh dependency caches. It verifies both Mac architecture slices in the
library and fixture executable; only arm64 executes on the Mac host. Both host
fixtures and baseline observations pass. The first Mac run exposed a `lipo`
argument-order bug, repaired before this rerun. Mac address-space containment was
unavailable. See the F04 evaluation for exact receipts, red/green logs and remaining
production/native/performance gates.

Hosted exporter workflow [38761104404](https://github.com/ashutoshpw/zmux/actions/runs/38761104404)
completed at `3ac6fa410b693c1978dd61d9649295b6bc93326f` with fresh dependency
caches. The compatibility exporter executes successfully on Linux x86_64 and
macOS 15.7.9 arm64. Mac x86_64 remains compilation-only; universal verification
covers the original raw fixture/library. Mac `RLIMIT_AS` is unavailable. This is
**pre-resource-patch** evidence and does not validate the allocator extension.

## Opt-in allocation and exact-history experiment

```sh
python3 tools/terminal-feasibility/run.py --resource-experiment \
  --output /tmp/zmux-terminal-resource-results
```

Read the frozen [resource contract](RESOURCE_CONTRACT.md) before interpreting this
mode. The runner applies `resource.patch` only to its fresh, checksum-verified
upstream extraction, after `git apply --check` and equality-checking the retained
upstream MIT license. The exact upstream pin, application engine/dependencies and
production limits remain unchanged. Default runs do not apply the patch.

The original MIT patch supplies general and dedicated page allocators before
construction, preserves page ownership through clone/reflow/alternate replacement,
and prunes exact oldest history prefixes at Screen growth/resize completion. The
fixed-ledger shim charges page-rounded **reserved mappings**, including preheated,
free-pool and old/new transient allocations. Its 16-MiB fixture quota is an experiment
setting. It is not total app/helper RSS, a production budget or evidence of measured
Mac memory/latency. Fixed host owner/ledger storage, 512-KiB copied frames and other
exclusions are recorded separately in the contract.

The host-only resource fixture checks exact zero/one/1,000-row limits and oldest-row
removal at the next two rows, byte-fragmented feed, clone, resize/reflow, alternate
creation/resizing/reset, constructor failure positions, aggregate/general/page
quota denial, sticky refusal and zero live mappings on cleanup. The two unchanged
original-constructor controls must fail the intended assertions (exit 101); their
phase records say `expected_failure: true`. A complete resource receipt therefore
includes those two intentional red phases, while the repaired resource fixture and
all original raw/exporter fixtures must pass. Resource fixture/header/shim/patch,
contract and retained-license hashes are included in `harness_sha256`; patch base
and hash also have a separate `upstream_patch` entry.

Compression is not called, snapshot import is unsupported, and optional
formatter/search/selection APIs and effects need separate acceptance. No total
app memory bound, full allocation closure for every upstream export, physical
footprint, Mac resource execution or production migration is claimed by this mode.

Alternate replacement after a failed in-place resize and the partial top-region
scroll rotation branch have allocator/pruning propagation reviewed in source;
they are not independently exercised by the current resource fixtures.


The reviewed local Linux resource run is
`/tmp/zmux-f04-resource-repro-final-reviewed-20261015/receipt.json` (SHA-256
`4c88318afa08e9afaf59e3363be3c20457897584f75672d7400ad4188a526b46`).
All 14 input hashes matched that historical reviewed snapshot; every normal phase passed and the two original
constructor controls fail their intended assertions. It uses an explicitly labeled
existing dependency cache, a private Cargo target, and fresh verified upstream/tool
archives. Patch SHA-256:
`0ed579ca59f7e5d5f1ca695c8bc481bbfd3b1cad9832e215c9475178e6328f02`.
The six operation-denial paths include ordinary history page growth separately from
lazy alternate initialization. Sticky denial takes effect for subsequent calls
after the current at-most-4,096-byte feed returns; there is no consumed-byte or
last-good-generation receipt. Hosted/Mac resource execution remains a separate gate.


## Opt-in callback effects experiment

```sh
python3 tools/terminal-feasibility/run.py --effects-experiment \
  --output /tmp/zmux-terminal-effects-results
```

This mode adds original `effects.c`/`effects.h`/`effects.rs`, using only public
upstream callbacks and the existing probe wrapper. It can run without the resource
patch or combine with `--resource-experiment`; neither flag changes the application.
A fixed 1,024-event batch preserves BEL/reply/RESET order, and reply bytes cap at
4,096. The single-threaded owner requires successful draining before more input.
Insufficient drain capacity does not drain or partially copy the batch, and overflow is
visible and sticky after the current call returns. The Rust owner publishes a new
owned frame/effect generation only after all checks pass, retaining the last good
generation on failure. BEL reuses the current public `TerminalNotice` type with a
32-notice ring and explicit oldest-notice eviction count. RESET is a control event;
it does not create a notice or restart sequence identity.

The real-engine fixture exercises callback order and owned lifetime, fragmented
queries, exact byte/event limits and overflows, pending-batch/oversized-input
refusal before mutation, small drain buffers, failed snapshot publication and 128
owner lifecycles. Two temporary **original shim** mutations must fail their intended
assertions: silent reply loss and a reset mislabeled as BEL. Their source hashes and
intentional exit-101 phase records are separate from the successful normal fixture.
No upstream code is changed for these controls. Normal effect inputs are hashed in
the receipt. Execution is host-only; Mac universal slices still cover the original
raw library/fixture rather than an effects executable.

The C context has a separately stated fixed storage bound; Rust frame/batch copies
are bounded independently. This is not a total app memory or host-allocation-failure
proof. Replies are captured only: there is no real PTY round trip, callback into UI,
clipboard, native notification dispatch or app integration. Title/cwd and OSC9/777
fidelity remain open because the pin silently truncates raw metadata and its generic
notification callback omits protocol provenance. See the
[effect slice design](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-effects-slice.md).

Combined resource/effects runs use separate terminal owners. `effects_new` uses the
unchanged public default constructor with its default allocator; it does not use the
resource fixture's mapping quota. The combined run therefore verifies both isolated
contracts, not a single integrated quota/effect/publication owner. That composition
and allocation failure during callback publication remain separate acceptance gates.

The shim's engine-error check is the upstream informational `VT_PROCESSING_ERROR`
bit. That bit excludes gracefully handled failures/configured limits and errors
limited to external effects or query responses. The fixture's explicit host
reply/event/snapshot failures are covered; it does not detect every possible engine
allocation failure or prove that no upstream query was silently lost. Controlled
allocation denial with this publication owner is still an independent gate.


The reviewed local combined run is
`/tmp/zmux-f04-effects-repro-reviewed-20261015/receipt.json` (SHA-256
`5f9f75280eeed19843798535b8f0dade799cf802fabbcfe906b07bb59a2cac1e`).
All 17 harness hashes matched that historical reviewed snapshot. Every ordinary phase passed, including exact
4,096-byte reply success; four explicitly labeled controls intentionally exit 101
(two resource and two effects). The effects fixture passes in 0.332 seconds and
records 24,632 fixed C host bytes on Linux, separately from Rust frame/batch storage.
It uses the labeled explicit dependency cache and does not establish fresh dependency
availability, Mac effects execution, production integration or a performance budget.

## Integrated resource/callback/publication owner

```sh
python3 tools/terminal-feasibility/run.py --resource-experiment \
  --integrated-experiment --output /tmp/zmux-terminal-integrated-results
```

This adds a separate real single-terminal fixture. The existing resource and effects
fixtures remain available; `--effects-experiment` can also run in the same command.
Integrated mode requires the resource patch, whose source and contract are unchanged.
`collector.c`/`collector.h` now share the original fixed callback implementation;
the standalone effects owner continues using its default constructor. The integrated
owner constructs one Resource terminal with the bounded constructor, its general/page
allocators and the same render helpers, and binds that collector before first input.
No opaque engine layout is cast and no second terminal/parser is created in this owner.

Successful calls stage an operation even with no callbacks. Export checks render
results and allocation counters again, then seals all further engine/config access
until exact-ID commit. Commit retires the batch without allocation. Rust validates
candidate UTF-8/event ranges before commit and swaps two preallocated slots to publish;
failed allocation, callback overflow or export retains the previous good slot and
refuses subsequent input. Scalar quota tightening is permitted before successful
export, with validation before assignment; history reads require idle. A valid export
blocks subsequent export/quota/history/engine calls until commit. Diagnostic stats,
explicit abort and teardown remain available.

The fixed C owner/collector/Resource/Probe allowance is subtracted from the requested
C managed budget before constructing the engine. The fixture reports fixed bytes,
reserved engine mappings and their current/peak sum. Rust separately reserves two
bounded publication slots before engine construction; no per-operation buffer growth
or Published/String clone is needed. On this Linux host the allowance is 155,952 C
bytes, 1,081,344 Rust backing capacity bytes and 800 inline Rust owner bytes. Heap/kernel
overhead, stacks, libraries and caller-made snapshot copies are separate; none of
these sums is total app RSS or physical footprint. Host budget/allocation and nullable
C constructor errors are returned before a usable owner is published.

The fixture composes exact 1/1,000-row history, callbacks and reflow in this same owner;
checks callback-before-quota-denial batches, every observed constructor allocation
position, quota/overflow/snapshot failure cleanup, sealed and stale commits, and 128
lifecycles. It forces an actual general allocation denial during render export after
a healthy mutation, rather than assuming resize allocates. A temporary original-shim
control omits that render result/quota fence and must fail the named last-good
publication assertion. Earlier separate negative controls also remain required.
Title/cwd/OSC notification provenance, optional compression/features, real PTY delivery,
AppKit fidelity and named-Mac workload budgets remain independent acceptance gates.

Read the [integrated owner design](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-integrated-owner.md)
for ownership, accounting and review boundaries. New integrated execution is host-only;
Mac universal compilation still names the original raw fixture/library only.

Final local integrated receipt:
`/tmp/zmux-f04-integrated-repro-final-20261015/receipt.json`, SHA-256
`0c0bd1483598c8ebc812ecaa68b8a027311076b1679a2798e15e1693ba219a30`.
All 23 frozen input hashes match. Every ordinary stage passes and five labeled
controls intentionally exit 101. The integrated fixture completes in 1.078 seconds
on Linux; independent rerun and hosted integrated execution remain separate gates.

## Opt-in metadata extension

```sh
python3 tools/terminal-feasibility/run.py --resource-experiment \
  --effects-experiment --integrated-experiment --metadata-experiment \
  --output /tmp/zmux-terminal-metadata-results
```

Read the frozen [metadata contract](METADATA_CONTRACT.md) and its linked design/
evidence reference. This mode requires resource and integrated experiments and
applies original `metadata.patch` only to the verified temporary source, after
the unchanged `resource.patch`; both patch hashes are separate in the receipt.
Default constructors and application dependencies remain unchanged.

The versioned constructor captures selected OSC metadata inside the existing
parser with source, ConEmu subcommand, completeness and failure status, before
upstream clipping/merging. A fixed sidecar adds descriptors/raw bytes to the same
Integrated ordered callback and publication owner; Resource alone owns the terminal
and allocators. A generic constructor factory charges sidecar storage once. No
second parser/terminal, callback allocation or alternative failure owner is added.

Full semicolon titles and notification body remainders, CAN/SUB suppression and
checked notice sequence exhaustion deliberately repair current-adapter defects.
Host filtering, cwd rules and RIS metadata retention are explicit migration
compatibility policy, not established cmux parity. Title reporting remains disabled
and attempts to enable it on this experimental constructor reject explicitly.

The real boundary fixture covers fragmented source/termination, exact title/cwd/
notice limits, invalid UTF-8/bidi filtering, source/subcommand ordering with BEL/
reply/RESET, capture/arena/event/sequence failures, actual allocator/render denial,
last-good publication, constructor denials, exact history/reflow/alternate/clone
and 128 zero-mapping teardowns. Two temporary original C mutations must fail named
source-provenance and arena-publication assertions. Prior resource/effect/integrated
controls remain required. Metadata execution is host-only; universal slices still
cover the original raw fixture/library. Hosted Mac, real PTY delivery, app/native
integration and measured workload budgets remain separate gates.
