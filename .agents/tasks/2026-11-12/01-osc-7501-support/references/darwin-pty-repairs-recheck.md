# Darwin PTY repairs independent recheck

**Date:** 2026-11-13
**Compared:** current worktree against `b3334d2`
**Scope:** Session deferred-read progress, linked Ghostty PTY fixtures, Desktop exact-drain fixture, and the Darwin linkage test-name guard. Review only; no source, test, index, workflow, or task status edits were made.

## Verdict

No blocking source correctness issue found in this bounded diff. The Session progress publication preserves committed terminal state and leaves deferred ordering and deadline handling intact. Desktop's byte counter and checkpoint cache rules remain consistent. One low-severity test-strength qualification is recorded below. This is source review acceptance for the scoped repair, not native Ghostty/macOS acceptance.

## Finding

### Low: abort assertions do not prove a partial receipt or directly observe no replay

[`engine.rs`](../../../../../crates/session-runtime/tests/engine.rs#L371) exercises both ordering-deadline failure and close while input is blocked. [`block_input`](../../../../../crates/session-runtime/tests/engine.rs#L222) confirms real backpressure by requiring an unfinished receipt after the 100 ms quiet interval. The abort branch then checks that queued receipts are not `Complete` and that aggregate accepted bytes are positive at [lines 424-439](../../../../../crates/session-runtime/tests/engine.rs#L424).

Those assertions allow the positive byte total to come entirely from earlier completed chunks while every still-pending receipt finishes `NotWritten`; they do not require a pending receipt to report `Partial` with `0 < bytes_written < total_bytes`. Also, the fixture child waits for the `release` marker before reading input ([lines 203-220](../../../../../crates/session-runtime/tests/engine.rs#L203)); the deadline and close branches never release it and terminate the Session. The later `send_input` rejection proves that new input is not admitted after termination, but cannot directly observe whether queued bytes were replayed to the child during abort cleanup.

The successful sibling test independently observes the all-admitted prefix, DSR reply, semantic input suffix, and trailing `POST` order at [lines 316-369](../../../../../crates/session-runtime/tests/engine.rs#L316), so this is a narrow abort-path proof limit, not evidence of a production replay bug. If the acceptance contract requires specifically proving partial-write abort accounting and no replay to a live consumer, the abort fixture needs an observable receiver or a controlled partial-write owner fixture. Otherwise retain this as a stated coverage limit.

## Positive review evidence

- **Session publication:** the worker increments `bytes_read` on physical PTY reads, defers parsing while earlier writes remain pending, and retains the existing ordering deadline ([`lib.rs`](../../../../../crates/session-runtime/src/lib.rs#L1450), [`lib.rs`](../../../../../crates/session-runtime/src/lib.rs#L1530)). The progress branch runs only when the normal dirty-frame path is not publishing, no causal reply is active, deferred ordering is pending, and `FRAME_INTERVAL` has elapsed ([`lib.rs`](../../../../../crates/session-runtime/src/lib.rs#L1605)). It does not advance the deferred queue or refresh the terminal. The helper copies the last published state, viewport, exit code, and failure while advancing only `bytes_read` ([`lib.rs`](../../../../../crates/session-runtime/src/lib.rs#L2329)). Subsequent deferred processing and normal publication remain responsible for committing terminal output. This avoids per-read snapshot copies; each progress publication still clones the current viewport once per frame interval.
- **Fixture ordering:** `BLOCKED` reports the full query length after writing it. Both linked Session cases wait for `State::Running` and `bytes_read >= before.bytes_read + query_bytes` before adding later input ([`engine.rs`](../../../../../crates/session-runtime/tests/engine.rs#L195), [`engine.rs`](../../../../../crates/session-runtime/tests/engine.rs#L316)). The prefix expectation uses completed receipts plus retained pending receipts, so it counts every admitted 4096-byte chunk. The deadline branch now expects `causal PTY ordering deadline`, matching the pre-reply ordering interleaving in the unchanged worker deadline path.
- **Desktop drain:** the fixture derives its drain count from the same 12-chunk constant used by both write phases. A foreground Python process loops over short reads until the exact count is consumed and exits unsuccessfully on early EOF before the shell emits `PROVIDER_STATE_DRAINED_*` ([`sessions_tests.rs`](../../../../../crates/desktop-runtime/src/sessions_tests.rs#L361)). Both holds, the 2 s and 1 s delays, provider deadlines, and real PTY input pressure remain in place. No production seam or duplicate test was introduced.
- **Desktop cache:** the worker refreshes `Slot.cached` when `bytes_read` changes ([`lib.rs`](../../../../../crates/desktop-runtime/src/lib.rs#L2135)). `advance_checkpoint_snapshot` refuses to replace a cached snapshot whose byte count is newer than the checkpoint ([`lib.rs`](../../../../../crates/desktop-runtime/src/lib.rs#L2783)). Session `bytes_read` is monotonic, and checkpoint snapshots are produced by the same worker after the ordered barrier, so this path does not introduce byte-counter regression. Native/control projection carries the same field.
- **Linkage guard:** `SESSION_TEST_NAMES` now names `blocked_ordering_deadline_or_close_aborts_without_publishing_or_replaying_queued_input`; the linkage runner still requires all four names and an exact four-test success result ([`check-darwin-linkage.py`](../../../../../tools/terminal-engine-artifacts/check-darwin-linkage.py#L40), [execution guard](../../../../../tools/terminal-engine-artifacts/check-darwin-linkage.py#L534)). The rename therefore preserves the executed-test-set binding.
- **Test-case audit:** these are changes to existing production-owner tests. The Session and Desktop tests protect separate ordering/checkpoint contracts. `assert_read_progress_keeps_committed_snapshot` is test-local and checks observable public snapshot fields; no test-only production export, wrapper, or flag was added.

## Validation and limits

- Reran `RUST_TEST_THREADS=2 cargo test --locked --offline -p zmux-desktop-runtime provider_observation_and_registration_use_deadlined_live_checkpoints`: **1 passed**, with the other workspace targets filtered by the name selector.
- The repair reference reports the default Session suite as **44 passed, 0 failed, 2 ignored**; the optional Ghostty `engine` target selected **0 tests** in that local configuration. I did not rerun the broad suite or `scripts/check`.
- The recorded local Ghostty/Darwin-target compile attempt stopped before Session test Rust compilation because host Clang rejected Darwin `-arch arm64` and `-mmacosx-version-min=14.0` under `-Werror`. No local macOS host is available.
- Hosted run `43335306931` and its red logs are pre-repair evidence at source `9048d943eea63ff18218f0ba1b0dbfe158810ccf`. They explain the original interleaving and observed Darwin PTY capacity but do not verify this repaired source. Native linked execution and hosted post-repair checks remain pending; no native acceptance is claimed here.

No workflow, commit, or delivery action was run as part of this review.
