# F04 bounded callback effects experiment

2026-10-15. Original MIT, isolated in `tools/terminal-feasibility`; no production
engine switch, upstream pin change or change to `resource.patch`/resource contract.

## Frozen first slice

Use the pinned public C callbacks for BEL, WRITE_PTY and RESET only. A stable C
owner copies callback data synchronously into a fixed 1,024-event ordered batch
and 4,096-byte reply buffer. Events preserve order across bells, reply chunks and
resets. Callbacks do no UI/PTY/file/network work, acquire no external locks, allocate
nothing and never reenter engine processing. Feed calls are at most 4,096 bytes.
Bounds are experimental settings, not proposed production capacity changes.

The host must drain a successful batch before another consuming call. Insufficient
caller drain capacity rejects before draining or partially copying the batch. Reply/event
capacity overflow and engine/snapshot failure are explicit and sticky at call
return. The upstream call may process the rest of its feed; there is no exact
consumed-byte report, instruction interruption, replay, fallback or hidden reset.

A Rust non-Send/non-Sync RAII owner builds a candidate snapshot and effects batch,
then publishes an owned generation only after all checks succeed. On failure it
retains its last good owned generation and refuses further input. BEL maps to the
existing public `TerminalNotice` type (`bell`, fixed attention title, empty body),
with at most 32 retained notices and explicit oldest-notice eviction accounting.
RESET is an ordered control event, not a notice; it must not reset notice sequence
identity. PTY reply bytes are returned only as owned data, never actually written
by this fixture. Borrowed callback pointers never survive callback return.

## Independent acceptance and authoring gate

The guarded contracts are callback order/data lifetime, bounded once-only drain,
capacity-before-consumption, explicit overflow and publication after success. A
credible regression is an escaped borrowed reply, reordered/drop-with-success
callback, cleared pending batch, replayed failed feed or mutated published frame.
Existing raw/exporter fixtures cover grid/query samples, but not the ordered batch
or failure/publication lifecycle. The new owner is the real C integration boundary;
no application test seam, fake terminal or mocked allocator is introduced.

Use fixed protocol expectations: mixed BEL/DSR/RESET order and exact replies,
fragmented DSR, resize/continued output, owned survival after further feed/drop,
small output buffers without drain, pending-batch refusal before input, exact reply
and event limits/limit+1 failure, last-good generation after failure and repeated
create/feed/drop. Negative controls must fail for the intended overflow/order/data
assertion; no broad native or total-memory acceptance follows from this fixture.

## Explicit next metadata extension

Title/cwd and OSC9/777 are not supported by this first slice. The pinned handler
truncates raw title at 1,024 bytes and raw cwd at 4,096 bytes before host sanitizing
or URI decoding; its getters expose no truncation marker. A percent-encoded valid
4,096-byte path can therefore become a different truncated path. The desktop
notification callback carries title/body without OSC9 versus OSC777 provenance,
which cannot safely be inferred from an empty title. Source-tagged bounded raw
metadata/overflow reporting needs a separate original patch/API design and review.
RIS clears upstream title/cwd without their change callbacks, so a later metadata
exporter must also observe RESET. It must preserve current display sanitization,
URI/path validation, source labels and 16-KiB control admission before any migration.

Mac budgets remain separate: named hardware/build, isolated one/eight idle and
visible/hidden output, browser-attributed and SSH-stall phases, repeated baseline
measurements, then versioned CPU/RSS/footprint, marker/main-actor latency, fairness,
queue and cleanup thresholds. Existing whole-journey samples are not those budgets.

Parent authorized optional `--effects-experiment` integration after resource hosted
verification completed. Default and resource-only behavior remain available; all
modes preserve the exact upstream pin and application dependencies. Effects are
host-only, including when combined with `--macos-universal`; universal slices still
refer to the original raw fixture/library. The runner hashes the three effect
inputs and executes two temporary shim mutations: silently dropped reply overflow
and a reset mislabeled as BEL. Each must fail its named assertion with exit 101;
the receipt labels intentional failures and hashes the temporary mutated source.
Resource patch and contract hashes are unchanged. No staging/commit by this worker.

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

## Proposed separate metadata/provenance extension

Do not infer a notification protocol from title emptiness or reinterpret a clipped
cwd as a complete path. The next smallest source extension should carry explicit
protocol provenance and completeness from OSC capture through the command/action,
handler and C boundary. Review the parser's normal/allocating capture limits as well
as `windowTitle`/`reportPwd`; increasing a handler limit alone is not sufficient.
Keep old public construction/default behavior, adding strict options only for the
isolated integration owner and a separately hashed original patch against the pin.

For strict metadata input, retain the existing 16-KiB control admission boundary,
charged scratch storage and explicit overflow/incomplete status before publication.
The Rust owner can then apply the existing 256-display-character title filter and
preserve or reject the exact UTF-8 cwd (4,096 decoded bytes). Preserve invalid URI
behavior and distinguish empty/cleared metadata from absent/unsupported input.
Carry OSC0/2, OSC7/9/1337 and OSC9/777 source explicitly where their semantics differ;
unknown callbacks cannot recover a known OSC after its provenance was discarded.
Never turn observed remote cwd into local filesystem authority or a launch command.

Acceptance needs fragmented ST/BEL and UTF-8, long titles containing filtered/bidi
characters, percent-encoded paths whose raw URI exceeds 4,096 bytes but decoded path
fits, invalid escapes/UTF-8/controls, empty source-specific metadata, exact source
labels, interleaved notices/reset, quota denial and no partial publication. First
characterize the current adapter's RIS metadata behavior with a real transcript;
upstream clearing and legacy host retention must not silently define a new policy.
Only after this source-backed contract is frozen should the metadata patch be
implemented. No metadata patch or application change is included in this slice.


## Reviewed local receipt

`/tmp/zmux-f04-effects-repro-reviewed-20261015/receipt.json`, SHA-256
`5f9f75280eeed19843798535b8f0dade799cf802fabbcfe906b07bb59a2cac1e`, records
base HEAD `1868dcc164e85e00af0bbea0869b63b039d8c592` plus 17 exact working-tree
harness hashes; all were rechecked after completion. It uses a labeled explicit
existing dependency cache, fresh verified source/toolchain archives and a private
Cargo target. Both experiment flags run: all ordinary raw/adapter/compatibility,
resource and effects stages pass. Two original-constructor resource controls and
two temporary original-shim effects controls intentionally fail their named
assertions with exit 101. The effects fixture takes 0.332 seconds; this is not a
performance measurement or accepted latency budget.

It verifies exact default 4,096 reply bytes (682 DSR replies plus one status reply),
a configured six-byte success/seven-byte refusal, default overflow, exactly 1,024
callback events and limit+1 refusal, explicit ring eviction, failed snapshot
publication and 128 owner lifecycles. Fixed C context plus the existing Probe upper
bound is 24,632 bytes on this Linux host; copied snapshots and Rust frame/batch
storage are separate. Independent source review reports no remaining blocker with
the preceding proof limits. Nine existing real-process runner tests, strict C
warnings, Rust formatting, Python syntax and scoped diff checks also pass. No Mac
callback-effects execution is included; parent owns hosted workflow delivery.
