# Hosted final verification for `87cc88d`

**Date:** 2026-11-13 UTC
**Source:** `87cc88d776f7378856da86d131e98be0c22f9846` on `main`
**Verdict:** No final universal macOS app was produced or accepted. All three requested workflow runs now have terminal outcomes; Foundation, Ghostty linkage, and macOS development build are red.

## Source and task record

- Local `HEAD`, GitHub `main`, and the three workflow heads all resolve to `87cc88d776f7378856da86d131e98be0c22f9846`.
- The task `status.md` still names `b3334d269e1f0cac20a0d4886421d2a43667e155` and earlier pending runs as the current delivery state. It was left untouched under this verification's path boundary.
- The status record reports the preceding full local `scripts/check` result as **759 passed, 0 failed, 7 ignored**. It was not rerun here. The hosted run below independently stopped in its own `scripts/check` step.
- The Darwin repair recheck review at [`darwin-pty-repairs-recheck.md`](darwin-pty-repairs-recheck.md) accepted the scoped source repair against `b3334d2`; it explicitly was not hosted native acceptance. The current linkage run exposes the remaining executed Session failures below.

## Workflow results

| Gate | Run | Result | Executed evidence |
|---|---|---|---|
| Foundation checks | [43342408930](https://github.com/ashutoshpw/zmux/actions/runs/43342408930) | **Failure** | `foundation (macos-latest)` and `foundation (ubuntu-latest)` both failed. |
| Versioned Ghostty linkage | [43342460208](https://github.com/ashutoshpw/zmux/actions/runs/43342460208) | **Failure** | ARM engine and its 7501 test passed; two linked Session tests failed and the linked 7501 real-PTY Session test was not executed. Native host was not requested. |
| Universal macOS development build | [43342408962](https://github.com/ashutoshpw/zmux/actions/runs/43342408962) | **Failure** | Swift type-check and both Linux scheduler helper jobs passed. The development job failed in `scripts/check`, before universal app compilation or native acceptance. |

### Foundation failures

- **macOS job `131613627847`:** `sessions::tests::provider_observation_and_registration_use_deadlined_live_checkpoints` failed at `crates/desktop-runtime/src/sessions_tests.rs:869:5`, with `provider observation must wait for its delayed real Session checkpoint`. Summary: **135 passed, 1 failed, 0 ignored**, exit 101. Captured excerpt: [`foundation-macos-desktop-checkpoint.log`](/tmp/zmux-osc7501-final-87cc88d/foundation-macos-desktop-checkpoint.log).
- **Linux job `131613628162`:** first failure was `automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration`, at `apps/cli/tests/provider_hooks.rs:2689:9`: `automatic OpenCode launch did not reach its plugin host: LOCAL_OPENCODE_SHELL_READY`. The log then showed the untracked fallback launching once and `OPENCODE_FALLBACK_LAUNCHED`. Eleven later failures in that test binary reported `PoisonError` at `provider_hooks.rs:641:44`. Summary: **3 passed, 12 failed, 2 ignored**, exit 101. Captured excerpt: [`foundation-linux-provider-hooks.log`](/tmp/zmux-osc7501-final-87cc88d/foundation-linux-provider-hooks.log).

These excerpts preserve the hosted output and do not infer a root cause.

### Linkage artifact and executed results

The failed linkage run retained artifact `13999298481`, [`terminal-engine-linkage-87cc88d776f7378856da86d131e98be0c22f9846`](https://github.com/ashutoshpw/zmux/actions/runs/43342460208/artifacts/13999298481). I downloaded it from the GitHub artifact API to `/tmp/zmux-osc7501-final-87cc88d/linkage.zip`.

- API size: **16,248,102 bytes**; API digest: `sha256:adaa2b63cc90e07eed4187c30bf5074aa5927a2be5bc5d1ff41720b485df516e`. Downloaded size and digest match.
- Outer ZIP CRC check passed. Bounded extraction rejected absolute/traversal/backslash paths and symlinks; **357 entries**, **49,484,990 uncompressed bytes**. Extracted under `/tmp/zmux-osc7501-final-87cc88d/linkage-artifact/`.
- The receipt records source `87cc88d776f7378856da86d131e98be0c22f9846`. All **229** entries in its `source_sha256` map matched Git blobs at that exact commit. The versioned manifests also match: `source.json` SHA-256 `d19f74d9138b065014b17fb9deef9d9a2e653e6b0fd8c022eef3d1f7868d9cd8`; `linkage.json` SHA-256 `7ddc18b2c12a524358378e46326868c1d83ed88c683bf0f622d6485feaf2e077`.
- The accepted pinned producer input was the already successful run [43315098805](https://github.com/ashutoshpw/zmux/actions/runs/43315098805), artifact `13988885464`, [`terminal-engine-darwin-thin-7bf0864f9155384684f8fe92be8e9c547fdd9add`](https://github.com/ashutoshpw/zmux/actions/runs/43315098805/artifacts/13988885464), **9,322,902 bytes**, SHA-256 `aa232650b06d2c9184e94cfd41f38d80d91f61901726a703aed25b7ade69b855`. It was not rebuilt. The accompanying receipt is bound to candidate `ghostty-7501-v1`; the previously accepted library hashes and producer details are in [`ghostty-produced-artifact-review.md`](ghostty-produced-artifact-review.md).
- Executed on ARM: the engine suite passed **4/4**; `linked_engine_stages_raw_reports_query_prompt_ris_child_exit_and_close` passed **1/1**. Logs: `execute-arm64.log` and `execute-program-status-arm64.log` in the extracted artifact.
- The ARM linked Session suite ran **4 tests: 2 passed, 2 failed**. Both failing backpressure fixtures printed `blocked fixture completed receipts before stage: 0`, then failed at `crates/session-runtime/tests/engine.rs:122:5` with `left: 3`, `right: 2`. Log: `execute-session-arm64.log`.
- The `arm64/session-program-status` binary was built (SHA-256 `ee1ca4669fa46b32d153c0de6fa9d646c184e2d99e4773b062fd43634759a1fb`), but the receipt says its 7501 real-PTY test `linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain` was **not executed**. The receipt reports `optional_session_arm64_executed=false`, `native_host_requested=false`, and `backpressure_acceptance_claimed=false`. Therefore the requested complete linked Session gate did not pass. No Intel runtime or AppKit-linked result is present.

### Universal macOS build failure and artifact state

The development job `131615135839` failed during `Validate Rust core and foreign ABI` / `scripts/check`. Test `public_desktop_projects_live_osc_alert_and_selects_only_the_exact_program` failed at `crates/desktop-runtime/tests/program_status.rs:76:21` after 10.13 seconds with `desktop did not reach expected OSC 7501 state: revision=5, request=1, error=None`. Summary: **0 passed, 1 failed, 0 ignored**, exit 101. Captured excerpt: [`macos-development-scripts-check.log`](/tmp/zmux-osc7501-final-87cc88d/macos-development-scripts-check.log).

The universal app build, packaged-app smoke, native sidebar/system status menu, native Programs acceptance, and final upload were not reached. The finished run's artifact list contains only the scheduler helper/layout artifacts; it contains no candidate app, `zmux-macos-dev-universal-87cc88d776f7378856da86d131e98be0c22f9846`, or downloadable final app artifact. No app ZIP, source/build metadata, minimum OS, signature, notarization state, `native-sidebar-programs.json`, or packaged screenshots were available to verify. No visual inspection was performed. No delivery link can be reported.

## Boundaries

- No workflow retry/dispatch, `scripts/check` rerun, source/index edit, app launch/manipulation, or paid-resource action was performed.
- The temporary downloaded failure artifact and log excerpts are under `/tmp/zmux-osc7501-final-87cc88d/`. The only repository write from this verification is this reference file; unrelated work remains preserved.
- This evidence establishes hosted workflow outcomes on the exact source SHA. It establishes no accepted app build, notarization, Intel runtime, minimum-OS runtime, physical Mac behavior, or production/live-user acceptance.
