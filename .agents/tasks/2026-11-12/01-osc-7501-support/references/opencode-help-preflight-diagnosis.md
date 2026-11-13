# OpenCode standalone help preflight failure diagnosis

**Source:** `ab789718c1cea30b8cf610f9839bb1b1b1b665ad`
**Hosted evidence:** Foundation run `43418903359`, Ubuntu job `131863658740`
**Scope:** Read-only diagnosis of the failed local OpenCode integration test and a possible synthetic-fixture repair. This assigned reference is the only authored file; no production/test/workflow/build changes were made and nothing was staged or committed.

## Finding

The hosted failure is at the OpenCode standalone `--help` preflight. The log
records `stage=run_standalone_help_preflight; elapsed_ms=2009`, then the single
untracked fallback marker `OPENCODE_FALLBACK_LAUNCHED`. The test never observes
the managed fixture plugin host's `{"ready":true}` marker and panics at
`apps/cli/tests/provider_hooks.rs:2715`.

The elapsed time is strongly consistent with the preflight reaching its existing
two-second deadline. The production stage intentionally maps every
`bounded_command::capture` error to the same fixed label, however, so the log
does not expose the capture error kind. It does not prove that a timeout, rather
than another error near that boundary, was the exact failure.

The eleven later `PoisonError`s are cascades from this first panic. The test
holds the `PROCESS_FIXTURE` mutex through its `Directory` guard; unwinding at
the failed assertion poisons it, and subsequent tests unwrap that mutex at
`provider_hooks.rs:661`. They are not eleven additional provider failures.

## Source path and observed evidence

- `apps/cli/src/hooks/provider/opencode.rs:25-35` calls
  `standalone_supported` before creating the invocation config directory,
  mirroring user config, writing the plugin, or validating the root session.
  At `:90-127`, it runs the resolved executable with exactly `--help`, passes
  `Instant::now() + Duration::from_secs(2)` to the bounded capture, and
  classifies a successful result by searching help lines for `--standalone`.
  A capture error becomes the fixed `run_standalone_help_preflight` stage.
- `apps/cli/src/bounded_command.rs:246-300` spawns the command in its own
  process group, captures stdout and stderr, and checks the deadline while
  waiting for both child exit and pipe EOF. On the error path, `Owned::drop`
  calls `finish` (`:71-109, :212-217`), which kills the process group and waits
  for the leader. That cleanup can add time after the two-second lookup bound.
- `apps/cli/src/hooks/provider.rs:321-340` passes preflight failure to the
  original-argument untracked fallback. The tracked route is submitted later
  (`:351-365`) and the provider is spawned later still (`:393-413`). Thus this
  failure did not reach tracked lifecycle startup or the fixture plugin host.
- The test constructs its executable as a shell wrapper at
  `apps/cli/tests/provider_hooks.rs:2598-2604`:
  `exec node "$FIXTURE_OPENCODE_RUNNER" ... "$@"`. The interactive shell
  configuration unsets `FIXTURE_OPENCODE_HELP_DELAY_MS` before the first
  launch and exports the runner paths and standalone-support setting
  (`:2615-2631`).
- `crates/session-runtime/tests/fixtures/provider-opencode-runner.mjs:1-3`
  statically imports Node's filesystem, path, and child-process modules before
  the module body runs. It unpacks the wrapper-supplied values and checks for
  `--help` at `:5-7`. The help branch optionally waits for
  `FIXTURE_OPENCODE_HELP_DELAY_MS`, writes its start/completion markers, and
  prints a fixed `FLAGS` listing (`:7-20`). It does not need the plugin host or
  provider invocation path for help.
- For a normal provider invocation the same fixture inspects the invocation
  config and writes its call report (`:23-49`), then prints
  `OPENCODE_FALLBACK_LAUNCHED` for the original untracked arguments or starts
  the plugin host for `--standalone` (`:51-69`). The main test asserts the
  host-ready response and preserved config at `provider_hooks.rs:2712-2741`.
- Existing coverage already exercises unsupported standalone help and
  one-launch fallback (`provider_hooks.rs:2895-2949`). It also sets
  `FIXTURE_OPENCODE_HELP_DELAY_MS=5000`, observes the synthetic child PID,
  requires that PID to be gone before fallback, and checks the reported time is
  within 1,900–2,700 ms (`:2951-3042`). Finally, it unsets the delay and checks
  ordinary `opencode --help` output still contains `FLAGS` (`:3052-3075`).

## Fixture-repair candidate

The source supports a narrow fixture-only optimization: the wrapper can answer
the exact, undelayed `--help` request itself, avoiding a Node process for the
static help text. Have the wrapper take this path only when its argv is exactly
`--help` and `FIXTURE_OPENCODE_HELP_DELAY_MS` is unset. Print the same
standalone-supported or unsupported `FLAGS` line selected by
`FIXTURE_OPENCODE_STANDALONE_SUPPORT`, then exit successfully. If the delay
variable is set, keep the current `exec node ... "$@"` path. Keep that same Node
path for every non-help invocation.

This shape preserves the 5,000 ms controlled child, its PID/completion markers,
and the current production timeout assertion. It also leaves actual synthetic
provider launches and plugin-host invocation on Node. No production timeout,
retry, or production seam is needed. The candidate only removes Node startup
from the synthetic fast-help paths; it is not evidence that startup caused the
hosted failure.

## Fixture authoring gate

1. **Observable behavior:** The synthetic executable advertises standalone
   support when configured, rejects it when configured unsupported, and leaves
   normal provider arguments, invocation config, lifecycle observations, and
   the original two-second preflight contract intact. A delayed help child
   still times out, is terminated and reaped, and falls back to one original
   provider invocation.
2. **Credible regression:** A wrapper branch that misreads help argv or support
   configuration could make the production adapter reject supported mode or
   accept unsupported mode. Routing delayed help through the fast branch could
   silently stop exercising the real bounded child timeout and cleanup.
3. **Coverage gap:** The existing real-Desktop owner test already covers
   supported and unsupported help, the actual managed plugin-host launch,
   config preservation, the controlled five-second delay and child cleanup,
   and raw help output. Keep these assertions at this owner boundary; a second
   unit test that repeats the same synthetic dispatch would be redundant. Do
   not add an assertion whose only purpose is to prove that Node was skipped.
4. **Production seam:** None. Limit the change to the generated shell wrapper
   in `provider_hooks.rs`; retain the existing delay environment variable and
   fixture runner. No production flag, wrapper, or injection hook is required.

## Proof limits and handoff

The log establishes the failing stage, its near-deadline elapsed time, the
fallback, and the first assertion failure. It does not include help stdout,
child status, capture error detail, or runner load/scheduling evidence. Node
module startup is visible in the fixture path but its cold-load duration is only
a hypothesis. The proposed fast-help branch is an evidence-based way to remove
unneeded work from the synthetic successful-preflight fixture; it does not
identify the hosted runner's root cause and is not a verified repair.

No test or workflow was run for this read-only diagnosis. Parent acceptance is
needed before any fixture change; the parent owns follow-up validation and app
delivery.
