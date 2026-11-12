# Graceful-close dirty-frame repair

**Date:** 2026-11-12

**Scope:** `crates/session-runtime/src/lib.rs` periodic frame publication after
graceful close aborts the staged terminal. The existing shutdown fixture already
owns the observable deadline/forced-cleanup contract, so no shutdown test change
was needed.

## Cause

`Worker::run` aborts the staged terminal as soon as it observes close. Under the
graceful policy it then continues reading the PTY until the existing close
deadline, counting every read while skipping terminal parsing. A `dirty` frame
bit set before close can survive that transition. When its normal frame timer
expires, `publish(..., refresh_viewport = true)` calls
`TerminalOwner::refresh_viewport`; the engine rejects refresh after abort with
`terminal owner unavailable; explicit session recovery required`. That replaces
the required graceful-deadline failure even though the retained child is still
reaped and the output flood remains bounded.

The pre-fix exact shutdown fixture reproduced this failure:

```text
cargo test --locked --offline -p zmux-session-runtime --test shutdown \
  graceful_deadline_escalates_only_the_retained_child_and_reports_forced_cleanup \
  -- --exact --nocapture
```

It returned the terminal-owner error instead of the graceful deadline after
reading 3,514,368 bytes. The integrated round-three log recorded the same error
after 3,444,736 bytes at `crates/session-runtime/tests/shutdown.rs:310`.

## Repair

On each graceful-close iteration, the worker retires the obsolete dirty-frame
flag after aborting the terminal. This keeps close polling on the existing
bounded idle/deadline interval instead of a zero-duration frame timer. Periodic
publication also requires `self.close_deadline.is_none()` as a guard against
refreshing an aborted owner. The worker continues its existing bounded PTY
reads, byte accounting, shared-deadline handling, signal escalation and
physical cleanup without parsing or refreshing the aborted terminal. Ordinary
live-session frame publication is unchanged.

Successful close still uses the existing terminal-close status snapshot path,
which clears program records without refreshing the aborted engine. The failure
path still publishes the existing graceful-deadline error and last committed
viewport.

## Regression and proof

- **Red:** the exact shutdown fixture failed before this repair with
  `terminal owner unavailable` after 3,514,368 bytes; its retained-child and
  cleanup diagnostics showed the child had been reaped.
- **Green:** the same exact fixture passed after the guard. Its existing
  assertions remain intact for graceful-deadline error, finished worker, failed
  status, positive drained-byte count and child reaping. All three shutdown tests
  passed in the focused integration suite, including cleanup-failure reporting
  and successful graceful close with nested child/resource retention.
- **Related focused checks:** lifecycle 17/17, Program Status 6/6, shell
  integration 13 passed / 2 ignored, and child ownership 2/2. Strict Session
  library Clippy, package formatting and scoped `git diff --check` passed.
- An initial combined focused run passed the preexisting-output checkpoint test
  but the final-drain test stopped when `Session::snapshot()` returned the
  documented `SessionError::Busy` from its nonblocking `try_lock`. The parent
  updated that test's polling to continue on `Busy` within its original deadline
  and fail on other errors. No production code changed for that adjustment. The
  combined focused rerun then passed both checkpoint tests.
- Final combined command passed all 44 tests with 2 ignored across the Session
  library, shutdown, lifecycle, Program Status, shell integration and child
  suites.

The full `scripts/check`, linked Ghostty execution, hosted checks and packaged
macOS acceptance were not run here; they remain with the parent. These focused
Linux checks do not establish linked-engine or native-app behavior.
