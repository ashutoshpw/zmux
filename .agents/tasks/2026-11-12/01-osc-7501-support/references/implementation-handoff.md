# Implementation handoff and ownership

## Parent objective and authorization

Implement the entire saved OSC 7501 plan on `main`, preserve unrelated dirty work,
run meaningful tests and mandatory checks, commit cohesive validated slices, push
scoped changes under AGENTS standing authorization, and deliver a verified fresh
universal macOS artifact. Use GPT-Luna workers at max effort; no recursive delegation.
Discovery has already been accepted. Do not restart broad discovery or stop at a plan.
Main owns acceptance, task docs, commits, workflow dispatch and final artifact checks.

Latest execution direction from the parent narrows this worker to the terminal,
session and Ghostty slice. Do not commit, push, dispatch workflows or manipulate an
app in this pass. The parent owns new candidate artifact production and overall
acceptance.

Baseline `0ac51927315e21830b6c31cb36ceb824730dc5c2`; first core slice is now committed
locally as `d573e8b87258d426ed8ccdfea20165a6a5fc2b5a` on main, exact validated tree
`e286dacdc4290caa167c7bf1f069c173f5c537ee`. Do not revert this commit. Source integration
continues uncommitted; no push yet. Baseline `RUST_TEST_THREADS=2
scripts/check` passed 707 tests, 7 ignored, 246 seconds. Log is
`/tmp/zmux-osc7501-baseline-check.log`. The full isolated core check is now green;
receipt `/tmp/zmux-osc7501-core-slice-after-lint.json` records its exact source.
Preserve the three unrelated dirty paths recorded in status.md; original hashes
are in `/tmp/zmux-osc7501-preservation.json`. Do not stage/revert them.

## Stable shared protocol owner

`crates/program-status` and root Cargo wiring exist; shared suite now passes 10
tests, including reusable same-session store copy, and package formatting.
API: `ProgramStatusStore::try_new(SessionEpoch)`, `try_clone`, `copy_from`,
`ingest(args, exact_full_sequence_len)`, `prompt_boundary`, `attached_child_exited`,
`ris`, `terminal_closed`, `snapshot(after_cursor)`. Snapshot/lifecycle methods may
return errors; inspect signatures, do not assume infallible calls.
Public serde DTOs include State/Kind/Record/Transition, epoch/cursor newtypes,
after-record event presentation data, cursor and dropped-through markers.
64 max records + 128 transitions; retained-store estimate 494,080 bytes under a
512 KiB budget, with allocator/parser/snapshot/fan-out exclusions. Never add heartbeat
expiry or fabricate provider identity. Tests include 64 literal maximum-payload records.

## Terminal/session owner: source slice complete; optional linked acceptance pending

Owns `vendor/vte`, `vendor/vt100`, terminal-adapter, ghostty-vt, session-runtime,
managed SSH Python producers and related tests/dependency docs/notices.
The VTE raw completion supplement, vt100 forwarding/RIS hooks and session engine
integration are complete. `Viewport.program_status` is always present to Rust and
serde-skipped on raw Viewport serialization. Each PTY receives a fresh worker epoch.
Default-engine read stages defer full viewport export until the existing frame
publication cadence, while replies stay staged until the sole writer physically
accepts their complete bytes. The default causal reply timeout is two seconds;
partial writes, deadlines and observed cancellation abort publication and require
recovery.

OSC 7501 query reply is ordered before primary DA. Prompt start and RIS update
lifetimes; DECSTR and alternate screen preserve records. Attached-child exit applies
only after final PTY drain. Explicit `State::Closed` publication calls
`terminal_closed()` and publishes its clear transition. Real-PTY and engine tests
cover these behaviors, byte ordering, close and fresh epochs.

Managed SSH `provider-launch.py` binds a private route datagram receipt socket,
sends `starting` through the lifecycle FIFO, and waits with a deadline for exact
Desktop admission before spawning. Reject/timeout and pre-spawn failure emit
`cancelled`; they launch the original command exactly once untracked. `ended` is
emitted only after an accepted spawned invocation exits. The lifecycle stream
forwards `cancelled` to the strict Rust wire-v4 consumer and removes its pending
route. No receipt filesystem path crosses SSH; cleanup checks socket ownership.
The current Rust wire schema consumes provider, route ID, provider generation and
phase under outer nonce/generation/sequence. Runtime identity comes from the active
Desktop stream binding.

Ghostty uses a NEW `program-status-v0.4.patch` after existing frozen resource and
metadata patches; historical patches/accepted roots stay untouched. The patch is
`tools/terminal-feasibility/program-status-v0.4.patch`, SHA-256
`f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925`. Its pinned
source archive is Ghostty commit `22fa134a07f5749edd9e92c6bc17d8aaf61843be`,
SHA-256 `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`.
Frozen patch inputs are `resource.patch` SHA-256
`0ed579ca59f7e5d5f1ca695c8bc481bbfd3b1cad9832e215c9475178e6328f02` and
`metadata.patch` SHA-256
`a97eb547dd8d7b3c31db1bb6773dc94719f0b59357a1d0b9ad684e67280cede7`. The
metadata header hash after those frozen patches is
`127555a4e60dec63714e68066eee8725d288ef17b1b53cdc6d89fd0df7ecde6c`; the new
patch leaves it unchanged. It adds raw 7501, prompt-start and RIS callbacks through
the existing C ABI and distinguishes DECSTR.

Consumer `build.rs` forwards `ZMUX_GHOSTTY_CANDIDATE_SET` (default `historical`)
to `accept-linkage.py --candidate-set` and reruns when the variable changes. The
new `ghostty-7501-v1` source and linkage candidate is still awaiting fresh produced
libraries/receipts. The pinned-source Zig test run passed 162/162 metadata-filtered
tests; patch application and all three source input hashes were independently
revalidated in this pass. `cargo check -p zmux-ghostty-vt` passes without the linked
feature. The Rust optional linked-engine target is feature-gated and remains
unverified until the parent/artifact owner supplies the fresh accepted candidate.

Focused local evidence for this slice:

- `cargo test --locked --offline -p zmux-program-status`: 10 passed;
- terminal-adapter parsing, Program Status and staged engine: 18 passed, including
  bounded causal-reply overload recovery;
- session lifecycle, Program Status, shell integration, child and engine targets:
  36 passed, 2 ignored; the engine target selected zero tests without the optional
  Ghostty feature. Coverage includes wire-v4 cancellation forwarding, rejected /
  timed-out / pre-spawn fallback exactly once, final-drain retirement, fresh PTY
  epochs and explicit-close clearing;
- vendored VTE raw dispatch: 3 passed;
- scoped root formatting, whitespace, strict Clippy and non-linked Ghostty compile:
  passed. Whole-vendored-tree formatting checks were not usable because VTE's
  manifest references absent `examples/parselog.rs` and both pinned dependency
  trees report formatting differences throughout untouched upstream code; those
  baselines were left intact;
- pinned Ghostty source suite previously passed 162/162 metadata-filtered tests for
  this supplement hash (not rerun here). This pass rechecked clean supplement
  application after frozen patches and matching source archive, frozen patch,
  supplement and unchanged metadata-header hashes. The optional linked engine
  remains pending fresh `ghostty-7501-v1` candidate production and linkage receipts.

## Desktop/bridge/CLI owner: Rust consumer source complete

Owns desktop-runtime and native-bridge, minimal remote-runtime lifecycle wire,
`apps/cli/src/hooks/provider.rs`, and directly relevant tests. Native AppKit and
terminal/session files belong to other workers: do not edit them without coordination.
Parent reports the Rust consumer slice and focused nine policy tests plus real-PTY
projection/live-epoch tests complete. Wire v4 has a distinct `cancelled` phase;
terminal/session producer behavior above is aligned with that consumer.

Accepted wire is optional top-level `program_status: Vec<ProgramStatusTerminal>`
and `program_status_alerts: {epoch: UUID,cursor,dropped_through_cursor,events}`.
Exact native field names are already implemented in Models.swift/NativeBridge.swift.
Terminals have full owner, session/command epoch, availability, record_count,
records_complete, omitted_records and records. Nonfocused terminals expose one
priority summary; focused owner exposes full bounded records. Selecting a summary
naturally exposes details on next snapshot; no separate detail command needed.
Keep the whole new status/alert projection within existing 1 MiB control / 8 MiB
native caps. No raw journal serialization; preserve canonical 64 worker records.

Alert policy: a registered pending adapter reserves Provider ownership before spawn
unless OSC already won the command epoch. Ending route does not erase the source
lock; prompt/session boundaries release it. Do not apply the new OSC 2-second
cooldown to existing provider alerts. No text/app matching or fabricated provider IDs.
New owner must consume undropped live initial transitions, including fast first
working->done or blocked; recovery/gaps baseline silently. Preserve event content
when done is followed by clear in one read. Parent identified and requested tests
for old-route filtering before consuming an earlier-epoch journal and for stale
desktop cached prompt epoch at pending registration: prove or fix those orderings.

Local Codex/OpenCode pre-spawn route registration/cleanup belongs in provider.rs;
Claude already has SessionStart registration. Remote strict wire 3->4 is accepted.
The managed producer sends the exact route UUID, provider and provider generation;
the active Desktop connection supplies runtime identity and outer
nonce/generation/sequence binding. Desktop ACK is required before spawn.
Timeout/rejection launches the original command once untracked and retires the
reservation through `cancelled`, without waiting on the UI or core executor.

## Native owner: source slice complete, available for fixes

Owns AppKit Swift and `scripts/check-native-sidebar`. Programs subgroup, safe labels,
exact selection, admitted OSC alert gate and managed sound-origin tags are present.
Do not route OSC through provider identity/gate. Fixtures now preserve same owner
for epoch negatives; invalid oversized owner lists reset bounded checkpoints.
Packaged sidebar smoke emits two literal reports then holds with zsh `read -r`
builtin (no long-lived sleep child), selects exact program, checks full unchanged
surface/session/PID inventory, saves `native-sidebar-programs.json` and
`zmux-sidebar-programs.png`. Existing workflow globs retain them.
Whitespace/Python checks pass; Linux has no swiftc/xcrun. Actual Swift/AppKit,
packaged UI/audio request and screenshot inspection remain hosted gates.

## Artifact tooling owner: source candidate validated; linkage evidence pending

Owns tools/terminal-engine-artifacts and its artifact/linkage workflows. Explicit
versioned 7501 source/build/linkage selectors preserve historical candidate JSON,
reviews, digests and default behavior. Versioned production requires both fresh
Apple slices and rejects reuse. Source candidates remain distinct from produced
library identities and independent acceptance.

### Source-candidate implementation and pins

- `candidate_sets.py` now recomputes the full canonical producer
  `library_recipe` digest, requires canonical `include/**/*.h` paths, binds the
  complete source header map to both producer receipt inventories, accepts only
  integer schema version 1 (not boolean), and requires a valid `darwin.supplement`
  map during versioned linkage preflight.
- `source.json` is a real source-only candidate at
  `tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json`,
  SHA-256 `1f3577941c34498ee1da1c3325dfbfd5c4a525c29a59fb6dba01ff1277eb3ec8`.
  It pins historical baseline `038aa8eb5dd381c353733550e95579221e1ea6967ae03a860c486436eb690c12`,
  upstream inputs `f96041d8d5b6af22ba1751bd7e76adb91b8331f9de0276c45aacadacd9e6019b`,
  and Darwin recipe `f57162b68a7866935bbd7662629b7508a4fbd6102134a5cd10efa21010be914d`.
- Pinned Ghostty archive SHA-256 is
  `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f` at commit
  `22fa134a07f5749edd9e92c6bc17d8aaf61843be`. Its 37 actual post-patch `include/**/*.h`
  hashes were recomputed after applying patches in this order: `resource.patch`
  (`0ed579ca59f7e5d5f1ca695c8bc481bbfd3b1cad9832e215c9475178e6328f02`),
  `metadata.patch`
  (`a97eb547dd8d7b3c31db1bb6773dc94719f0b59357a1d0b9ad684e67280cede7`), and
  `program-status-v0.4.patch`
  (`f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925`). The
  recorded Darwin ARM Zig 0.16.0 archive pin remains
  `b23d70deaa879b5c2d486ed3316f7eaa53e84acf6fc9cc747de152450d401489`.
- Historical manifests and linkage pins were not edited. No versioned linkage
  manifest, binary digest, producer output, or independent review was created.

### Command receipts and validation

- `python3 -m unittest discover -v` from `tools/terminal-engine-artifacts`:
  **44 passed, 1 skipped**; the skip requires an Apple linker. Python bytecode
  compilation of the changed tooling also passed.
- `python3 build-darwin.py --check-recipe` returned
  `{"state":"recipe_verified","candidate_set":"historical","frozen_inputs":148,"engine_built":false,"darwin_execution_verified":false}`.
- `python3 check-darwin-linkage.py --check-inputs` returned
  `{"state":"linkage_inputs_verified","candidate_set":"historical","sources":168,"engine_rebuilt":false,"native_linkage_verified":false}`.
- `python3 build-darwin.py --candidate-set ghostty-7501-v1 --check-recipe`
  returned `state=recipe_verified`, source path
  `tools/terminal-engine-artifacts/candidate-sets/ghostty-7501-v1/source.json`,
  source manifest SHA-256
  `1f3577941c34498ee1da1c3325dfbfd5c4a525c29a59fb6dba01ff1277eb3ec8`, ordered
  three patch records above, `frozen_inputs=149`, `engine_built=false`, and
  `darwin_execution_verified=false`. Independent recomputation matched the
  pinned archive, all patch/source hashes, all 37 header pins, baseline, upstream
  input and recipe hashes.

### Versioned linkage gates added, not yet executed

For `ghostty-7501-v1`, linkage now compiles
`terminal-adapter --test program_status` for both Apple architectures, retains
each executable's hash and Mach-O inventory, and runs only
`linked_engine_stages_raw_reports_query_prompt_ris_child_exit_and_close` on
ARM64, requiring exactly one pass. The existing four staged-engine cases remain
separate.

With `--session-runtime`, the versioned lane also compiles the session
`--test program_status` target for both architectures, retains its hash/Mach-O
evidence, and runs only
`linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain` on
ARM64, requiring exactly one pass alongside the existing four session-engine
cases. Linkage source hashing includes the adapter test and includes the session
test when present, along with Cargo manifests/lock and artifact license inputs.
At the last local inspection, the session test file did not yet contain the
requested Ghostty-gated test; the final hosted `--session-runtime` gate must
include it. No linked 7501 binary or test result is claimed.

## Remaining acceptance

Finish integrations and focused tests, independent post-implementation Luna review,
remediate through workers, mandatory scripts/check at selected commit trees, then
scoped main commits/push. New Ghostty candidate needs actual fresh arm64/x86_64
producer artifacts and optional hosted ARM linked Rust/session tests. Main dispatches
workflows after source validation. Download final universal app artifact and verify
inner ZIP/source/build metadata/architectures/minimum OS/signing/screenshots and task
acceptance. Record source vs hosted vs physical/live-provider proof separately.
