# Hosted OSC 7501 Desktop fixture repair

**Date:** 2026-11-13 UTC
**Scope:** `provider_observation_and_registration_use_deadlined_live_checkpoints`
and `public_desktop_projects_live_osc_alert_and_selects_only_the_exact_program`.
No Desktop production, Session library, CLI, workflow, status, or plan files were
changed.

## Hosted evidence and diagnosis

The exact-head failure record is in
[`hosted-final-87cc88d.md`](hosted-final-87cc88d.md). The Foundation macOS run
failed at the elapsed observation assertion (`sessions_tests.rs:869` in that
source), after the test had reached the live checkpoint path. The universal
development run failed its public Desktop projection wait with revision 5,
request 1, and no worker error (`program_status.rs:76` in that source).
Full captured logs remain at:

- `/tmp/zmux-osc7501-87cc-foundation-full.log`
- `/tmp/zmux-osc7501-87cc-development-full.log`

The Session fixture previously relied on 2-second and 1-second child sleeps.
It queued twelve 4,096-byte writes, but had no handshake proving the child was
still refusing input when public lifecycle calls reached the worker. Under
macOS load, the observation thread could start after the second sleep released
the child, leaving no delayed live checkpoint to account for the asserted
300 ms.

The shell source supports the Desktop failure hypothesis. Interactive `sh` and
`dash` launches receive the managed `ENV` file, which sources `claude.sh`;
Bash-backed `/bin/sh` installs the `PROMPT_COMMAND` that emits OSC 133 A. The
program-status lifetime rule treats that marker as the prompt boundary and
retires blocked records. The test's `printf` could return to that boundary
before the Desktop snapshot projected its record. The hosted evidence reports
no request error, and no production defect was demonstrated. Linux's local
`/bin/sh` did not reproduce the hosted timing behavior.

## Repair

The real Session child now opens a test-owned named FIFO before reporting
readiness. At each backpressure stage it waits on that FIFO before draining the
same exact number of PTY bytes. A test-local controlled worker driver receives
the actual `Desktop::execute_authorized` envelopes, calls the production
`Worker::command`, and inspects the real private pending-checkpoint queue. It
confirms both checkpoints remain pending for the existing 300 ms assertion
interval, then releases the child and polls those actual Session checkpoints to
completion. The fixture's fixed 2-second and 1-second release sleeps were
removed; timeout values, elapsed
assertions, successful checkpoint results, cancellation and expiry checks,
provider Event receipt, alert count, and the adjacent route test remain.

The public projection test now runs `printf ...; IFS= read -r _` as one command.
That keeps the shell command active through the exact target selection and
stale-target negative control. It sends a newline only after those assertions,
allowing the shell to return to its real prompt boundary before shutdown. The
record, alert, native-notification, exact-selection, and stale-row assertions
are unchanged.

## Test-authoring gate

1. **Observable contract:** Registration, provider observation, and receipt
   Events wait for worker-ordered live Session checkpoints and retain their
   deadline/cancellation behavior. The integration test projects the real OSC
   blocked record and alert, selects its exact target, and rejects a stale
   target.
2. **Credible regression:** A time-only release can occur before the public
   calls reach the checkpoint owner, making the elapsed assertion scheduler
   dependent. Returning from `printf` can also emit OSC 133 A and retire the
   blocked record before projection.
3. **Coverage boundary:** The unit test remains the Desktop checkpoint owner's
   real-PTY admission test; the integration test remains the public Desktop
   projection/selection owner. The adjacent route test protects a distinct
   prompt-epoch and fast-exit contract. No duplicate test was added.
4. **Production seam:** None added. The test-local driver observes the actual
   private queue while calling production worker commands, public
   `execute_authorized`, and real Session checkpoints. The integration test
   uses the actual managed shell and Desktop projection.

## Controls and validation

The pre-change focused Session and public projection tests passed locally on
Linux, so neither hosted macOS failure reproduced directly here. Two temporary
negative controls failed for the intended reasons and were then removed:

- Releasing the Session FIFO before the public calls made the real checkpoint
  queue drain during the asserted hold; the test failed at the pending-queue
  backpressure assertion (`left: 1`, `right: 2`).
- Emitting OSC 133 A immediately after the public `printf` made the blocked
  record disappear; the public projection predicate failed with revision 5,
  request 1, and no error, before reaching selection. The fixture was restored
  without changing that predicate.

Final local evidence after restoring both controls:

- Focused provider checkpoint test: **1 passed**, 3.41 s.
- Public Desktop `program_status` test: **1 passed**, 0.14 s.
- Adjacent live provider-route admission test: **1 passed**, 0.33 s.
- `cargo clippy --locked --offline -p zmux-desktop-runtime --all-targets -- -D warnings`:
  passed.
- `cargo fmt --package zmux-desktop-runtime -- --check` and
  `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

These are local Linux results. macOS Foundation and the hosted universal build
were not rerun by this scoped repair; the report does not establish native
macOS acceptance or a production fix.
