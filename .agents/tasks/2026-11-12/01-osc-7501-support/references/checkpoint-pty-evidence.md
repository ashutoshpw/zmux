# Program Status checkpoint PTY evidence

**Date:** 2026-11-12

**Scope:** one production-owner contract test in `crates/session-runtime/tests/program_status.rs`; no production changes.

## Authoring gate

1. **Observable contract:** a worker-ordered asynchronous checkpoint must not return a current-looking snapshot while earlier admitted PTY input and the resulting causal query/reply exchange have not completed. A queued deadline or explicit cancellation retires the unserviced request without a later successful stale snapshot. A new live checkpoint succeeds after the PTY work resumes and reports the current command/status.
2. **Credible regression:** a checkpoint is processed ahead of earlier tracked input, a deadline/cancel flag is ignored while its request waits behind blocked work, or the caller cannot obtain a current checkpoint after the child resumes.
3. **Existing coverage gap:** `default_real_pty_flushes_admitted_input_before_causal_query_replies` verifies the child's exact input-then-reply byte stream and final status, but it does not exercise `ProgramStatusCheckpoint` deadline, cancellation, or recovery after unserviced requests.
4. **Production seam:** none. The test uses a real default Session/PTy, the public tracked `InputReceipt`, the public checkpoint handle, and child-owned file signals. It adds no parser accessor, test flag, or injected receipt.

## Scenario and assertions

The Python child reports readiness and waits for a file signal. The test admits bounded 4096-byte chunks one at a time, retaining each receipt until complete. It stops at the first receipt still incomplete after the bounded observation window; all earlier receipts have completed. This is the public observation that the local PTY write is backpressured while the child has not started reading.

The child then writes an OSC 133 prompt boundary, an OSC 7501 working report, the feature query, and DA query, and writes a marker only after the PTY write returns. It does not read stdin until a separate release file appears. While the tracked input receipt remains incomplete, the test observes an explicit checkpoint timeout, queues and cancels another checkpoint, and verifies neither request succeeds before release.

After release, the child compares the complete admitted input byte sequence followed by the exact feature-detection and DA reply bytes. It remains alive after that check. The test waits for the child's ordered marker, confirms both unserviced handles are retired as cancelled, and obtains a fresh successful checkpoint containing the new command epoch and working record. It then releases the child, observes exit, and joins the Session worker.

## Validation

- `cargo test --locked --offline -p zmux-session-runtime --test program_status`: 6 passed.
- `cargo fmt --check -p zmux-session-runtime`: passed.
- `cargo clippy --locked --offline -p zmux-session-runtime --all-targets -- -D warnings`: passed after changing `wait_for_file` to take `&Path`.
- `git diff --check -- crates/session-runtime/tests/program_status.rs`: passed.
- Isolated temporary source export with checkpoint admission changed to return the current shared snapshot immediately: the new test failed at the intended “checkpoint returned before admitted PTY input and its causal reply completed” assertion. The production checkout was not changed for this negative control.

## Proof limits

The tracked receipt proves the input write is incomplete at the PTY boundary; the child marker proves it emitted the query before the checkpoint request. The test does not inspect Session's private parser or assert the instant the worker turns those bytes into an internal causal-reply object. After release, the child's exact byte comparison proves that the causal replies follow the admitted input, and the fresh public checkpoint proves current status publication after that exchange.
