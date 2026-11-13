# Desktop checkpoint PTY fixture repair

**Date:** 2026-11-13
**Scope:** Repair the input-drain handshake in
`provider_observation_and_registration_use_deadlined_live_checkpoints`.
Production session and provider behavior, task status, plan, and delivery remain
with their existing owners.

## Finding

The real `/bin/sh` PTY fixture admits twelve `INPUT_CHUNK_BYTES` writes while
each hold marker is active. It then waited 200 ms, killed a background `cat`,
restored canonical input, printed `PROVIDER_STATE_DRAINED_*`, and waited for the
next line. The timer and marker did not establish that the 49,152 admitted
bytes had been consumed. A remaining raw-mode prefix could interfere with the
next canonical line and prevent the second hold marker.

Foundation run 43337548294 at `b3334d2` failed only this test on macOS. Its log
`/tmp/zmux-osc7501-foundation-b333-macos.log` records the wait timing out at
`sessions_tests.rs:761` with `PTY did not print PROVIDER_STATE_DRAIN_HOLD_2`;
135 tests passed and one failed.

A Linux controlled probe showed why the old timer was not a byte barrier. A
deliberately slow reader with PTY stdin explicitly attached consumed 16,380 of
49,152 bytes before the 200 ms kill, although the parent had written all
49,152 bytes. Linux still reached its second marker, so this did not reproduce
the macOS failure. In a separate `/bin/sh -c` probe, an asynchronous reader
without explicit stdin redirection saw EOF. Neither behavior supplies a
portable completion handshake.

## Repair

The fixture now passes the expected batch size to a foreground Python child.
The child loops on `os.read` until it consumes exactly the expected byte count,
handling short reads, and exits unsuccessfully if the PTY closes early. Only
after that successful read does the shell restore canonical mode and print the
existing `PROVIDER_STATE_DRAINED_*` marker. The count is derived from the same
12-chunk constant used by both test input loops.

The real `/bin/sh` PTY and worker remain in use. The two hold sleeps (2 seconds
and 1 second), timeout values, pending/expiry/cancellation coverage, successful
`execute_authorized` paths, and existing assertions remain unchanged. The
200 ms polling window and timed background-reader kill were removed because
they could not acknowledge byte completion.

## Test-case gate

- **Observable contract:** Provider observation, registration, and receipt
  Events use worker-ordered live PTY checkpoints, with the existing deadline,
  cancellation, and successful public execution behavior.
- **Credible regression:** Unconsumed bytes from the first blocked batch can
  contaminate the next canonical input and prevent the second real PTY barrier.
- **Coverage boundary:** This remains the existing real-PTY Desktop owner test.
  The adjacent route-admission test guards a distinct prompt-epoch and fast-exit
  contract; no duplicate test was added.
- **Production seam:** None added. No test assertion or production behavior was
  removed or weakened.

## Validation

- Pre-change Linux focused PTY test: 1 passed. This did not reproduce the
  hosted macOS failure.
- Post-change focused PTY test: 1 passed.
- Adjacent real-PTY route-admission test: 1 passed.
- `cargo clippy --locked --offline -p zmux-desktop-runtime --all-targets -- -D warnings`:
  passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The macOS Foundation result is the reported pre-change evidence only. This
Linux repair run does not establish native macOS acceptance. Parent-owned
full-check, hosted rerun, and delivery evidence remain pending.
