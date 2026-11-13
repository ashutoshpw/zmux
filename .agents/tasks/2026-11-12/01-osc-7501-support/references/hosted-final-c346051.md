# Exact-source hosted acceptance: `c346051`

**Source:** `c34605102c19380315c3b262686d26f7d57c6c1b` on `main`
**Scope:** read-only hosted checks; no retries, workflow dispatches, source edits, local checks, commits, pushes, or app operations.

## First primary failure

Foundation run [43360964670](https://github.com/ashutoshpw/zmux/actions/runs/43360964670) checked out the requested SHA. Its `foundation (macos-latest)` job `131674106406` failed in **Check existing Rust modules and foreign C ABI** while running `scripts/check` on the arm64 macOS 26.7.2 runner.

In `apps/cli/tests/provider_hooks.rs`, the first failing test was `automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration`: line 593:43 unwrapped `Err(Busy)`. The poisoned shared lock then caused the other 11 failures at line 641:44 (`PoisonError`). The suite summary was **3 passed, 12 failed, 2 ignored**. This is a provider-hooks failure, not the intentional mutation-child EOF case; the preceding `execution_admission` tests passed.

The complete failed macOS job log was retrieved and retained at `/tmp/zmux-osc7501-final-c346051/foundation-macos-job-131674106406-full.log` (54,071 bytes, SHA-256 `acd0776c6d8f4a156ac86fdebd2db379cbd00acecae5bafa1ec53f944dff4544`, 688 lines). The Foundation run's artifact list was empty. No root cause is inferred from the `Busy` result.

At the evidence snapshot, the Ubuntu Foundation job was still in progress. No retry was made and no remaining Foundation jobs were awaited after the macOS failure.

## Other requested run snapshots

- Universal macOS run [43360964642](https://github.com/ashutoshpw/zmux/actions/runs/43360964642) was still in progress. `swift-typecheck` and both Linux scheduler jobs had passed; the `development` job was at **Check Swift fixture cleanup boundary**. Universal app compilation, native suites, artifact upload, and build receipt were not yet observed. No final artifact is accepted by this evidence.
- Ghostty linkage run [43360978497](https://github.com/ashutoshpw/zmux/actions/runs/43360978497) completed successfully for this SHA, with `Session=true` and `native-host=false` as specified. Its detailed test-count, receipt-pin, and per-architecture compile evidence was not collected after the first primary failure, so those acceptance details remain unverified here.

This fail-fast pass does not complete task delivery or the final artifact gate. The supplied local `scripts/check` result (763/0/7, 285.75 seconds) was not rerun or independently revalidated.

## Parent completion of successful Ghostty artifact verification

The fail-fast pass above stopped before artifact verification. Parent subsequently
verified run `43360978497` and artifact `14007902860` independently. The actual
22,998,362-byte archive matches API SHA-256
`616532a3d83541d36fa447498f590b42582e1ac4a1caabbcdaccbdb1ad509ea0`.
ZIP CRC, bounded extraction and all 229 receipt source pins match the exact
`c34605102c19380315c3b262686d26f7d57c6c1b` commit. Repository-relative source
aliases were normalized and constrained for Git blob lookup.

ARM execution passed engine 4/4, linked Program Status 1/1, Session 4/4 and
linked real-PTY Program Status 1/1. Both Apple targets' engine, Program Status,
Session and Session Program Status compile/link phases passed. The child observed
all 53,248 pre-query prefix bytes before its DSR reply. All phases exited zero.
Intel runtime and native host/AppKit linkage were not requested or claimed.

Actual receipt/logs and parent verification are retained under
`/tmp/zmux-osc7501-final-c346051/linkage-artifact/` and
`/tmp/zmux-osc7501-final-c346051/linkage-parent-verification.json`. Packaged-app
acceptance remains a separate pending gate.

## Parent universal workflow follow-up

The universal macOS run `43360964642` subsequently failed in `scripts/check`
at `checkpoint_during_final_drain_keeps_pre_retirement_state`: the returned
checkpoint had `bytes_read == 0`. The complete log is retained at
`/tmp/zmux-osc7501-c346-development-full.log`. Swift typechecking and both Linux
scheduler jobs passed. App compilation, packaging and native Programs acceptance
were not reached. The fixture held the worker until after child exit, so unread
PTY output retention across last-slave close is under investigation; this log
alone does not prove that kernel behavior was the sole cause.
