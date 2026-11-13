# Independent OpenCode hosted-setup recheck

**Date:** 2026-11-13 UTC
**Compared against:** `87cc88d776f7378856da86d131e98be0c22f9846`
**Scope:** Read-only review of the local changes in `provider.rs`, `provider/opencode.rs`, `provider_hooks.rs`, and the synthetic OpenCode runner fixture, plus the task repair and hosted-final records.

## Verdict

The changed production path matches the stated diagnostic and fallback scope. I found no introduced lifecycle ordering, configuration preservation, or error-detail leak. One test-proof gap remains: the controlled delay case does not independently prove child termination or enforce the exact two-second deadline. This is a coverage finding, not evidence that the current production timeout is wrong.

## Finding

### P2 — The delayed fixture does not prove the stated deadline and cleanup contract

At `apps/cli/tests/provider_hooks.rs:2943-2968`, the test waits for the fallback call and then checks that the five-second fixture's completion marker is absent. Since it makes that assertion immediately after fallback, the marker would also be absent if the synthetic child were still running and had not yet reached five seconds. The test does not parse or assert a bound on the reported elapsed value, or check child liveness after its scheduled completion time. A timeout increased from two seconds to a value below five seconds would also pass this assertion.

The production preflight still passes `Instant::now() + Duration::from_secs(2)` to `bounded_command::capture` at `apps/cli/src/hooks/provider/opencode.rs:96-103`. The existing `Owned::drop` path calls `finish()` synchronously and logs cleanup failure at `apps/cli/src/bounded_command.rs:212-217`; that cleanup implementation is outside this diff. The source inspection supports the current behavior, while the new test does not protect the exact bound or prove cleanup. Tighten the assertion before describing this case as verification of deadline cleanup.

## Verified behavior

- Setup failures use fixed `&'static str` stage labels and elapsed milliseconds in `apps/cli/src/hooks/provider.rs:322-340` and `apps/cli/src/hooks/provider/opencode.rs:17-23,25-127`. The original `CliError` is discarded at stage mapping; no path, config value, raw OS error, plugin content, or provider data is formatted into the setup reason.
- `prepare_launch` completes before the `ProviderRouteSignal::Started` submission at `apps/cli/src/hooks/provider.rs:351-365`. A setup failure stops and joins the listener, removes the invocation directory, and falls back before that route-start point. The same cleanup calls were present in the compared baseline. Their join/removal results remain ignored, so this change does not add new cleanup-error suppression.
- The preflight retains the two-second deadline at `apps/cli/src/hooks/provider/opencode.rs:94-103`. Config and plugin mirroring remain symlink-based at `apps/cli/src/hooks/provider/opencode.rs:129-168`; the normal managed launch asserts config and user-plugin symlinks, preserved contents, one installed plugin, and mode `0600` at `apps/cli/tests/provider_hooks.rs:2700-2719`.
- The delayed fallback asserts exactly one additional launch with the original `--session` arguments, preserved user config and plugin, no Zmux plugin, one existing lifecycle session, and the sanitized `run_standalone_help_preflight` stage at `apps/cli/tests/provider_hooks.rs:2930-2992`. The fixture delay controls occur only in `provider-opencode-runner.mjs` and this integration test.
- The readiness marker is appended to `PROMPT_COMMAND` in the test RC at `apps/cli/tests/provider_hooks.rs:2593-2610`. The generated private Bash RC sources the user RC and then installs the program-status prompt at `crates/session-runtime/src/shell_integration.rs:372-379`, so the marker runs at the first prompt after that startup step.
- The hosted failure evidence is from the unmodified baseline SHA and does not identify which `prepare_launch` operation failed. The repair note correctly keeps historical causation unproven. This review ran no hosted workflow and makes no claim that the local change resolves the historical hosted failure.

## Validation performed

- `cargo test --locked --offline -p zmux --test provider_hooks automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration -- --exact --nocapture` — passed, 1 test, 3.29 seconds.
- No `scripts/check`, workflow, commit, push, source edit, or index edit was performed. This recheck note is the only authored repository file from this review.
