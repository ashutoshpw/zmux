# OpenCode hosted setup failure investigation and bounded repair

**Date:** 2026-11-13
**Source under investigation:** `87cc88d776f7378856da86d131e98be0c22f9846`
**Scope:** CLI OpenCode local setup diagnostics and the real-Desktop `provider_hooks` fixture.

## Hosted observation

Foundation Linux job `131613628162` in run `43342408930` failed first in
`automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration`
at `apps/cli/tests/provider_hooks.rs:2689` in the hosted source. After the test
sent `opencode --session session:root/opaque`, its ten-second host wait saw
only `LOCAL_OPENCODE_SHELL_READY`; it did not see the fixture host's
`{"ready":true}`. The PTY then showed the OpenCode setup fallback and one
`OPENCODE_FALLBACK_LAUNCHED` marker. The retained evidence is in
[`hosted-final-87cc88d.md`](hosted-final-87cc88d.md) and
`/tmp/zmux-osc7501-87cc-foundation-full.log`.

The eleven subsequent `PoisonError`s are cascades. `Directory::new()` holds
`PROCESS_FIXTURE` at `provider_hooks.rs:641`; the first failing test panicked
while holding that guard, so later tests failed to acquire it. They do not
identify additional provider defects.

## Setup boundary and limits of the hosted evidence

The diagnostic reason identifies `opencode::prepare_launch` as the failing
boundary. Invocation-directory creation and the private event listener have
already succeeded; otherwise `provider.rs` selects their separate fallback
reasons. `prepare_launch` runs before the `ProviderRouteStarted` request and
before the tracked OpenCode process is spawned. This failure therefore does
not exercise the CLI's 100 ms provider-event socket send, 150 ms ordinary
lifecycle budget, or 2.7 second provider-route budget. The OpenCode `--help`
preflight has its own existing two-second child deadline.

The historical log cannot identify the precise failing operation. At that
source, the caller discarded the `prepare_launch` error and printed only
`OpenCode session plugin setup failed`. The output contains no preflight exit
status or duration and no invocation-config filesystem state. It also contains
no Linux runner CPU/load, `/tmp` pressure, or process interleaving evidence.
Possible failure points inside `prepare_launch` include the bounded `--help`
preflight, private config/plugin directory creation, user-config symlink
mirroring, plugin write/permissions, and root-session validation. Treating one
as the hosted root cause would exceed the evidence.

The local environment snapshot during investigation was 24 CPUs, load averages
`2.58, 2.65, 2.77`, 57 GiB available memory, and 266 GiB free on `/tmp`. The
unmodified exact OpenCode test passed locally in 1.32 seconds. These are local
measurements and do not describe the hosted Linux runner.

## Bounded changes

- Setup failures now carry a fixed stage label through the fallback path and
  include elapsed milliseconds. Labels cover command eligibility, standalone
  help initialization/execution/status/option detection, private config and
  plugin directory creation, user-config mirroring, plugin installation and
  permissions, and root-session validation. Diagnostics contain no paths,
  config values, raw OS errors, plugin contents, or provider data.
- The two-second preflight, config mirroring via symlinks, plugin permissions,
  single fallback, and original provider arguments are unchanged.
- The fixture's readiness marker now runs from Bash's first `PROMPT_COMMAND`,
  after the private Bash RC sources the user RC and calls
  `zmux_install_program_status_prompt` (`crates/session-runtime/src/shell_integration.rs:372-379`).
  The previous marker was printed before that final shell-startup step, so it
  could report readiness while startup was still in progress.
- The existing real-Desktop test now delays only its synthetic OpenCode `--help`
  dependency for five seconds. It observes the help child start, confirms the
  child does not reach its completion marker before the existing two-second
  deadline, and verifies one original-argument fallback, unchanged user config,
  and no fabricated lifecycle session. The resulting sanitized stage is
  `run_standalone_help_preflight`; elapsed time is reported for future hosted
  diagnosis.

The controlled delay reproduces a credible bounded-preflight failure path; it
does **not** prove that the historical hosted failure occurred in that stage.
No deadline was increased and no retry, config replacement, or test removal was
introduced.

## Test authoring gate

1. **Observable contract:** A delayed OpenCode help preflight must not block
   beyond its existing bound. The original command runs once with its original
   arguments, user configuration remains unchanged, and no tracked lifecycle
   session is fabricated.
2. **Credible regression:** Removing or bypassing the child deadline would let
   the five-second help process finish and proceed into a tracked launch rather
   than the expected single fallback. The child start/completion markers also
   guard cleanup of the bounded preflight process.
3. **Coverage gap:** Existing `provider_hooks` coverage exercised a successful
   plugin host and an unsupported `--standalone` flag, but did not delay the
   real preflight child or distinguish a preparation stage. The regression is
   kept at the existing real-Desktop CLI boundary rather than duplicating the
   same fallback contract in a helper test.
4. **Production seam:** None added. The delay switch and marker paths exist
   only in the synthetic provider fixture. Production diagnostics use a fixed
   stage label and monotonic elapsed time.

## Validation

- Baseline before the change:
  `cargo test --locked --offline -p zmux --test provider_hooks automatic_local_opencode_tracks_exact_root_and_preserves_user_configuration -- --exact --nocapture`
  — 1 passed in 1.32 seconds.
- Updated owner boundary:
  `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture`
  — 15 passed, 0 failed, 2 ignored in 41.80 seconds, including the controlled
  delay and exact local OpenCode route.
- `cargo clippy --locked --offline -p zmux --all-targets -- -D warnings` — passed.
- `rustfmt --edition 2024 --check apps/cli/src/hooks/provider.rs apps/cli/src/hooks/provider/opencode.rs apps/cli/tests/provider_hooks.rs` — passed.
- Scoped `git diff --check` for the four owned implementation, test and fixture
  paths — passed.

## Handoff and proof boundary

The exact stage of the historical hosted failure remains **not established**.
The new diagnostic will distinguish it on a fresh run. The local controlled
preflight case validates one plausible failure mode and the corrected shell
readiness barrier; it is not a hosted rerun or proof of the original cause.
`scripts/check`, hosted workflows, commit/push, app build and delivery remain
with the parent task owner. Unrelated status, Desktop and Session test edits
were preserved.

## Parent resolution of review coverage finding

The independent review accepted the production diagnostic scope and found the
missing completion marker insufficient to prove termination or timing. Parent
strengthened the same existing fixture: its start marker now records the actual
synthetic help PID, and the test requires an `ESRCH` liveness probe before
fallback. It also parses the sanitized elapsed field and requires 1,900–2,700 ms
for the two-second timeout case. The upper bound allows response/cleanup overhead
within the existing hook budget and rejects a widened three-second preflight;
it is not a claim of exact scheduler timing.

- Enhanced focused test passed in 3.43 seconds with the original two-second
  production preflight. Log: `/tmp/zmux-osc7501-opencode-cleanup-proof-green.log`.
- A temporary three-second deadline failed at the timing assertion with
  `elapsed_ms=3015`; original source was restored immediately. Log:
  `/tmp/zmux-osc7501-opencode-deadline-proof-red.log`.
- The restored final source is running the aggregate check. No subprocess is
  signalled by the liveness assertion; it only queries whether the PID exists.
