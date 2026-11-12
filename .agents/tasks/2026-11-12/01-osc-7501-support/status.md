# Status

- **State:** in-progress
- **Updated:** 2026-11-12
- **Baseline:** `0ac5192` on `main`; protocol/parser slice committed as `d573e8b`. Integration source committed and pushed as `3b385182e43e66bde01100640b9c6cf213eac048`.

## Current overall acceptance

- Latest stable full `scripts/check` passed: **756 passed, 0 failed, 7 ignored**,
  strict workspace Clippy, formatting, license inventory and native bridge ABI.
  Source fingerprints were unchanged and unrelated preservation hashes match.
  Final scoped Session repair review accepted both fixes with focused
  independent PTY re-execution. Integration commit/push and hosted delivery follow.
  See [portable validation](references/portable-validation.md). Earlier failed
  check entries below are historical evidence, superseded by this full pass.

- Protocol, terminal/session/Ghostty supplement, desktop/provider transport, CLI,
  native presentation and usage documentation source slices are implemented.
- First full integrated `scripts/check` failed at `apps/cli/tests/ssh.rs`: all
  three managed-SSH cases observed a remote helper abort with
  `assertion failed: output.write(&bytes).is_ok()` and no lifecycle hello.
  The parent measured the actual built `remote_command` at 146,116 bytes;
  direct `exec` of `/bin/true` with that argument returns `E2BIG` (OS error 7),
  exceeding Linux's 128 KiB single-argument limit. The completed compressed
  bootstrap is 41,957 bytes and passes direct exec, exact byte recovery and all
  3 loopback SSH tests. Focused session checks, strict Clippy, formatting and
  license inventory pass; its independent review found no runtime defect.
  Parent resolved the reported test-quality findings with focused green tests
  and an isolated missing-final-OSC negative control.
  Log of the original failure: `/tmp/zmux-osc7501-integrated-check.log`.
  This is not a passing gate. VTE raw dispatch and non-linked Ghostty compile
  passed as supplemental checks.
- The native completion recheck passed its scoped source review. Artifact-tool
  review found recipe-body digest, canonical header pin/binding, Boolean schema
  and missing supplement-map validation gaps. Repairs and the real versioned
  source manifest are complete. Its independent recheck found no implementation
  defect. Parent expanded the recipe fixture to cover every producer component;
  the Python suite ran 44 tests total, 43 passed and 1 Apple-linker skip.
  Fresh library production and linked OSC execution are still required.
- Integrated review found Ghostty abort-before-close retaining records, stale
  prior-command OSC ownership suppressing provider alerts, and malformed duplicate
  ID validation order. Parent reproduced the ID regression before repair, then
  fixed parsing order; all 10 shared protocol tests pass. Parent removed the
  failed-engine guard from Rust-only terminal-close metadata clearing; actual
  linked Session close execution remains a hosted gate. Provider epoch arbitration
  now uses worker-ordered Session checkpoints; its focused policy, real-PTY route,
  public Desktop and provider lifecycle tests pass. The scoped combined strict
  Clippy run is still blocked by the out-of-scope `attention.rs:853` warning. See
  [provider epoch repair](references/provider-epoch-repair.md). The parent will run
  full validation after all fixes stabilize.
- The second integrated check exited 101 after 465 passed, 2 failed and
  5 ignored. Source fingerprints were unchanged and the unrelated ledger matched.
  The primary failure is the native-frame Unicode/delta test timing out; the other
  failure is its shared test mutex poisoning. Native frame diagnosis found a stale test-consumer cursor, not a production defect.
  The fixture now synchronizes via full snapshots before polling deltas; both
  native-frame tests pass with original semantic assertions and deadlines.
  Strict workspace Clippy now passes after notification metadata grouping.
  Log/receipt: `/tmp/zmux-osc7501-final-check.log` and `.json`.
- The Session checkpoint repair now holds an ordered barrier until bounded PTY
  draining observes WouldBlock/EOF and prior deferred output, writes and replies
  commit. Its deterministic real-PTY regression failed under immediate publication
  and passes after repair. The independent final Session repair recheck accepted
  graceful-close dirty-frame publication and natural-exit checkpoint state after
  three narrow focused checks; see
  [final Session repair recheck](references/final-session-repairs-recheck.md).
  Parent retains the full `scripts/check`; Desktop/remote reservation retirement
  across reconnects remains separate. Neither focused results nor review
  establish delivery.
- Parent combined focused check after the drain repair passed Session unit 2/2,
  real-PTY Program Status 6/6, native bridge unit 3/3 and native-frame ABI 2/2.
  Artifact Python policy suite passed 43 with one expected Apple-linker skip;
  the versioned recipe preflight verified 149 inputs without building an engine.
- Third full stable check exited 101 after **628 passed, 1 failed, 7 ignored**.
  Source fingerprints were unchanged and unrelated preservation hashes match.
  Existing graceful shutdown flood test received an aborted terminal-owner error
  instead of the required forced-deadline outcome. A bounded Session owner
  remediation is in progress; all assertions and deadlines remain required.
  Evidence: `/tmp/zmux-osc7501-check-round3.log` and `.json`.
- Integration source is pushed as `3b385182e43e66bde01100640b9c6cf213eac048`.
  Foundation run `43299857515` and macOS run `43299857734` started on that source.
  macOS Swift typecheck failed at `ProgramSidebarViews.swift:602-606`: the
  tuple-returning helper needs an explicit switch return after its guard. A
  narrow native source repair is complete in the shared checkout; see
  [Swift typecheck repair](references/swift-typecheck-repair.md). No accepted app
  artifact exists.
  Fresh Ghostty producer run `43300002338` builds both slices without reuse.
  Actual libraries, independent byte review and linked acceptance remain pending.
  The connector dispatch returned 403; the authorized local GitHub CLI dispatch
  succeeded. Native compilation/execution remains hosted-only on this Linux host.
- Historical planning/discovery notes below describe the entry state and are
  superseded by the implemented source and current acceptance above.

## Current terminal/session/Ghostty implementation

- Default terminal/session implementation is complete. The worker gives each PTY
  a fresh session epoch, stages metadata and causal replies on the sole writer,
  drains final PTY output before retiring working/blocked records, and clears
  records on explicit close. Prompt markers and RIS update their specified
  lifetimes; DECSTR and alternate-screen changes preserve records.
- Deferred default-engine PTY stages commit without exporting a full viewport for
  every read. `refresh_viewport()` exports once at the existing frame publication
  cadence; the real-PTY engine test covers this coalescing behavior.
- Managed SSH provider launch now sends `cancelled` after rejected/timed-out
  admission and pre-spawn failure. `ended` is reserved for an admitted spawned
  invocation that exits. The lifecycle stream accepts and forwards `cancelled`,
  releasing the pending epoch reservation. The fixture checks exact route receipt,
  single original-command fallback and cleanup for Codex and OpenCode.
- The new Ghostty supplement is
  [`program-status-v0.4.patch`](../../../../tools/terminal-feasibility/program-status-v0.4.patch),
  SHA-256 `f2ea5e25d0f9015c4118ef864ab8c6c85f64e7285a5f06cad8321516995f4925`.
  It applies after the unchanged `resource.patch` and `metadata.patch` to pinned
  Ghostty commit `22fa134a07f5749edd9e92c6bc17d8aaf61843be` (source archive SHA-256
  `a07afea0e0b01cdfbf5b418c56bfd67366929dac0f9636267de5e5637351df5f`). The
  frozen-patch metadata header is unchanged at SHA-256
  `127555a4e60dec63714e68066eee8725d288ef17b1b53cdc6d89fd0df7ecde6c`.
- The optional linked Ghostty Rust/session lane remains pending its fresh
  `ghostty-7501-v1` libraries and linkage receipt; the source-only manifest exists. The current
  historical library cannot verify 7501. No binary digest or review was invented;
  artifact production and hosted linkage remain with the parent/artifact owner.

### Focused validation

- `cargo test --locked --offline -p zmux-program-status`: **10 passed**.
- `cargo test --locked --offline -p zmux-terminal-adapter --test parsing --test program_status --test staged_engine`: **18 passed**.
- `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-session-runtime --test lifecycle --test program_status --test shell_integration --test child --test engine`: **36 passed, 2 ignored**; the `engine` target selected 0 tests without the optional `ghostty` feature. Coverage includes wire-v4 cancellation forwarding, rejected/timed-out/pre-spawn fallback exactly once, final-drain retirement, fresh PTY epochs and explicit-close clearing.
- `CARGO_TARGET_DIR=/tmp/zmux-osc7501-vte-target cargo test --manifest-path vendor/vte/Cargo.toml --offline raw_osc_dispatch`: **3 passed**.
- Scoped root `cargo fmt --check`, Clippy with `-D warnings`, `cargo check --locked --offline -p zmux-ghostty-vt`, and `git diff --check`: **passed**.
- Whole-vendored-tree `cargo fmt` checks are not clean: the pinned VTE manifest references absent `examples/parselog.rs`, and the VTE/vt100 copies report formatting differences throughout untouched upstream files. No vendored baseline formatting was changed.
- The pinned Ghostty source test run for this exact supplement hash previously passed **162/162** metadata-filtered tests (not rerun here). This pass verified that the supplement applies cleanly after the frozen patches; the pinned archive SHA-256, frozen patch hashes, supplement SHA-256 and unchanged metadata-header SHA-256 all match the handoff. Fresh optional linked Rust/session acceptance remains pending the candidate artifact.

## Completed planning evidence

- Refreshed the canonical Program Status Protocol page: revision 0.4, 2026-10-10.
- Inspected default terminal callbacks, linked Ghostty metadata normalization,
  worker causal replies, provider lifecycle ownership, native alert delivery,
  existing parser/PTY/desktop test boundaries and development-build requirements.
- Saved a six-slice implementation plan, explicit protocol/coexistence decisions
  and an owner-based test matrix. No runtime/test code changed; no app build required.
- Important source finding: Ghostty has a metadata callback path, but Zmux's current
  normalizer does not expose 7501. Upstream support alone is not product support.
- Exact pinned vt100 reset/raw-length/reply APIs and Ghostty supplement capability
  remain the first implementation feasibility gate. Conventional local Cargo
  registry source directory was absent; do not claim those APIs were verified.

## Preserved unrelated work

- Modified `.agents/tasks/2026-10-18/01-cloud-origin-build/status.md`.
- Untracked `.github/workflows/profile-lifecycle-probe.yml`.
- Untracked `tools/native-profile-lifecycle/`.

## Remaining parent acceptance

Finish versioned Ghostty candidate production/linkage and the applicable hosted
native/artifact gates, then update overall task acceptance. This worker's explicit
scope ends at the terminal/session/Ghostty source slice; no commit, push, workflow
dispatch or app manipulation was performed.

## Accepted discovery and current ownership

- Desktop/bridge/CLI continuation completed focused implementation/checks. The
  terminal/session producer now emits and forwards `cancelled` on pre-spawn
  failure/timeout; normal `ended` remains reserved for the spawned invocation's
  exit. Parent reports 9 desktop policy tests and the PTY projection/live-epoch
  tests passing.
- Native review remediation completed: newly admitted completions survive prompt
  advancement within the prior/current same-session checkpoint bounds; stale,
  future, replaced, recovery and oversize-event cases are covered. Shared ID
  duplicate/invalid-path edge cases were added to existing tests (10/10 pass).
- Artifact remediation completed: fresh IDs accepted only on the versioned route,
  historical IDs remain pinned, schema version 1 enforced, and executable input
  checks use real controlled file/hash fixtures. Tool suite now passes 39 tests
  with one Apple-linker skip; actual new patch/manifests/produced libraries remain
  outstanding, not replaced by placeholders.
- Desktop native worker lost its task context after partial source edits and
  stopped. Parent preserved those edits and assigned their completion to a fresh
  GPT-6-Luna max T3 child on the shared main checkout, using the durable
  [implementation handoff](references/implementation-handoff.md). Terminal and
  artifact owners continue unchanged; no code was reverted for this handoff.
- Baseline validation passed before code edits. Luna protocol-owner implementation
  owns the new dependency-neutral `crates/program-status` crate and root Cargo wiring.
- Terminal discovery verified the actual patched `vendor/vt100` and locked VTE
  source: split OSC callbacks discard raw termination/length, VTE has a 16-parameter
  truncation boundary, and its dispatch-on-ESC behavior cannot prove completed ST.
  Accepted a narrow attributed VTE/vt100 raw-event supplement plus RIS callbacks,
  avoiding a second parser or reconstructed field lengths. Linked Ghostty hook
  discovery found a feasible source-only metadata whitelist supplement using the
  existing C ABI; current retained artifacts cannot observe 7501 and must not be
  mutated. New exact-source artifacts and linkage acceptance are required.
- Luna desktop implementation owns desktop-runtime/native-bridge status projection,
  selection, source arbitration and notification-origin metadata. Status must be a
  top-level all-terminal field because native frame publication is focus-scoped.
- Accepted native discovery and authorized AppKit/script implementation: optional
  top-level program status decoding, separate Programs rows, exact validated
  selection, OSC-origin sound gate, and literal real-PTY packaged sidebar fixtures.
  Existing workflow receipt/screenshot globs can retain the added native evidence.
  Main owns acceptance, task evidence and later build delivery.
- Launch-order discovery found local Codex/OpenCode spawn before their first
  desktop lifecycle observation. Authorized a narrow CLI pending-route signal
  before provider spawn and scoped cleanup, plus the managed-SSH equivalent if
  needed; `app` text cannot stand in for that evidence. Desktop worker owns this
  extension and coordinates session-owned remote producers with terminal worker.
- Canonical spec recheck clarified invalid ID grammar is ignored with no mutation;
  over-limit IDs remain hard-limit rejections. The shared decoder follows this
  distinction and never falls back to the root record.
- Managed-SSH pending-route admission is acknowledged over a bounded private
  remote-local datagram socket before spawn. FIFO ordering alone is not treated as
  Desktop admission. Timeout/rejection launches the original command once
  untracked, emits `cancelled`, and removes only the socket owned by that launch.
- Parent integration review identified aggregate serialization risk: maximum legal
  records across 32 terminals can exceed the existing 1 MiB control / 8 MiB native
  response caps. Desktop projection must be explicitly budgeted, with omissions
  and bounded detail access; raw journals need not be serialized to AppKit because
  the separate admitted-alert feed owns native delivery. Added D08 boundary test.
- Accepted artifact-tooling discovery: historical producer/linkage scripts are
  hard-pinned to old source/library digests and default arm64 reuse. Authorized
  explicit versioned 7501 source-candidate and linkage selectors, fresh two-slice
  no-reuse production, and distinct output/review acceptance. Historical candidate
  manifests, reviews, artifact IDs and retained roots stay immutable.
- Parent artifact-tooling review found the new acceptor used an unimported
  `digest` symbol; fixed the missing import. Also found the new linkage selector
  still unconditionally restricts download IDs to the historical run/artifact,
  so the new candidate cannot yet pass `--check-inputs` with fresh produced IDs.
  Source/linkage schema-version refusal and an executable positive selector
  acceptance path need verification before this slice is considered complete.
- Parent early integration review requested two explicit regressions: initial
  nonempty Session snapshots must preserve fast live working/done or blocked
  transitions, and new OSC cooldown must not rate-limit existing provider events.
  Recovery baselines and actual final-output completions remain distinct cases.

## Validation

- First cohesive core slice committed locally on `main` as
  `d573e8b87258d426ed8ccdfea20165a6a5fc2b5a`, exact validated tree
  `e286dacdc4290caa167c7bf1f069c173f5c537ee`. Full isolated `RUST_TEST_THREADS=2
  scripts/check` passed after lint fixes (2026-11-12 13:49:44 UTC); source hashes
  unchanged across run. Receipt `/tmp/zmux-osc7501-core-slice-after-lint.json`.
  Normal commit hooks were honored; integration work and unrelated dirty paths
  remained unstaged. No push or app artifact delivery has occurred yet.
- Isolated first core tree `4fa392dee1b2a9935e58ad030ccf3b3090fff27e`
  ran mandatory checks: 717 tests passed / 0 failed / 7 ignored, then Clippy
  stopped on seven new shared-crate style warnings (exit 101). Notice inventory,
  native bridge and raw VTE 3-test gates passed separately. Receipt:
  `/tmp/zmux-osc7501-core-slice-receipt.json`. This is not a green full check.
- Parent fixed the mechanical shared-crate Clippy suggestions and three test
  array-allocation warnings, without suppressions or behavior changes. Current
  `cargo clippy --locked --offline -p zmux-program-status --all-targets -- -D
  warnings` and shared tests (10/10) pass. Full isolated acceptance rerun pending.
- Terminal/session compile milestone: `cargo check --locked --offline -p
  zmux-terminal-adapter -p zmux-session-runtime` passed. Always-present typed
  status is serde-skipped on raw Viewport. Default worker epochs and staged causal
  replies are implemented; real-PTY and full regression checks remain in progress.
- A fresh independent GPT-6-Luna max T3 review covered the completed shared
  protocol, VTE/vt100 supplement and native source slices. Review completed:
  shared store/parser passed; native command-epoch equality drops a live done
  followed by prompt before UI poll (P2), and legacy smoke retained two sound tags
  while asserting they were absent (P2). Parent fixed the trivial legacy fixture;
  a separate Luna remediation task owns native epoch admission plus ID-duplicate
  and native event-count coverage gaps. Integrated source still needs final review.
- Artifact selector regression suite currently passes 35 tests with one Apple
  linker skip; historical recipe and input checks pass. New candidate absence
  rejects explicitly without falling back to historical libraries. Fresh 7501
  manifest/outputs/acceptance remain outstanding.
- Native source slice now includes Programs rows, source-specific audio gate and
  real-PTY sidebar fixtures. Parent caught and worker fixed owner-changing epoch
  negative fixtures plus over-limit checkpoint retention. Worker reports scoped
  whitespace and Python syntax checks passed; Swift/AppKit compilation and real
  packaged acceptance remain unexecuted until the macOS gates.
- Shared owner accepted: `cargo test --locked --offline -p zmux-program-status`
  passed all 9 integration contract tests (P01-P08), with no failures; package fmt
  and scoped whitespace checks passed. Retained-store ledger is 494,080 bytes for
  max current records plus after-record transition snapshots under a 512 KiB
  estimate, excluding allocator bookkeeping, parser/PTY scratch, owned snapshots,
  serialization and fan-out. Integrated/independent review is still required.
- Follow-up P06 aggregate case now uses literal 64 unique maximum-payload records
  (128-byte IDs, 32-byte app, 192-byte decoded title, 2048-byte decoded message)
  and fills 128 after-record transition snapshots. Shared suite still passes 9/9;
  this guards the spec minimum independently of the configured capacity constant.
- Authoritative VTE raw-OSC boundary: `cargo test --manifest-path vendor/vte/Cargo.toml
  --offline raw_osc_dispatch` passed 3 tests; `cargo check --locked --offline -p
  zmux-terminal-adapter` passed through patched VTE/vt100. This verifies the parser
  supplement boundary, not completed Session/native OSC support.
- Shared semantic crate initial compile: `cargo check -p zmux-program-status --offline`
  passed; protocol owner reports Cargo.lock updated for the shared crate and local
  VTE patch. P01-P08 contract tests and engine/session integration remain in progress.
- Implementation baseline: `RUST_TEST_THREADS=2 scripts/check` passed on `0ac5192`
  before runtime/test edits, exit 0 after 246 seconds (2026-11-12 12:01:48 UTC).
  Log: `/tmp/zmux-osc7501-baseline-check.log`. This establishes baseline Linux
  formatting, workspace tests, Clippy, license inventory and native bridge checks;
  it is not OSC support or macOS packaged acceptance.
- Planning-document local links, required sections, whitespace and scope checks passed.
- Reviewed protocol limits, duplicate-field handling, callback feasibility, source
  handoff policy and test ownership; implementation acceptance boxes remain unchecked.
- Runtime tests, linked-engine gates, hosted CI and app packaging: not run for this
  documentation-only request. Future feature delivery requires the full recorded gates.

## Scoped bootstrap transport and linked-PTY follow-up

- Reproduced the managed-SSH exec-argument defect before repair: the trusted
  `remote_command()` argument was 146,116 bytes and direct `/bin/true` exec failed
  with `E2BIG` (OS error 7). The new exec-bound regression failed for the same
  reason before the transport change.
- Replaced expanded command transport with a cached Rust gzip/Base64 payload and
  bounded `/bin/sh` decoder using builtin `eval`. Expanded and encoded limits are
  256 KiB and 48 KiB. The decoder supports GNU `base64 -d` and BSD `base64 -D`,
  preserves the original shell argument positions and exact trailing bootstrap
  bytes, and prints a diagnostic before falling back to the original shell if
  `base64` or `gzip` cannot decode. The decoder adds no Python 3 requirement.
- The repaired remote command measured 41,957 bytes and direct exec succeeded.
  The actual remote bootstrap remains over 128 KiB; a Rust gzip round-trip test
  checks byte-for-byte recovery.
- Focused validation passed: session-runtime `--lib --test shell_integration
  --test program_status` (1 unit, 13 shell integration, 5 Program Status passed;
  2 existing shell tests ignored); `RUST_TEST_THREADS=2 cargo test --locked
  --offline -p zmux --test ssh` (3 passed); package formatting, strict session
  runtime Clippy and `git diff --check`; `scripts/license-inventory --check`.
- The artifact lane's exact feature-gated test
  `linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain`
  reuses the explicit-close fixture after the linked query/final-drain case. It
  checks the Ghostty-specific DA reply plus clear records after `Session::shutdown`
  aborts the staged engine. Actual linked execution remains pending the accepted
  macOS Ghostty artifact.
- The full `scripts/check` and its before/after source fingerprint are held until
  the concurrent provider-epoch source repair is stable. The parent must fingerprint
  all relevant Rust/native files, embedded shell/Python/JavaScript payloads, Cargo
  manifests and `Cargo.lock` immediately before and after the accepted full run;
  the older fingerprint omitted payloads and does not establish this source set.
- Details: [bounded bootstrap repair](references/bootstrap-repair.md).

## Native frame delta test remediation (2026-11-12)

- The `native_frames` timeout came from repeatedly polling a fixed revision and discarding intermediate delta responses. Diagnostic evidence showed revisions 13 and 14 returned before the target output at 15; storing revision 14 evicted revision 12 from the two-entry history. The production ABI correctly returned a full baseline for the now-unknown revision. The unpoisoned sibling test also needed full-viewport synchronization because delta `contents` is a replacement span.
- Changed-state waits now synchronize through the full snapshot ABI, then poll the original delta baseline within the same eight-second deadline. Existing delta, row, Unicode, link, generation and recovery assertions remain. No production owner files changed. See [native frame repair](references/native-frame-repair.md).
- Final focused validation: native frame ABI tests 2/2, terminal-adapter staged/Program Status tests 7/7, session-runtime real-PTY Program Status tests 5/5, strict all-target Clippy for native-bridge/terminal-adapter, and strict session-runtime library Clippy passed. The broader session-runtime all-target lint is blocked by unrelated `clippy::ptr_arg` in `crates/session-runtime/tests/program_status.rs:83`, which was left untouched. The required full integrated check remains with the parent and was not rerun here.

## Remote provider route retirement (2026-11-12)

- The accepted second provider-epoch P2 is repaired in the desktop policy owner. Route-start ACK enqueue remains pending evidence; exact transport loss removes unconfirmed reservations and clears only source locks predicted by those reservations. Valid lifecycle ownership and normal spawned-invocation `Ended` routes remain confirmed and retain ended-epoch ownership. Provider alerts promote predictive locks so later cancellation cannot downgrade established provider authority.
- Added policy regressions for unconfirmed transport retirement, provider-alert promotion before cancellation, and normal ended-route ownership. The unconfirmed-retirement test failed before the implementation for the intended reason: 0 OSC alerts admitted after retirement, expected 1.
- Added a loopback SSH regression that receives a real framed provider-route start, queues its exact receipt, observes a gapped G1 stream and actual G2 replacement, then proves OSC alert admission after retiring G1. Existing exact-binding receipt and provider-state stream tests pass.
- Validation passed: desktop policy 14/14; real SSH route-reconnect 1/1; exact receipt binding 1/1; provider-state sequence-gap 1/1; public Desktop PTY status 1/1; session lifecycle 17/17; session PTY Program Status 6/6; strict desktop-runtime all-target Clippy; package formatting; scoped whitespace check.
- The SSH fixture does not pause the local writer before its first receipt pipe write, so it does not prove that exact queue-drop interleaving. The peer leaves stdin unread and the later generation is produced by the real stream supervisor. If a receipt reached the provider wrapper but no lifecycle observation or normal `Ended` phase reached Desktop before transport loss, delivery remains ambiguous; recovery intentionally releases that unconfirmed reservation. Details and the exact proof limits are in [remote route retirement](references/remote-route-retirement.md).
- No Session source/test, `tests/program_status.rs`, native UI, bridge, artifact tool, or Cargo file was changed. `scripts/check`, commit, and push remain with the parent.

## Graceful shutdown dirty-frame publication (2026-11-12)

- Reproduced the round-three shutdown failure with the exact existing flood
  fixture: before repair it returned `terminal owner unavailable` after
  3,514,368 bytes instead of the graceful-deadline error. The retained child was
  reaped, but the worker result/status was wrong because a pre-close dirty frame
  reached `refresh_viewport()` after `abort_engine()`.
- Graceful close now retires the stale dirty-frame flag and the periodic publish
  path also requires no close deadline. The worker keeps its existing bounded
  PTY drain, byte accounting, shared deadline, escalation, and cleanup; no
  refresh or parse occurs on the aborted engine. Normal terminal-close status
  publication still clears program records.
- The exact graceful-deadline fixture passed after the repair. The final
  combined focused run passed 44 tests with 2 ignored across Session library,
  shutdown, lifecycle, Program Status, shell integration, and child suites; this
  includes both checkpoint tests, cleanup failure, nested graceful cleanup, and
  explicit-close record clearing. Strict Session library Clippy, package fmt,
  and scoped whitespace checks passed. Details: [graceful-close repair](references/graceful-close-repair.md).
- The first combined checkpoint run exposed `Session::snapshot()` returning
  `Busy` during the final-drain test's nonblocking poll. Parent updated that
  test-only poll to tolerate `Busy` within its original deadline and retain
  failures for other errors; no production change was made for it. The combined
  focused rerun then passed both checkpoint tests.
- The full `scripts/check`, linked Ghostty execution, hosted checks, commit,
  push, and packaged macOS acceptance remain with the parent.

## Hosted follow-up and Swift repair

- `scripts/check` after the one-line Swift return fix passed **756/0/7** in
  256.43 seconds; source fingerprints and unrelated preservation matched.
- Foundation Linux passed at `3b38518`. Foundation macOS failed the real local
  Codex route admission; later provider-hook failures were mutex-poison cascades.
  Log `/tmp/zmux-osc7501-foundation-macos-failure.log`. A bounded owner diagnosis
  is recorded in [macOS route admission diagnosis](references/macos-route-admission-diagnosis.md).
  It identifies the 150 ms CLI route-start cap versus the asynchronous 2 s
  Session checkpoint as the strongest plausible cause; the hosted log does not
  prove the exact checkpoint latency or sole root cause. No Mac acceptance is
  inferred from the Linux pass.
- Ghostty producer `43300002338` failed its old metadata fixture at RIS metadata,
  after fresh ARM library/C ABI compilation and inspection. Failed diagnostic
  artifact `13980787888` was downloaded and API SHA-256/size verified. It is not
  an accepted library artifact; exact failure and versioned harness repair are
  being investigated while historical fixture pins remain unchanged.

## Pre-spawn provider route-start deadline repair

- Only local `ProviderRouteStarted` now uses its absolute CLI deadline, capped
  by the existing 2.7 second `EVENT_BUDGET`. Ordinary lifecycle operations
  remain capped at 150 ms; provider event sends remain at 100 ms. The CLI
  carries one absolute deadline through control connect, writes and response
  reads.
- Desktop stamps the authorized route command deadline before queueing its
  envelope. The real Session checkpoint is bounded by the earlier of its
  existing 2 second cap and that command deadline minus a 250 ms response
  margin. The poller cancels and fails closed at that cutoff or on the existing
  caller cancellation flag. Tracked provider spawn still requires an accepted
  `Started` response; the exact `Cancelled` inverse and one fallback remain.
- A sanitized fallback reason now includes elapsed milliseconds and a coarse
  control-request outcome class, without raw IPC errors, paths, payloads or
  provider data.
- Red/green controls: the CLI delayed-response test fails when Started is
  temporarily returned to the 150 ms helper. The real-PTY Desktop test fails
  when deadline propagation and expiry rejection are temporarily removed.
  Both controls were restored and the final focused tests passed.
- Validation: CLI provider unit tests 3/3; real-PTY Desktop route admission
  1/1; `provider_hooks` integration suite 15 passed, 2 ignored, 0 failed
  (including the exact local Codex Desktop fixture and one-launch fallback);
  strict all-target Clippy for `zmux` and `zmux-desktop-runtime`; workspace
  formatting and whitespace checks. The integration suite preceded only the
  final sanitized diagnostic-text change; final source and test targets then
  compiled under Clippy.
- The real-PTY regression delays Desktop polling 225 ms and verifies exact
  command-epoch alert arbitration from the actual Session checkpoint. It does
  not force the Session worker itself to spend more than 150 ms completing the
  checkpoint. The CLI socket test proves transport budgeting only. No native
  macOS reproduction was available, and the hosted log does not prove the
  checkpoint latency or a sole root cause. `scripts/check` was not rerun; the
  parent owns the aggregate run. Details: [route-start budget repair](references/route-start-budget-repair.md).
