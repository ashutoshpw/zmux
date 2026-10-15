# F04 terminal engine evaluation and isolated feasibility

Updated: 2026-10-15. F04/T01 remain open. No production dependency or engine changed.

## Decision supported by this evidence

Keep the current production adapter while evaluating a pinned development
`libghostty-vt` behind Rust ownership. An isolated Linux build and C/Rust fixture now
establish that its C render API can supply owned text snapshots and preserve wrapped
text across resizing. They do not establish a shipping adapter, Mac behavior,
performance, graphics support in Zmux, or a complete license/notice closure.

`alacritty_terminal` is a credible permissive Rust fallback for reflow, terminal
queries and search. It was source-reviewed, not compiled or executed in this pass.
Choosing it would still leave image protocols and substantial native input/rendering
work. Neither language preference nor the small fixture chooses a production engine.

## Pinned upstream sources and licenses

Only public upstream projects were inspected. No cmux GPL/BUSL implementation,
bindings, tests, assets or build artifacts were imported or translated.

| Candidate | Exact source examined | License and packaging boundary |
| --- | --- | --- |
| Current `vt100` | `0.16.2`; crate VCS commit `eb66ffaf7d771c13303ef73b29f6f2a56fdacecf`; crate SHA-256 `054ff75fb8fa83e609e685106df4faeffdf3a735d3c74ebce97ec557d5d36fd9` | MIT. Existing `THIRD_PARTY_NOTICES.md` covers this pinned dependency and `vte 0.15.0` (MIT OR Apache-2.0). |
| Ghostty release | `v1.3.1`, commit `332b2aefc6e72d363aa93ab6ecfc86eeeeb5ed28` | Upstream MIT. The public C header exposes key encoding, OSC/SGR parsers and paste utilities; it does **not** expose the complete terminal/render C interface used below. This is a C integration distinction, not a claim that the Ghostty application lacks terminal state or reflow. |
| Ghostty development | `1.3.2-dev`, commit `22fa134a07f5749edd9e92c6bc17d8aaf61843be` | Upstream MIT. `include/ghostty/vt.h` explicitly warns that the incomplete API is unstable and will change. Full terminal, render, input and graphics interfaces exist at this exact pin. |
| `alacritty_terminal` | `0.26.0`; crate VCS commit `94e7c8874e526b1e67b349d9ba30ddf81669119e`; downloaded crate SHA-256 `bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59` | Apache-2.0, not MIT-only. Compatible with an MIT application when the dependency retains its own license/notices and applicable distribution obligations. Requires Rust 1.85.0; local Rust is 1.95.0. |

Pinned primary references:

- [vt100 license](https://github.com/doy/vt100-rust/blob/eb66ffaf7d771c13303ef73b29f6f2a56fdacecf/LICENSE),
  [resize implementation](https://github.com/doy/vt100-rust/blob/eb66ffaf7d771c13303ef73b29f6f2a56fdacecf/src/grid.rs),
  [callbacks](https://github.com/doy/vt100-rust/blob/eb66ffaf7d771c13303ef73b29f6f2a56fdacecf/src/callbacks.rs).
- [Ghostty release C API](https://github.com/ghostty-org/ghostty/blob/332b2aefc6e72d363aa93ab6ecfc86eeeeb5ed28/include/ghostty/vt.h),
  [development license](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/LICENSE),
  [development C API warning](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt.h).
- Ghostty development [terminal/effects](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt/terminal.h),
  [render state and borrowed lifetimes](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt/render.h),
  [Unicode rules](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt/unicode.h),
  [graphics](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt/kitty_graphics.h),
  [feature gates](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/build_options.zig),
  [toolchain/dependency pins](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/build.zig.zon).
- Alacritty [crate manifest](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/Cargo.toml),
  [license](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/LICENSE-APACHE),
  [terminal owner](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/src/term/mod.rs),
  [reflow](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/src/grid/resize.rs),
  [host effects](https://github.com/alacritty/alacritty/blob/94e7c8874e526b1e67b349d9ba30ddf81669119e/alacritty_terminal/src/event.rs).

Root licenses establish candidates for evaluation, not approval to ship an arbitrary
upstream build. The isolated Ghostty build fetched pinned packages including uucode
(MIT), translate-c (MIT), aro (MIT plus Unicode License V3 data), and zlib source.
The standalone Zig distribution has an MIT root license and separately licensed
bundled components. Record the actual linked dependency/Unicode/compiler-runtime
notice closure before distribution; the full application dependency list is not a
substitute. No third-party files were added to Zmux by this evaluation.

## Current owner and gaps

The production baseline inspected is Zmux `13504c31558e62bdc85868917b4e5bc3c5a333a3`.
`crates/terminal-adapter/src/lib.rs` SHA-256 is
`efb518bb37153b408f2a3a88f87075e4716f2c7a6e332b89a0363a7808356a84`.

`session-runtime` owns parsing, PTY I/O, resize and input ordering on its worker.
`terminal-adapter` exports immutable runs; `TerminalView` owns AppKit drawing,
selection and text composition. Preserve that boundary with any replacement.

| Area | Current implementation/evidence | Candidate source support and remaining integration |
| --- | --- | --- |
| Basic VT and effects | Fragmented UTF-8/CSI, alternate screen, cursor modes, bounded history, styles, title, cwd and notifications have portable fixtures. The metadata callback handles bell/title/OSC; no query-response pipeline is wired. | Ghostty effects include bounded-copyable PTY replies/title/cwd/notifications; defaults ignore effects until registered. Alacritty emits `Event::PtyWrite`, clipboard, title and query events. Both need explicit host policy and ordered response admission. |
| Keyboard/mouse | Legacy arrows and T01a function/navigation keys are encoded natively; wheel encoding reads live parser modes on the worker. Extended keyboard, pointer buttons/motion and focus reporting remain incomplete. | Ghostty supplies C encoders configurable from terminal modes, including Kitty keyboard. Alacritty tracks Kitty keyboard modes, but its terminal crate is not the complete application key encoder. Neither candidate replaces Command shortcuts, physical layout handling or IME. |
| Unicode | Current cells preserve combining characters and wide CJK in small fixtures. They do not provide grapheme-cluster width behavior for the tested mode-2027 emoji sequences. | Ghostty exposes matching codepoint/grapheme utilities and mode-2027 rules. Alacritty cells retain zero-width characters with `unicode-width`; this review establishes no complete ZWJ/VS16 equivalence. AppKit glyph placement, fallback and selection still need native evidence. |
| Reflow | Current `vt100::Grid::set_size` resizes/truncates rows and clears wrap flags on width changes. The executed probe loses text when narrowed. | Ghostty documents primary-screen reflow and no alternate-screen reflow. Alacritty implements grid reflow with primary/alternate distinction. Only the small Ghostty primary-screen case was executed here. |
| Styles/links/search | Current native export covers bold/italic/underline/inverse/indexed/RGB; it has no image-placement or hyperlink export. Selection is native and terminal find remains open. | Ghostty exports styles, hyperlinks, selection and search; Alacritty has cell hyperlinks, selection and regex search. Current run/selection contracts need extensions before these features reach users. |
| Graphics | No terminal image-protocol renderer. The 16-KiB control-string guard explicitly fails long payloads. Clipboard-image upload is a separate feature. | Ghostty development has Kitty image storage/placement APIs, requiring bounded storage, decoding policy and a renderer; those features were disabled in the experiment. No Kitty/Sixel graphics implementation was found in the inspected Alacritty terminal crate. No Sixel or image-rendering acceptance is claimed. |
| Resource/recovery | Geometry is bounded to 120×240, history to 1,000 lines, control strings to 16 KiB. Oversized control strings fail the session parser; richer recovery remains open. | Ghostty history byte/line limits are estimates at page granularity, **not** exact substitutes for current hard limits. Its VT write API returns void; a separate sticky processing-error flag reports some failures, while configured limits/effect failures need separate handling. Alacritty configured history does not itself bound every allocation or host event queue. |

## Executed Linux comparison

The current adapter was built with `cargo build -p zmux-terminal-adapter --locked`.
An original scratch Rust driver linked it without modifying workspace manifests.
The Ghostty driver used an original C shim and a Rust RAII owner:

- Construct and mutate opaque C terminal/render handles on one thread. Rust owner
  uses `PhantomData<Rc<()>>` rather than asserting `Send`/`Sync`.
- Read render rows/cells and copy full UTF-8 cell strings into a caller-owned
  512-KiB snapshot buffer. A cell exceeding 256 UTF-8 bytes fails explicitly.
  Borrowed C iterators and strings never escape the snapshot call.
- Capture query replies in a 4-KiB callback buffer; never block or recursively feed
  the terminal from a callback. Clipboard/network/native effects stay disabled.
- Preserve 120×240 geometry and 16-KiB feed-chunk caps. Set history line/byte
  preferences, but rely on the fixture's finite input and an external process cap
  for this experiment; page-granular preferences are not hard memory guarantees.
- Run with 512 MiB address-space, three CPU seconds and ten wall seconds as fixture
  containment limits. These are **not proposed product performance budgets**.

Independent inputs and observed outputs:

| Fixture | Current adapter | Ghostty development C/Rust driver |
| --- | --- | --- |
| Feed `abcdefghijKLMNOPQRST` into 4×10, narrow to 4×5 | `abcde`, `KLMNO`, two empty rows | `abcde`, `fghij`, `KLMNO`, `PQRST` |
| Widen back to 4×10 | Lost halves do not return | Original two complete rows return |
| Byte-fragmented `e` + U+0301 + `x` | Cursor column 2 | Cursor column 2 |
| Byte-fragmented `界x` | Cursor column 3 | Cursor column 3 |
| Request mode 2027, then `👩` + ZWJ + `💻x` | Cursor column 5 | Cursor column 3 |
| Request mode 2027, then `❤` + VS16 + `x` | Cursor column 2 | Cursor column 3 |
| `CSI 6 n` on an empty terminal | No production response sink exists; not executed as a PTY query | Copied callback bytes exactly `1b5b313b3152` (`ESC[1;1R`) |
| Retain Rust snapshot across two resizes; create/free 128 terminals | Not a new lifecycle comparison | Assertions pass; driver exits zero |

These are targeted parser/API observations. Wrapped cursor coordinates differ:
vt100 exports a pending-wrap column at the width; Ghostty exports the last cell.
An adapter must normalize that contract rather than compare raw coordinates.
The scratch Ghostty cell dump includes blank wide-continuation slots; it is not a
finished native run exporter. Both engines' CJK cursor result is recorded separately
from that debug-text formatting. No clipboard, image, PTY backpressure, OOM-injection,
ASan, AppKit rendering or native accessibility test ran in this experiment.

## Build receipt and reproduction boundary

No Zig executable was initially on PATH. The isolated toolchain was downloaded to
`/tmp/zmux-f04-evaluation-20261015`, without installing or changing a global toolchain.
The official [Zig download index](https://ziglang.org/download/index.json) supplied
the checksum for `zig-x86_64-linux-0.16.0.tar.xz`:
`70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00`.
Downloaded bytes matched it. Ghostty development requires Zig 0.16.0; the examined
release requires 0.15.2 and has the narrower C interface described above.

The source archive came from
`https://codeload.github.com/ghostty-org/ghostty/tar.gz/22fa134a07f5749edd9e92c6bc17d8aaf61843be`;
its observed SHA-256 is
`a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`.
This archive hash is an observed receipt, not a separate publisher signature.

The first build succeeded in 73.68 seconds, under a 600-second deadline and two
build jobs. This is one Linux build-cost observation, not a runtime or Mac result.
Stdout/stderr were drained with at most 1 MiB retained; this build emitted no log
bytes. Scratch-local Zig caches avoided changes to the workspace or shared caches.

```sh
zig build -Demit-lib-vt=true -Demit-xcframework=false -Dapp-runtime=none \
  -Doptimize=ReleaseSafe -Dsimd=false \
  -Dvt-features=-all,+render-state,+grid-introspection,+formatter,+input-encode \
  -j2 --prefix /tmp/zmux-f04-evaluation-20261015/ghostty-install
```

The independent C shim compiled with `cc -std=c11 -Werror -DGHOSTTY_STATIC` and
the upstream include directory. Rust 1.95.0 linked that object and the installed
`libghostty-vt.a`; the bounded driver exited zero on its first run.

Local evidence is preserved in `/tmp/zmux-f04-evaluation-20261015`:

- `ghostty-probe.c`, SHA-256 `30863ca5ffe286011858aa0e85aa9304438e3513c2a01fa2c363b9e5d41a0fe8`.
- `ghostty-probe.rs`, SHA-256 `931782db53025c58acfe89126049842afc9922e10351747a157dccbd9bae28f2`.
- `ghostty-probe-result.json`, `ghostty-build-receipt.json`, download/toolchain
  receipts, and `current-adapter-probe.rs` / `current-adapter-probe.txt`.
- `feasibility-evidence.tar.gz` contains only the original drivers and receipts,
  SHA-256 `53c538e3e6453d17efcec8f6eeb276604a814f1038216e30836df30e617de65d`.

These scratch paths are local evidence. The original drivers are now also preserved
in [tools/terminal-feasibility](../../../../../tools/terminal-feasibility/README.md),
with a separate manually dispatched workflow. It downloads exact upstream pins at
run time rather than vendoring another application's implementation into Zmux.

The first clean-cache run of this reproducible runner verified both archive hashes,
then failed while Zig fetched pinned theme/zlib packages with `NameServerFailure`.
A bounded follow-up check resolved DNS but received HTTP 403 for those exact HTTPS
URLs. No fresh-network retry or URL fallback was used. The failed receipt remains
at `/tmp/zmux-terminal-feasibility-repro-linux/receipt.json`.

A separate run with the explicit task-owned, Zig-content-hash-verified package cache
completed at `/tmp/zmux-terminal-feasibility-repro-verified-cache/receipt.json`.
It rebuilt the library in 61.632 seconds, compiled the original C/Rust drivers and
passed both fixture executions. Its receipt labels `explicit_existing_cache` and
records driver SHA-256 values. This validates runner mechanics and the Linux API
observations; it does not clear the fresh dependency-download gate. CI uses a fresh
temporary dependency cache and has no fallback.

Portable checks also passed Python syntax, pin JSON, workflow YAML parsing, Rust
formatting and scoped diff checks. A later lifecycle review found that stdout EOF
could end deadline enforcement and that a successful leader could leave a child
running after it closed stdout. The runner now enforces the deadline independently
of EOF and stops its owned subprocess group on every phase completion. The baseline
Cargo build also explicitly uses `--jobs 2`, matching the receipt's build-job limit.

The real-process regression command is:

```sh
python3 -B -m unittest discover -s tools/terminal-feasibility -p test_runner.py -v
```

Before the repair, four tests ran with two failures: the EOF case took 5.028 seconds
and reported `timed_out: false`, and a successful leader left its child running.
The red log is `/tmp/zmux-feasibility-runner-red.log`. After repair all four tests
passed in 1.259 seconds, recorded at `/tmp/zmux-feasibility-runner-green.log`.
The cases cover EOF while the leader runs, a successful leader with a closed-stdout
child, an exited leader with a stdout-holding child, and draining 2 MiB while
retaining 1 MiB. Both workflow jobs run them before building upstream code.
The earlier complete Linux receipt predates this repair; the focused regression
run validates the lifecycle change without another upstream build. No macOS build,
universal slice or hosted workflow execution is claimed here.

## Next bounded experiment and decision gates

The smallest useful next test is the same Rust-owned C render-snapshot/reflow fixture
on Linux and macOS, with native execution identified by architecture and source SHA.
Compile both Mac architectures separately before any universal-library claim; an
arm64 execution does not establish x86_64 runtime behavior. Keep this workflow manual
and separate from application builds/dependencies.

Then extend one experimental adapter to export the existing `Viewport` contract,
normalizing pending wrap, wide continuations, grapheme strings, styles and modes.
Use a real local PTY for one cursor-query round trip while preserving ordered writes
and bounded overload handling. Explicitly verify unsupported/control-string recovery,
allocator failure, callback overflow and destructor order before admitting the adapter
to production. Keep graphics and filesystem/clipboard side effects disabled initially.

Named-Mac workload measurements remain separate: idle/output/hidden tabs/browser/SSH
stall phases, CPU/RSS and send-once marker latency, followed by explicit numeric budgets.
The external sampler's mixed packaged journey is not those isolated measurements.
Complete the actual notice closure, Mac packaging, native selection/IME/rendering and
resource gates before selecting an engine or increasing current capacity limits.
