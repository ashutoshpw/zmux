# Desktop barrier repair recheck

**Date:** 2026-11-13 UTC
**Baseline:** `87cc88d776f7378856da86d131e98be0c22f9846` (`HEAD`)
**Scope:** Uncommitted changes in `crates/desktop-runtime/src/sessions_tests.rs`,
`crates/desktop-runtime/tests/program_status.rs`, and
`crates/session-runtime/tests/engine.rs`, reviewed against the hosted failure
record and repair description. CLI-provider files and the CLI worker's scope
were not inspected.

## Verdict

**Accepted for this scoped test-fixture repair review; no severity-ranked
findings.** The reviewed changes are confined to the three test files. They do
not add a production seam, and the meaningful deadline, cancellation, success,
alert, selection, and Session publication assertions remain in place.

This is not native macOS acceptance. The review and focused execution ran on
Linux; the after-fix macOS Foundation and linked Session runs remain unverified.

## Evidence

- `sessions_tests.rs`: the child opens a test-owned FIFO before reporting
  readiness, then waits on it at both input-drain barriers. The test queues
  twelve 4,096-byte PTY writes and routes actual `Desktop::execute_authorized`
  envelopes through `Worker::command`. For the two concurrent public lifecycle
  calls, it verifies each adds a real Session checkpoint, polls the production
  checkpoint queue while asserting its length remains two for 300 ms, releases
  the child, then polls until both checkpoints complete. The timeout, expiry,
  cancellation, successful provider observation, Event receipt, no-late-state,
  no-alert, and exactly-one-alert assertions remain. The old fixed 2 s / 1 s
  child release sleeps are removed; this does not rely on a scheduler-only
  delay or a replayed checkpoint fixture.
- The FIFO is created under a per-test mode-0700 directory with mode 0600.
  The parent opens the writer after child readiness and supplies one release
  token at each barrier. The scoped public callers are joined, and the success
  path completes both FIFO waits before the existing Desktop shutdown path
  closes the Session. Darwin execution of the shell's `<>` FIFO open remains
  part of the unverified native gate.
- `program_status.rs`: the shell command now waits on `read` after emitting the
  OSC report, keeping the command active through projection, exact target
  selection, and stale-target rejection. The test sends a newline afterward
  and waits for request 5. Record, alert-feed, sound-origin, wire source,
  session epoch, exact-selection, and stale-target assertions are retained.
- `engine.rs`: both blocked Session cases take their comparison snapshot after
  input admission and before query emission. This accommodates a legitimate
  scroll-to-bottom publication during input admission while retaining the
  generation, content, title, status, deadline/close, and input-order checks.
- The test names are unchanged. `scripts/check` runs unfiltered
  `cargo test --workspace --locked`; the macOS workflow also invokes
  `scripts/check-native-sidebar`, whose OSC receipt requirements still include
  `osc_7501_emitted_through_shell_pty`, `program_status_exact_owner`,
  `program_status_available`, `alert_feed_present`,
  `focused_terminal_full_records`, and exact-selection/coexistence keys.

## Focused validation

All commands used an isolated `/tmp/zmux-desktop-barrier-review-target` and
passed on Linux:

- `cargo test --locked --offline -p zmux-desktop-runtime provider_observation_and_registration_use_deadlined_live_checkpoints` — **1 passed**.
- `cargo test --locked --offline -p zmux-desktop-runtime --test program_status public_desktop_projects_live_osc_alert_and_selects_only_the_exact_program` — **1 passed**.
- `cargo test --locked --offline -p zmux-desktop-runtime provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read` — **1 passed**.
- `git diff --check 87cc88d --` the three reviewed files — **passed**.

The optional Ghostty-linked `session-runtime` engine tests and actual after-fix
macOS Foundation/native gates were not run. No full `scripts/check`, workflow,
commit, push, source edit, or index edit was performed.
