# Isolated terminal feasibility

This manual experiment compares the current Zmux parser with a pinned development
`libghostty-vt`. It changes no application dependency, Cargo lockfile, native app,
control socket or running terminal. Read [provenance](NOTICE.md) and the
[F04 evaluation](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-engine-evaluation.md).

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
