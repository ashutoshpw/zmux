# Provider route-start deadline repair

**Date:** 2026-11-12
**Scope:** Pre-spawn local `ProviderRouteStarted` admission for automatic Codex and
OpenCode launches. This repairs the concrete CLI/Desktop deadline mismatch from
the [macOS route admission diagnosis](macos-route-admission-diagnosis.md). It
does not establish that this mismatch was the sole cause of Foundation run
`43299857515`, macOS job `131467797030`.

## Deadline contract

- Ordinary lifecycle operations still clamp to `LIFECYCLE_BUDGET = 150 ms` in
  `apps/cli/src/hooks.rs:1181-1206`. Provider event sends still use their
  existing 100 ms connect/write timeouts in `apps/cli/src/hooks/provider.rs`.
- Only `ProviderRouteStarted` uses its caller's absolute deadline, capped by
  `EVENT_BUDGET = 2700 ms` (`apps/cli/src/hooks/provider.rs:914-925`). The
  CLI's `request_until` carries that same deadline through request encoding,
  socket connect, writes and response reads (`apps/cli/src/main.rs:750-769`).
  `Ended`, `Cancelled` and other lifecycle requests continue through the
  150 ms clamp.
- Desktop stamps the route command's absolute deadline before queueing its
  `Envelope`, and waits only for the time remaining to that deadline
  (`crates/desktop-runtime/src/lib.rs:908-948`). Its existing command timeout
  remains 2 seconds. A route checkpoint uses the earlier of its existing 2
  second checkpoint cap and the command deadline minus a 250 ms response
  margin (`lib.rs:2982-3012`). The checkpoint does not renew a deadline after
  queueing.
- The poller checks caller cancellation and the route checkpoint cutoff before
  consuming a ready checkpoint. At the checkpoint cutoff it cancels the
  Session checkpoint and resumes the route command with an error; after the
  caller timeout it retains the existing cancellation-flag path
  (`lib.rs:2584-2627`). A route is still admitted only from the actual Session
  checkpoint result. The CLI still waits for an accepted `Started` response
  before spawning, submits the same route ID as its `Cancelled` inverse on
  failure, and launches one untracked fallback (`provider.rs:354-388`).
- Failed pre-spawn admission now includes elapsed milliseconds and a bounded
  class (`desktop_rejected_route` or `control_request_failed`) in the fallback
  reason. It does not print raw IPC errors, socket paths, request payloads or
  provider data (`provider.rs:354-388`).

## Regressions and proof boundaries

- `route_start_transport_budget_exceeds_the_lifecycle_cap_but_cleanup_does_not`
  (`apps/cli/src/hooks/provider.rs:1087-1165`) delays local-control responses
  by 250 ms. `Started` succeeds; `Ended` and `Cancelled` time out under their
  existing lifecycle budget. This is a transport-budget test only. Its local
  peer returns a framed accepted response and does not prove Desktop admission.
- `provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read`
  (`crates/desktop-runtime/src/sessions_tests.rs:358-599`) uses a real PTY and
  the Session checkpoint API. An already-expired route deadline fails closed,
  and a subsequent OSC report remains alertable. For the valid route, the test
  delays the Desktop checkpoint poll by 225 ms, then observes admission from
  the real checkpoint and verifies that a later report at that command epoch
  is suppressed. This proves real-owner admission after 150 ms and exact-epoch
  alert arbitration. The test delays Desktop polling; it does not force the
  Session worker itself to spend more than 150 ms completing the checkpoint.
- The existing local Codex real-Desktop fixture remains in
  `apps/cli/tests/provider_hooks.rs:2128-2348`; it passed in the focused suite.
  The suite also passed its existing one-launch fallback case. The two helper
  subprocess tests marked ignored are invoked by their parent fixtures.

## Validation

- CLI provider unit tests: 3 passed, including the delayed transport-budget
  regression.
- Desktop real-PTY route admission regression: 1 passed.
- `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture`:
  15 passed, 2 ignored, 0 failed. This run preceded the final sanitized
  diagnostic text change; the final CLI source and all test targets compiled
  under strict Clippy afterward.
- `cargo clippy --locked --offline -p zmux -p zmux-desktop-runtime
  --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Negative controls failed for the intended reasons. Restoring the 150 ms
  helper for `Started` made the CLI regression fail at the delayed response.
  Removing the Desktop caller-deadline cap and expiration check made the real
  PTY regression admit the already-expired route. Both controls were restored;
  the final focused tests pass.
- `scripts/check` was not rerun. The parent owns its aggregate check. The
  available local runner is Linux; no native macOS reproduction was available.

## Hosted evidence limit

The hosted macOS log still proves only the pre-spawn fallback and the control
server's `Broken pipe`. It has no route RPC timing or Session checkpoint outcome.
This repair closes the observed 150 ms versus asynchronous checkpoint budget
mismatch while preserving ordinary lifecycle bounds. It does not prove that the
checkpoint itself exceeded 150 ms in that run or exclude another macOS
scheduling/control delay as the cause.
