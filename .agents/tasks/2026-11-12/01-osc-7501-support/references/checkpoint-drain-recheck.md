# Session checkpoint drain recheck

**Date:** 2026-11-12
**Scope:** read-only review of the Session checkpoint barrier in
`crates/session-runtime/src/lib.rs`, its private real-PTY regression, and the
public timeout/cancel contract in `crates/session-runtime/tests/program_status.rs`.
The only write is this review record. Shared `main` and its dirty source/index
were preserved. Desktop route retirement was outside this review.

## Verdict

**The unread-PTY ordering repair is accepted for its stated interleaving, with one
remaining P2 in checkpoint publication during final drain.** The previous
already-readable-bytes finding is closed in the reviewed code. The checkpoint
can still return a snapshot marked `Exited` before the worker commits the
child-exit transition that retires live program records.

## Finding

### P2: A final-drain checkpoint can publish `Exited` with unretired program records

When the pending checkpoint has no deferred actions, pending PTY write, or
causal reply, `service_pending_checkpoint` permits service after a read drain or
EOF (`crates/session-runtime/src/lib.rs:1913-1918`). It selects `State::Exited`
as soon as `exit_seen` is set (`1925-1934`). The read loop calls this service
before the `run` exit condition (`1578-1590`). Only after `run` returns does
`execute` call `terminal.attached_child_exited()`, commit its batch, and publish
the final snapshot (`1248-1270`).

Concrete trigger on natural child exit, without a graceful-close request: admit a
checkpoint, then have the PTY child emit its final `state=working` report and
exit. Once those bytes are parsed and EOF is observed,
the checkpoint can be answered with `Snapshot.state == Exited` while the working
record is still present. The later child-exit batch retires it, but the caller
retains the earlier `Arc<Snapshot>`. This conflicts with the final-drain lifecycle
contract. The existing final-drain test verifies the eventual snapshot and event
order (`crates/session-runtime/tests/program_status.rs:299-324`), but does not
hold a checkpoint across that transition.

Complete the child-exit status commit before answering a checkpoint as `Exited`,
or keep the checkpoint state consistent with the pre-retirement snapshot. A
real-PTY regression should request a checkpoint before the child’s final report
and assert that an `Exited` result does not retain a working/blocked record.

## Reviewed paths with no additional finding

- **Previously readable PTY bytes:** checkpoint admission is serialized with the
  worker’s pre-read admission check. The checkpoint remains a barrier while
  reads proceed; each successful read invalidates the drain observation, and
  service waits for deferred output, pending writes, and causal replies to
  finish. Later queued input is not captured past a checkpoint and is resumed
  after it is answered (`lib.rs:1514-1555`, `1759-1770`, `1891-1971`).
- **Writes and causal replies:** successful user PTY writes reset the drain bit;
  a causal reply is created only while parsing newly read output, which has
  already reset it. Service also requires `causal_reply` to be empty. I found no
  path that reuses a prior `WouldBlock` observation across those operations.
- **Bounded output and deadlines:** each read pass is limited to
  `READ_QUANTA * IO_CHUNK_BYTES`; deferred output has a fixed byte cap and
  overflow fails the Session closed. Cancellation/deadline checks run while a
  checkpoint is pending. Close cleanup retires queued and pending handles, and
  the normal final PTY drain remains in the worker’s exit path.
- **Publication and later input:** the checkpoint refreshes and publishes the
  owner viewport before returning an immutable snapshot; dirty-frame coalescing
  is reset after successful checkpoint publication. `load_input` is blocked by
  an active barrier, and `capture_queued_inputs` stops at the first checkpoint,
  so later channel entries remain behind it.
- **Test seam:** the private regression uses the production `Worker::start`,
  production `execute`, public checkpoint request, and a real Python PTY child.
  Its gate is a test-owned channel around worker startup; no production flag,
  injected snapshot, parser accessor, or behavior-changing test hook was added.

## Test audit and proof limits

- Retain `checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty`: its child
  completes the PTY write and signals readiness before the checkpoint is
  admitted. The result asserts bytes were read, the marker was parsed, and
  command epoch 1 was published. The repair record documents the negative
  control: restoring immediate publication made `bytes_read == 0` and failed
  this test for the intended reason. I did not rerun that source-perturbing
  control in this read-only review.
- The public timeout/cancel test exercises real PTY backpressure, no early
  success, cancellation, exact input-before-replies ordering, and a later live
  checkpoint. Its observed timeout may come from the public handle’s local
  deadline check in `ProgramStatusCheckpoint::try_recv`, followed by handle-drop
  cancellation; it does not by itself prove the worker returned its own
  `SessionError::Timeout`. The later successful checkpoint proves the barrier
  eventually clears after release.
- The output-overflow test is a useful fail-closed control, but it does not run a
  continuously writing child with a checkpoint pending. The source bounds that
  path; no checkpoint-specific flood stress was run here. The PTY regression
  proves the forced pre-first-loop scheduling case, not that output written
  after a `WouldBlock` observation is included in the snapshot.

## Validation run for this review

- `cargo test --locked --offline -p zmux-session-runtime --lib checkpoint_tests::checkpoint_waits_for_preexisting_prompt_bytes_on_real_pty` — **1 passed**.
- `cargo test --locked --offline -p zmux-session-runtime --test program_status` — **6 passed**. This includes timeout/cancellation, output overflow, final drain, and explicit close cases.
- No source, test, or index edits. This review did not run full `scripts/check`; the separately reported graceful-close failure was excluded and not reviewed. No commit, push, workflow, app, or Desktop/remote-route review.
