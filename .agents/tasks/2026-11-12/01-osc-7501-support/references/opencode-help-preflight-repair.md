# OpenCode help-preflight fixture repair

**Date:** 2026-11-13 UTC

**Source:** `ab789718c1cea30b8cf610f9839bb1b1b1b665ad` plus this scoped fixture change
**Owned files:** `apps/cli/tests/provider_hooks.rs` and this reference

## Change

The generated `opencode` shell wrapper in
`apps/cli/tests/provider_hooks.rs:2598-2604` now handles only an exact single
`--help` argument when `FIXTURE_OPENCODE_HELP_DELAY_MS` is **unset**. The
wrapper uses shell built-ins to print the same supported or unsupported `FLAGS`
line as the Node fixture and exits successfully. `${VAR+x}` distinguishes an
unset delay variable from a set-but-empty value. The support check treats only
the exact string `"0"` as unsupported, matching the Node fixture's
`process.env.FIXTURE_OPENCODE_STANDALONE_SUPPORT !== "0"` behavior.

If the delay variable is set, including to an empty value, or argv is anything
other than exactly `--help`, the wrapper reaches the original unchanged
`exec node "$FIXTURE_OPENCODE_RUNNER" "$FIXTURE_OPENCODE_PLUGIN_HOST"
"$FIXTURE_OPENCODE_ROOT_SESSION" "$FIXTURE_OPENCODE_REPORT" "$@"` path.
Thus the controlled 5,000 ms help child still exercises the production
two-second deadline and PID cleanup, and all provider invocations preserve the
original Node runner and argv.

No production source, Node runner, test assertion, ignored annotation,
production seam, task plan/status, or index entry changed. This removes
unneeded synthetic Node startup from undelayed help and does not establish that
Node startup caused the historical hosted failure.

## Test authoring gate

1. **Observable behavior:** The synthetic executable continues to advertise
   standalone support by default and reject it only when the support variable
   is exactly `"0"`. Normal provider arguments and configuration continue
   through the Node runner. Any set delay value also continues through Node, so
   the existing 5,000 ms controlled child remains bounded by production's
   two-second deadline, terminated and reaped before one original-argument
   fallback. Existing lifecycle and user-configuration assertions are retained.
2. **Credible regression:** Treating a set-but-empty delay as unset would
   bypass the Node child; broadening the support check could reject valid
   values; or altering the final `exec node ... "$@"` could change the real
   synthetic provider invocation. The existing owner assertions cover the
   externally visible effects of these regressions.
3. **Coverage gap:** The existing real-Desktop `provider_hooks` owner test
   covers supported startup, unsupported help fallback, the controlled slow
   preflight and cleanup, raw help output, user-config preservation, lifecycle
   count, and original-argument fallback. The fixture optimization is in that
   test's generated executable, so no duplicate test was added. There is no
   permanent assertion solely checking that Node was skipped.
4. **Production seam:** None. The change is limited to the generated test
   wrapper and reuses its existing delay and support environment variables.

## Validation

- `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture`
  — passed: 15 passed, 0 failed, 2 ignored; finished in 40.83 seconds.
- `rustfmt --edition 2024 --check apps/cli/tests/provider_hooks.rs` — passed.
- `cargo clippy --locked --offline -p zmux --all-targets -- -D warnings` —
  passed.
- A temporary dispatch control extracted the exact shell string from the
  current Rust source and used a temporary fake `node` in `/tmp`. It verified
  that unset-delay exact help prints built-in output; delay values `""` and
  `"5000"` delegate to Node; non-help arguments delegate unchanged; and only
  support value `"0"` prints unsupported help. The temporary directory was
  removed.
- `git diff --check -- apps/cli/tests/provider_hooks.rs` and
  `git diff --no-index --check /dev/null .agents/tasks/2026-11-12/01-osc-7501-support/references/opencode-help-preflight-repair.md`
  — no whitespace diagnostics. No `scripts/check`, workflow, app build, commit,
  or push was run.

## Proof limits and handoff

The focused owner test and temporary control establish the fixture dispatch,
existing bounded-delay behavior, argv forwarding, and assertions that remain
in the test. They do not reproduce or identify the exact Foundation-runner
cause. The original 2,009 ms stage remains consistent with the production
two-second preflight deadline, while cold Node startup is still only a
hypothesis. A fresh hosted run is required to learn whether this reduces the
observed failure; it would not by itself prove a historical root cause.

The scoped change and evidence are ready for independent parent review. No app
artifact, production acceptance, commit, or push is claimed.
