# S01 shutdown fixture directory collision

## Evidence and repair plan (2026-10-16)

Native run38827114277 at7b6540e failed before packaged execution in
`graceful_close_retains_wrapper_pty_and_resource_until_nested_cleanup_and_final_reap`.
The first failure is fixture directory creation at `shutdown.rs:28`, OS17
AlreadyExists, while the other two shutdown tests pass. This is not evidence of
a production shutdown timeout or failed physical child cleanup. The retained log
is `/tmp/zmux-native-38827114277-job.log`.

The fixture names use PID and SystemTime nanoseconds, which are not unique across
parallel tests on Darwin. Follow the existing native-notification fixture owner:
add a process-local AtomicU64 sequence to each directory name. Preserve atomic
0700 create, all actual process assertions and cleanup behavior. No runtime code,
test timeout, shutdown policy, dependency or test-only production seam changes.

Validate at the existing three-test owner boundary. An isolated clock-collision
control holds the fixture timestamp constant: old naming must fail AlreadyExists,
sequence naming must pass with the same input, then exact source must pass. This
uses existing process tests, rather than adding a source-string or counter test.
Freeze only this fixture file and reference for parent review/publication.

## Focused validation

An isolated exactfc47e98 checkout runs
`cargo test --locked -p zmux-session-runtime --test shutdown -- --test-threads=3 --show-output`.
Holding only the existing fixture timestamp at0 makes old naming fail OS17
AlreadyExists in the same graceful-close test (two pass, one fails). The identical
fixed-clock input with the atomic sequence passes all three real lifecycle tests.
Restoring the exact unmodified-clock repaired source and recompiling passes all
three again. Scoped rustfmt and whitespace checks also pass. No new test is added;
the existing owner suite exposes this collision at actual directory creation.

Receipt: `target/agent-validation/s01-fixture-collision/receipt.json`, SHA256
`d6be0b71ed7f343cfcd1fd022ae5c816c707b5b821511c687af0669bd77cdbe3`.
Source/negative-control review and actual Darwin rerun remain separate gates.
This test-only fixture repair does not establish a fix for any prior production
graceful-shutdown Timeout or other cleanup failure.
