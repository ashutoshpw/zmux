# OpenCode help-preflight fixture recheck

**Date:** 2026-11-13 UTC
**Reviewed source:** `ab789718c1cea30b8cf610f9839bb1b1b1b665ad` plus the scoped fixture change
**Scope:** Read-only review of `apps/cli/tests/provider_hooks.rs`, the named diagnosis and repair notes, and the existing OpenCode Node runner and owner assertions. The active `FilePanelSmoke.swift` work was excluded.

## Verdict

No material findings in the scoped fixture change. The generated wrapper implements the stated narrow fast-help behavior, preserves the Node path and original arguments for delayed help and non-exact-help calls, and does not weaken or remove the existing provider-hook assertions. This is source-level acceptance for parent review, not native or hosted acceptance.

## Findings

None.

## Evidence

- At `apps/cli/tests/provider_hooks.rs:2598-2604`, the wrapper shortcut requires exactly one argument equal to `--help` and an unset `FIXTURE_OPENCODE_HELP_DELAY_MS`. `${FIXTURE_OPENCODE_HELP_DELAY_MS+x}` is POSIX parameter expansion: unset expands empty, while set-empty and set-nonempty expand to `x`. Thus set-empty and delayed help continue through the original `exec node ... "$@"` path.
- The support test compares `${FIXTURE_OPENCODE_STANDALONE_SUPPORT-}` only with the literal `0`; every other value, including unset and empty, emits the supported `--standalone` help line. Both `printf` branches match the help bytes in `crates/session-runtime/tests/fixtures/provider-opencode-runner.mjs:14-19`.
- The final `exec node "$FIXTURE_OPENCODE_RUNNER" "$FIXTURE_OPENCODE_PLUGIN_HOST" "$FIXTURE_OPENCODE_ROOT_SESSION" "$FIXTURE_OPENCODE_REPORT" "$@"` remains intact. Quoted `"$@"` preserves argument boundaries, empty arguments, and spaces for all calls reaching Node. The fixture's runner still receives its four wrapper parameters before the provider arguments, as before.
- The production preflight remains unchanged and still invokes `--help` with `Instant::now() + Duration::from_secs(2)` at `apps/cli/src/hooks/provider/opencode.rs:94-103`.
- The existing real-Desktop owner test remains the primary proof boundary: supported launch and exact arguments/config/plugin preservation at `provider_hooks.rs:2697-2741`; unsupported behavior using support value `0`, original-argument fallback, and no fabricated lifecycle at `:2895-2949`; 5,000 ms delayed help, original-argument fallback, child PID termination, missing completion marker, 1,900–2,700 ms elapsed assertion, unchanged lifecycle count and preserved config at `:2951-3050`; and ordinary help output after unsetting the delay at `:3052-3075`.
- The scoped source diff is one changed line containing the generated wrapper; no existing assertion was removed or altered. The diagnosis and repair notes correctly distinguish the observed near-deadline hosted failure from the unproven Node-startup hypothesis.

## Proof limits

This review inspected source and did not run tests or checks. The repair note reports `provider_hooks` 15 passed, 0 failed, 2 ignored, formatting, strict Clippy, and a temporary argv/environment dispatch control; those results were not independently rerun here. The temporary control reportedly covers set-empty dispatch, while the permanent owner test's 5,000 ms case exercises the set-nonempty delayed-child behavior. No additional permanent test is needed solely to assert that Node was skipped for undelayed help, since that is an internal fixture dispatch detail and the observable owner contracts remain covered.

This review does not establish the cause of the historical hosted failure, a fresh hosted result, native acceptance, or app/build acceptance. No workflow, build, `scripts/check`, commit, or push was run.
