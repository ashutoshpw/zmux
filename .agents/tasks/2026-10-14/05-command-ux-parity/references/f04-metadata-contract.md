# F04 metadata extension contract and evidence

2026-10-15. Source investigation, approved migration policy and original isolated
implementation. The application engine remains unchanged. The original
`resource.patch` is unchanged; the accepted [single owner](f04-integrated-owner.md)
harness gains a generic constructor factory, charged sidecar and guarded event
append hook. All earlier fixtures and controls remain required.

## Decision

Public title/PWD getters and the generic notification callback cannot preserve
the current adapter's metadata behavior. Add an **opt-in, creation-time metadata
capture policy inside the existing upstream parser**, with source and completeness
carried through its command/action/handler/C boundaries. Do not infer source from
notification title, decode a clipped getter, or introduce a second VT parser.
Keep defaults unchanged for other embedders. The isolated implementation follows
the accepted migration rules and explicit compatibility boundaries below.

The host owns normalized title/cwd/notices independently of the engine getters.
Use the integrated collector and existing failure/publication fence. Metadata
cannot make a failed operation publish its earlier valid frame or callbacks.

## Pinned source evidence

Ghostty `22fa134a07f5749edd9e92c6bc17d8aaf61843be`, MIT, development 1.3.2;
archive SHA-256 `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`.
These are source/API observations at that pin, not claims about a future release:

| Boundary | Evidence and implication |
| --- | --- |
| [OSC parser](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/osc.zig#L361) | Fixed capture is 2,048 bytes. OSC 0/2/7/9/777/1337 use fixed capture. Known-command capture failure changes state to invalid; `end()` returns no command. The unknown-command truncation marker does not cover known malformed/oversized commands. |
| [Title parser](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/osc/parsers/change_window_title.zig) | A sentinel consumes one capture byte: 2,047 title bytes parse, 2,048 do not. OSC 0 versus 2 provenance is already lost in the resulting title command. |
| [OSC 9 parser](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/osc/parsers/osc9.zig) / [777 parser](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/osc/parsers/rxvt_extension.zig) | Both produce the same notification `{title, body}` command. Empty titles are valid in both. OSC 9 also recognizes ConEmu subcommands before notification fallback. |
| [Cwd commands](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/osc.zig#L64) | OSC 7, OSC 9;9 and OSC 1337 CurrentDir become the same `{value}` command. Source cannot be reconstructed from a bare-path/raw-URI getter. |
| [Stream](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/stream.zig#L2724) | Invalid UTF-8 titles are ignored before the terminal handler. Title/cwd/notification actions lack original protocol provenance. A C callback change alone is too late. |
| [Terminal handler](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/src/terminal/stream_terminal.zig#L1866) | Title clips to 1,024 bytes; raw cwd clips to 4,096 bytes. Title clipping can split UTF-8. Raising these handler caps alone does not repair the earlier parser drop. |
| [Public C API](https://github.com/ghostty-org/ghostty/blob/22fa134a07f5749edd9e92c6bc17d8aaf61843be/include/ghostty/vt/terminal.h) | Title/PWD getters are borrowed until mutation; notification fields are callback-duration borrowed bytes without source. PWD may be queried inside its callback; title documents querying after its callback. RESET fires after RIS clears title/cwd, without title/cwd changed callbacks. DECSTR retains metadata. Informational VT processing error excludes some graceful/configured-limit failures. |

## Local observed behavior

Temporary original probes: `/tmp/zmux-f04-metadata-observations-20261015/`.
Receipt `receipt.json` SHA-256:
`412d625e2fb11d203e512200fb0cd48f10a3d854ba866ab147ccf8e4cc2c2a8c`.
It hashes 20 byte transcripts, both original drivers, current adapter source and
Cargo lockfile, compiled rlib/library and relevant pinned source files. It records
commands, compiler versions and both bounded logs. Both processes exit zero under
ten-second deadlines. These are observations, not new regression assertions.

Current adapter was freshly built with `cargo build --locked -p
zmux-terminal-adapter --target-dir /tmp/zmux-f04-metadata-observations-20261015/cargo-target`;
the Rust driver links that private build. C uses public APIs and the already built
resource-patched library, SHA-256
`30119e1dba1993331e58d23f98d874e231f3cb476f38ab7cbc9c07fb5fb61708`.
The eight parser/stream/header files listed in the receipt match the original pin;
the resource patch changes allocation/history ownership, not these metadata paths.
No upstream/app source was modified for these probes. Host: Linux x86_64, GCC 11.4,
Rust 1.95. This establishes no Mac execution, resource limit or performance result.

Unless stated otherwise, seed is `OSC 2;seed BEL`,
`OSC 7;file://localhost/tmp/start BEL`, then BEL. OSC denotes `ESC ]`.
The adapter publishes decoded `/tmp/start`; Ghostty stores the 26-byte raw URI.

| Transcript after seed | Current adapter | Pinned Ghostty public API |
| --- | --- | --- |
| RIS (`ESC c`) | Keeps title `seed`, cwd `/tmp/start`, prior bell notice. | Clears title/cwd; one RESET; no extra title/cwd changed callbacks. External collector notices are outside its ownership. |
| DECSTR (`ESC [ ! p`) | Keeps metadata/notices. | Keeps title/cwd; no RESET. |
| Empty OSC 7 | Keeps previous cwd. | Clears raw cwd and fires PWD callback. |
| OSC 7 with `https://x/new` | Keeps previous cwd. | Stores the unsupported raw value. |
| OSC 7 with `file://localhost/a%QZ` | Publishes `/a%QZ`; malformed percent triplet remains literal. | Stores raw URI; supplies no decoding policy. |
| OSC 7 with `file://localhost/` + `%61` repeated 1,000 times | Publishes the exact 1,001-byte decoded path. | Parser drops it; old raw cwd remains; no callback/error bit. |
| OSC 2 with 2,047 ASCII `x` | Publishes 256 `x`. | Publishes 1,024 `x` after handler clipping. |
| OSC 2 with 2,048 ASCII `x` | Publishes 256 `x`. | Drops command; title remains `seed`, error bit false. |
| OSC 2 with U+202E repeated 400 times, then 256 ASCII `x` | Filters bidi controls, publishes 256 `x`. | Stores first 1,024 raw bytes, no `x`, ending in incomplete UTF-8 byte `e2`; error bit false. |
| OSC 2 with bytes `a ff b` | Publishes `a` + U+FFFD + `b`. | Ignores invalid UTF-8; keeps `seed`. |
| OSC 9;9;/other or OSC 1337;CurrentDir=/other | Keeps previous cwd. | Publishes indistinguishable bare `/other` cwd values. |

Additional fresh-terminal cases establish delimiter and termination differences:

| Transcript | Current adapter | Pinned Ghostty |
| --- | --- | --- |
| OSC 9;body BEL | One `osc9` notice, fixed title `Terminal notification`. | Generic notification with empty title, body `body`. |
| OSC 777;notify;;body BEL | No notice, because normalized title is empty. | Identical empty-title/body notification to the previous row. |
| OSC 9;body;extra BEL | No notice (extra field). | Generic body `body;extra`. |
| OSC 777;notify;title;body;extra BEL | No notice (extra field). | Generic body `body;extra`. |
| OSC 2;one;two BEL | No title change: vt100 matches exactly two fields. | Title `one;two`. |
| OSC 2;seed BEL, OSC 2;new CAN | Title becomes `new`. | CAN aborts the second command; title remains `seed`. |
| OSC 0;both BEL | Title `both`. | Title `both`; originating 0 versus 2 is not retained. |

`crates/terminal-adapter/src/lib.rs` is the current policy owner. Its 16-KiB
control-string guard runs before parsing the supplied chunk and latches failure;
oversized control input is not an allowed silently truncated metadata update.
The existing vt100 callback matching explains exact-field behavior. The temporary
probe does not yet characterize every C0/C1/ESC/SUB/fragmented termination case.

## Upstream extension design

The separately hashed, original MIT `metadata.patch` applies against the same pin
and resource patch, only in a fresh isolated extraction. Its experimental C namespace
is versioned; no compatibility claim with an unreleased upstream ABI is made. The
resource patch/contract hash remains unchanged; receipts record both patches.

1. Add creation-time metadata policy to the bounded constructor: disabled by default;
   finite capture cap and a host-managed dispatch mode. V1 fixes its selected
   command set to 0/2/7/9/777 rather than introducing a mutable mask.
   Set policy before the first byte; never change it mid-string. Invalid cap/version
   rejects construction before engine allocation. Use the existing general allocator;
   allocation failure never falls back silently to a smaller buffer.
2. Recognition in the sole OSC parser retains the original number and subcommand
   alongside payload bytes before title UTF-8 validation, title/cwd clipping, or
   OSC 9/777 and cwd-source merging. Carry that descriptor through Command, Action,
   handler and C callback. Baseline host-managed commands are 0/2/7/9/777. Distinguish
   `osc9_notification`, `osc9_current_dir` and `osc1337_current_dir` when recognized;
   the latter cwd sources remain ignored by the baseline host policy. Do not capture
   unrelated OSC 1337 image payloads as metadata.
3. Emit a sized descriptor with version, source/command/subcommand, termination
   (BEL/ST/CAN/SUB/other), status and borrowed payload pointer/length. Complete payload
   bytes mean bytes accepted by the upstream VT framer, before metadata decoding;
   they are not a raw PTY transcript. Status distinguishes complete, aborted,
   malformed, policy overflow and allocation failure. No malformed/oversized known
   command may disappear through `null` without a status. Failure reporting must use
   fixed scalar state and no allocation; notify at first overflow/allocation denial,
   rather than waiting indefinitely for a terminator. Later termination must not
   publish a retained prefix as complete. Counter saturation itself is explicit.
4. Host-managed dispatch routes the selected metadata command once to this event
   path and suppresses its ordinary title/cwd/notification side effect, including
   alternate ConEmu interpretations of OSC 9. Defaults retain upstream behavior.
   No query of borrowed title from its changed callback is needed. If getters remain
   unused/empty in this mode, the compatibility exporter must explicitly accept
   host-owned metadata rather than rejecting or silently dropping it.
   Keep `GHOSTTY_TERMINAL_OPT_TITLE_REPORT` disabled and reject attempts to enable
   it on the experimental constructor (default constructors retain upstream policy):
   a same-feed title update followed by CSI 21 t cannot use the deferred host
   mirror correctly. Enabling that capability would require an ordered host-owned
   query event/response contract and separate acceptance, not an empty/stale reply
   from the unused engine getter. No engine-handle escape may enable it implicitly.
5. Capture policy covers at least the current 16-KiB admitted control string,
   including exact documented header/terminator/sentinel accounting. Test the
   count at both parser and owner boundaries: 16,383/16,384/16,385 and fragmented
   terminators. The current conservative admission guard remains a separate
   pre-consumption owner policy; the extension does not claim to make it a full
   VT parser. No required string may hit a hidden 2-KiB or 1-KiB cap first.

The callback only copies descriptor/bytes into the fixed integrated collector;
it never calls Rust/UI/PTY, allocates, reenters the engine or blocks. A source/status
event participates in the same ordered stream as BEL, WRITE_PTY and RESET.

## Approved host normalization and reset policy

Reuse one Rust metadata normalizer at the production adapter boundary after
acceptance. Normalize the complete event into the preallocated candidate slot;
never publish borrowed bytes or separately mutated title/cwd Strings. Approved migration rules deliberately repair baseline parser defects rather than
treating those defects as cmux parity:

- Title: OSC 0/2 retains the full payload after its first delimiter, including
  semicolons. This deliberately fixes the baseline exact-field dispatch bug. Use
  lossy UTF-8 replacement, remove
  controls and U+202A–U+202E/U+2066–U+2069, then retain at most 256 Unicode scalars.
  Apply filtering before the scalar cap; never clip raw UTF-8 before filtering.
- Cwd: only OSC 7; consume its full raw UTF-8 `file://` URI after the first
  delimiter, including valid semicolon path characters. This explicitly approved
  exact-field parser repair preserves first-slash-after-authority handling;
  decode percent triplets once. Missing prefix/slash, unsupported scheme, empty
  value or invalid raw UTF-8 preserves previous cwd. Decoded non-UTF-8, control
  characters or >4,096 bytes clears cwd, matching the current assignment path.
  Keep malformed percent triplets literal for parity; strict percent rejection is
  a separate hardening change. Authority is untrusted metadata, not remote-origin
  authorization. Never execute the path, silently chdir, or treat it as markup.
- Notices: BEL source `bell`; OSC 9 notification body is the full remainder,
  source `osc9` and fixed title. Classify OSC 9 ConEmu subcommands using the pinned
  upstream parser before notification fallback; do not mislabel progress/cwd/
  process/other subcommands as notifications or execute their requested effects.
  OSC 777 recognizes `notify;title;body`, preserving the full body remainder
  (including semicolons), source `osc777`. These body rules deliberately fix
  baseline exact-field dispatch bugs. Use existing lossy
  display filtering; title cap 128 scalars, body 2,048 scalars; empty normalized
  title suppresses the notice. Keep the 32-notice ring and explicit eviction count.
  Preserve sequence identity across RESET; checked sequence exhaustion is approved
  instead of reusing the baseline saturating identity. Cover the boundary.
- RIS: retain host title/cwd/notices as an approved migration compatibility policy,
  not cmux-proven behavior. The
  current adapter retains them. Engine screen/history still reset; emit RESET in order,
  without clearing quota failure, metadata mirror or notice sequence. A later
  shell metadata command can replace the retained value. A product decision to
  clear metadata on RIS needs its own accepted change and native presentation test.
- DECSTR: no metadata/notice reset. OSC 9/1337 cwd forms preserve baseline host
  cwd state. Do not accidentally enable new metadata
  sources merely because the new engine recognizes them.
- CAN/SUB: suppress aborted updates. This approved behavior deliberately fixes
  baseline CAN dispatch; no exact-legacy abort mode is desired. Characterize
  remaining termination cases at the real boundary. Policy overflow/allocation
  failure is always fatal to the owner; malformed/unsupported complete metadata
  is an explicit ignored event without failing the terminal.

Read-only cmux evidence: `Sources/GhosttyTerminalView.swift:3529` routes Ghostty
SET_TITLE to title ingress, PWD to current-directory handling, and desktop
notification title/body to an ingress carrying tab/surface/lifecycle context. This
establishes its public action boundary, not OSC delimiter/abort/reset semantics,
which reside in its engine. No cmux implementation was copied or translated. The
permissive pinned Ghostty protocol parser supplies the delimiter/subcommand evidence;
host filtering, cwd policy and RIS retention remain explicit migration policy.

## Storage and failure publication

Retain one Resource/terminal/allocator owner. Add an independently charged fixed
metadata byte arena and event descriptors to its collector; no second terminal,
parser, general heap or per-callback allocation. With 4,096-byte feed calls and
one partially assembled string, a conservative candidate arena is
`16,384 + 4,096 = 20,480` bytes: one completed prior capture plus the remainder
of the current call. Confirm this bound against the actual framer; if it is not
provable, use an explicit smaller admission policy or fail on arena exhaustion.
The existing 1,024 combined-event bound still applies; a raw byte bound does not
promise every callback-dense input succeeds.

Rust needs bounded storage for normalized strings: title at most 1,024 UTF-8 bytes,
cwd 4,096, each of 32 notices title 512 plus body 8,192. Those payload maxima total
283,648 bytes per slot, 567,296 for two slots, before struct/event/source overhead.
Also charge any one-operation raw-copy scratch arena. Fallibly reserve all backing
storage before creating the engine, report actual capacities/inline storage and
reject a too-small host budget. Existing 2-MiB experimental host allowance is not
automatically an accepted product budget. Normalization, commit and slot swap
must not grow Vec/String storage or allocate replacement strings per update.

Capture quota/arena/event failure latches before operation completion; status and
allocator checks supplement the incomplete informational VT error bit. A callback
prefix followed by failure discards the entire staged generation, preserving
last-good title/cwd/notices/frame/replies. Recheck after normalization/render/export;
successful export seals the generation until exact-ID nonallocating commit.
RESET cannot repair a failed owner. No replay, fallback, instruction interruption
or consumed-byte receipt is claimed. Preceding successful chunks may already have
published before a later fragment fails; this is operation atomicity, not whole
control-stream rollback.

## Acceptance and production closure

1. Accepted: source/normalization/reset/abort/sequence policies and concrete cap
   accounting. The implementation remains isolated to its patch and integrated
   fixture; existing default/resource/effect/integrated controls must pass.
2. At the real integrated boundary, assert exact source, title/cwd/notices and
   ordering for the transcripts above plus byte-fragmented UTF-8/terminators,
   2,047/2,048 and 16-KiB boundaries, 4,096-byte decoded cwd/one-pass percent cases,
   32/33 notices, invalid/empty fields, RIS/DECSTR and agreed CAN/SUB behavior.
   Expected values come from the accepted contract, not the engine under test.
   Force capture allocation denial, metadata arena/event overflow and metadata
   callbacks before later engine/render denial; require last-good publication
   and zero engine mappings at close. An original temporary control removing
   provenance or the failure fence must fail the corresponding named assertion.
3. Independent review and hashed Linux/macOS arm64 hosted execution establish
   this boundary. Compile-only x86_64 and unavailable Mac address-space limits
   remain labeled. Report every patch/input hash and default-mode nonregression.
4. Next deliver an ordered real PTY round trip through the same owner: response
   bytes arrive once at the producing child; accepted generation/effects retain
   order under partial write, backpressure, close/cancel and session replacement.
   Capturing WRITE_PTY bytes alone does not satisfy this gate. Do not add another
   disconnected effects owner.
5. Integrate the accepted owner behind the existing production `Viewport`/input
   seam in one cohesive adapter slice, after remaining render/input/selection/
   scroll/cursor/style/native packaging capabilities are enumerated and covered.
   Require exact-session notice/native/UI delivery and named-Mac numeric workload
   memory/latency budgets using the existing sampler. Total app footprint remains
   separate from reserved engine mappings and fixed host capacities. Select or
   switch the product engine only after these capability and resource gates pass.

The path is metadata completion, real PTY delivery, then the production adapter
and native/workload gate. The experiments are acceptance steps for that same owner;
none independently closes terminal parity or warrants a user-facing release.

## Implementation and executable acceptance

`metadata.patch` adds original opt-in `zmux_ghostty_terminal_new_metadata_bounded_v1`
and its sized C header; the ordinary constructors keep upstream metadata behavior.
The selected command digits/first delimiter/payload count toward its 4..16,384-byte
creation cap. ESC ], BEL/ESC termination and one internal sentinel are excluded.
The upstream VT framer ends OSC at ESC before the following backslash; this slice
preserves that behavior and does not claim strict full-ST validation. The existing
application-wide conservative control guard has not been migrated into an app
adapter; unrelated OSC protocols retain their pinned parser limits.

Capture uses the Resource general allocator without smaller-buffer fallback.
An explicit failure record reaches the callback after the failing scalar/bulk step,
without waiting for termination. C diagnostics distinguish policy overflow (19)
from allocation denial (20), in addition to Integrated engine/failure statistics.
OSC 1337 cwd provenance now survives explicit Command/Action source fields; only
its existing recognized PWD action is observed, not an extended image/unknown-1337
capture contract. Requested ConEmu effects are classified but never executed.

Original `resource_new_factory` and `integrated_new_factory` let the metadata
sidecar use the same bounded Resource construction and publication fence. Its
1,024 descriptors and 20,480 raw bytes are included once in the fixed C charge.
The combined ordered event stream still caps at 1,024. Appending is admitted only
inside the active engine call, so it cannot mutate a sealed result. Default
constructors delegate with zero extra charge and no factory. The C sidecar is the
sole Integrated destructor; Rust's `ManuallyDrop<Raw>` is a nonowning view of that
same handle, not another terminal or teardown owner.

Two normalized Rust slots, frame/effect buffers and operation scratch are fallibly
reserved before engine creation. Local capacities are 1,693,696 backing bytes plus
3,064 inline bytes; fixed C charge is 201,056 bytes on Linux. No per-operation host
allocation is required for normalization, validation, exact commit or swap. Fixture
input generation and convenience copied observations remain outside these owner
slots. Reserved engine mappings and fixed storage do not bound total RSS.

The first frozen full receipt is
`/tmp/zmux-f04-metadata-repro-final-20261015/receipt.json`, SHA-256
`9e5770af9618b35d202fa686bb1754c4406e8b6ea5f6f3209ca026c6839dcbef`,
base HEAD `df2e060840d487b81ed78e2b92f16aa744c694b2` plus 29 input hashes matching
its frozen manifest. It is predecessor evidence if the explicitly approved cwd
semicolon case or contract wording subsequently changes those inputs. All 39 normal
phases and seven intended assertion-failure controls pass without timeout. Build
60.810s, Integrated 0.824s, metadata 0.894s under separately reported Linux
three-CPU-second/512-MiB address-space/no-core containment. These are not measured
product workload budgets. Nine runner lifecycle checks pass in 2.568s.

The metadata boundary covers source0/2 and OSC9/777 order with BEL/reply/RESET,
full title/body remainders, CAN/SUB and malformed/empty metadata, invalid UTF-8/bidi,
4,096-byte exact decoded cwd, percent-once/literal-invalid-percent cases, scalar
limits at their maximum UTF-8 byte width, 32/33 notices and checked sequence
exhaustion. It verifies 16,383/16,384 control contents, visible unterminated 16,385
failure, constructor allocation-denial positions, same-call valid callbacks then
actual capture denial, arena/event/snapshot failure, actual post-render denial,
last-good preservation, exact1,000 history, alternate/resize/clone and 128 zero-live-
mapping teardowns. New original-shim controls erase OSC777 provenance or omit the
arena failure fence and fail their named assertions. Five earlier controls remain.
Independent review, final-current-input receipts, hosted Mac execution and the
production closure gates above remain separate acceptance requirements.

### Independent full run and final fixture delta

Independent full receipt:
`/tmp/zmux-f04-metadata-independent-review-20261015/receipt.json`, SHA-256
`36ff8d6b1524082a2a50ce476ba9cf53c454658674a497afb9b297d1640ac840`.
Its 29 frozen inputs matched the first author receipt; all 39 normal phases and
seven intended exit-101 controls pass without timeouts. Source review found no
implementation blocker. Review identified the cwd-semicolon policy ambiguity;
the parent explicitly approved full raw file-URI semicolon paths as an additional
exact-field parser repair, preserving the existing safety checks.

The final delta changes only `METADATA_CONTRACT.md` wording and two expectations
in the existing `metadata-owner.rs` boundary: its percent-once URI case now contains
a semicolon path, and its ConEmu batch asserts the fourth explicit `1337/9` source
record. The other 27 implementation/patch/shim inputs remain unchanged. Final
manifest `/tmp/zmux-f04-metadata-frozen-final-manifest-20261015.json` SHA-256:
`5943f3cfa41d5351edc7b98e7a7d385072fea51d3da413480326ab6169ca60ea`.
Metadata patch SHA-256 remains
`a97eb547dd8d7b3c31db1bb6773dc94719f0b59357a1d0b9ad684e67280cede7`;
resource patch SHA-256 remains
`0ed579ca59f7e5d5f1ca695c8bc481bbfd3b1cad9832e215c9475178e6328f02`.

Final focused author receipt:
`/tmp/zmux-f04-metadata-focused-final-20261015/receipt.json`, SHA-256
`0c0557848b5bbcb55af2f60da955f2a74671e1d157c792a9f17e01b7824cca76`.
It checks all final 29 hashes before/after recompiling strict C shims and all
three Rust variants; normal metadata and both intended source/arena controls pass.
It links the unchanged engine patch/shims to full-run evidence and hashes the
preserved verified static library and compiled artifacts. This is an affected-
fixture rerun after test/contract clarification, not another broad product check.
The second full run used for preserving its library began before the final
`1337` assertion was added; only its unchanged engine-build provenance is used,
not a claim that its recorded fixture hash equals the final fixture.

Final independent focused receipt:
`/tmp/zmux-f04-metadata-focused-independent-review-20261015/receipt.json`, SHA-256
`6a3b4dfbd0162c99d35fffea228cbb4e1e3a6efce54dd4c859c16fb758c419be`.
The reviewer independently rebuilt strict C and final normal/control Rust variants,
then executed normal exit zero and both named source/arena exit-101 controls with
no timeout. All final 29 manifest hashes match before/after; only the documented
two fixture/contract inputs differ from its complete 39+7 run. The semicolon and
explicit `1337/9` expectations execute successfully. Independent review reports no
remaining blocker for this isolated extension; it does not accept production/native
integration or named-Mac workload limits.

Parent `scripts/check` passed an isolated snapshot at
`/tmp/zmux-metadata-parent-verify-gmt0wpge`, with private target
`/tmp/zmux-metadata-parent-target-gmt0wpge` and log
`/tmp/zmux-metadata-parent-check.log`, before that fixture/contract-only delta.
Hosted metadata execution, production adapter changes, PTY delivery, native
presentation and named-Mac numeric workload gates remain open.

## Hosted isolated metadata acceptance

Workflow `38777353391` at `b86a7005dc316fef0504edf409aa3c8d73560dea`
passed Linux x86_64 and macOS arm64. Parent downloaded receipts/logs to
`/tmp/zmux-f04-metadata-hosted-38777353391` and verified every one of29 input
hashes against the committed source. Mac receipt SHA256
`53f29c4227e0483e4f158895d30267a5025a8924d0992a04111c62345ae20084`;
Linux receipt `31c0ec9d05a7bc8d61b3a1976dd7c3fe04692e6a43b30f879b5418ed1c2de5db`.
Normal metadata execution and both expected assertion controls pass on each host.
Mac universal raw compile passes; metadata execution is host architecture only.
Darwin RLIMIT_AS remains unavailable (ValueError); explicit allocator accounting
is separate from process/RSS limits. No real PTY round trip, app engine switch,
production rendering/input integration or measured-workload acceptance follows.
