# Provider epoch repair recheck

**Date:** 2026-11-12
**Scope:** read-only review of the provider epoch repair in the requested Desktop and Session owners. The only write was this review record. Shared `main` remained dirty; no source, test, or index paths were changed.

## Verdict

**Not accepted for integration yet.** Two concrete P2 ordering defects remain in the asynchronous route path. The specific checkpoint-A/newer-cache-B interleaving raised during review is handled correctly by the target hold and checkpoint observation path; it does not rewind an already-consumed B source lock.

The focused receipts in `provider-epoch-repair.md` report policy 11, real-PTY route 1, public Desktop 1, provider lifecycle 3, and the negative control. I did not rerun them because the parent’s full `scripts/check` was already running. This review does not wait for or claim that full check, hosted checks, or build acceptance.

## Findings

### P2: a checkpoint can run before already-readable PTY output is read

`Session::request_program_status_checkpoint` obtains `input_admission` with `try_lock`, queues an `Input::ProgramStatusCheckpoint`, then wakes the worker (`crates/session-runtime/src/lib.rs:896-930`). The worker only holds that admission lock around each PTY `read` (`1501-1518`). When its deferred-output queue is empty, it calls `load_input()` before entering the PTY read loop (`1384-1423`, `1493-1508`). After `poll` wakes, it drains the wake descriptor and starts another loop; it does not first drain a PTY descriptor that may also be readable (`1626-1637`). The checkpoint publishes the parser’s current state immediately (`1791-1821`).

Concrete trigger: the shell writes OSC 133 A for command B to the PTY, then starts the provider wrapper. If the PTY worker is idle between reads, the provider route request can enqueue and be serviced before the worker reads those already-written prompt bytes. The checkpoint therefore returns command A. `ProviderRouteStarted` uses that snapshot’s `command_epoch` to bind the route (`crates/desktop-runtime/src/lib.rs:3753-3785`). A fast provider event can also be checkpointed before the prompt bytes are read; `provider_alert_allowed` then sees A’s OSC lock and the event is consumed as suppressed (`program_status.rs:579-610`; `lib.rs:3894-3900`). Reading B’s prompt later clears that lock, but cannot restore the already-consumed provider event.

The barrier orders against PTY reads already performed and deferred output already committed. It does not establish that readable PTY output preceding a provider control frame has been parsed. The current real-PTY route test waits until command epoch 1 is already in a published Session snapshot, so it does not exercise this trigger (`sessions_tests.rs:408-415`). A production-owner regression should cover the PTY/control-stream ordering if it can be made deterministic without a test-only seam; otherwise keep this as an explicit acceptance limit and do not simulate the missing ordering with a supplied checkpoint receipt.

### P2: remote reconnect can drop an accepted route ACK and retain its source reservation

For a remote `ProviderRoute::Starting`, Desktop adds the provider route and queues an accepted receipt. A failed receipt enqueue cancels the route, but a successful enqueue is treated as delivered (`crates/desktop-runtime/src/lib.rs:2402-2444`; `remote_agent_lifecycle.rs:181-198`). The receipt queue later drops commands whose captured binding no longer equals the stream’s current binding (`remote_agent_lifecycle.rs:867-907`).

Concrete interleaving: route R starts on stream generation G1; its checkpoint completes, Desktop records R and queues the accepted receipt; before the stream writer drains that receipt, the lifecycle stream reconnects as G2. The queued G1 receipt is discarded. The reconnect path marks G1 routes ended (`lib.rs:2153-2167`; `program_status.rs:365-378`), but `route_active` does not check `ended`, so R still owns later transitions for the same terminal session and command (`program_status.rs:755-767`). The remote wrapper times out and attempts cancellation with its original expected generation; `provider-launch.py:93-106` declines to emit that cancellation after the generation changes. A later OSC alert in the same command is consequently suppressed by a route whose provider never received admission.

This is a cross-stream ACK/cancellation race, not a general failure of route ID or transport-generation equality. A meaningful production-boundary regression should exercise a real framed route start, stream replacement between ACK enqueue and drain, and the timeout/cancel path, then assert that the old route no longer suppresses an OSC alert. A fixture that simply injects a successful receipt after changing the expected binding would not prove the owner’s ordering.

## Reviewed interleavings with no finding

- **Checkpoint A captured, cached Session snapshot B arrives before Desktop polls A:** `begin_provider_checkpoint` is performed synchronously on the Desktop worker after checkpoint admission (`lib.rs:2972-2989`). While the target is pending, ordinary `observe` skips it (`program_status.rs:442-445`), so cached B cannot advance that owner’s source track. `advance_checkpoint_snapshot` preserves the newer cache (`lib.rs:2656-2678`); the checkpoint path copies A into a temporary target frame and observes A (`2637-2645`). It then releases the hold and resumes the matching work (`2649-2650`). The ensuing Desktop publication observes the retained B cache. Thus `provider_alert_allowed` is not asked to reset from an already-admitted B epoch down to A in this interleaving.
- **Prior normalized provider turn A after provider turn B:** the provider observation path carries a provider turn ID and only emits completion when that exact turn is still eligible; a retired runtime/generation is rejected by the provider lifecycle owner. Remote frames additionally require the current stream binding. I found no new epoch-repair defect in those fences. This does not establish correlation for provider sources that do not carry a turn ID.
- **Local and remote route cancel before checkpoint completion:** local caller cancellation is checked before polling the result, and explicit route cancellation removes the matching pending local work. Remote `Ended`/`Cancelled` frames remove matching pending remote route work before the checkpoint is resumed and send a negative receipt. Per-target checkpoint polling preserves earlier pending work before later work. No separate FIFO or ID mismatch defect was found in these paths.
- **Failure fallback:** Session checkpoint admission uses nonblocking `try_lock`/`try_send`; timeout/drop/cancel are reported through the handle. Local route admission fails closed on checkpoint errors. Remote route start receives a negative receipt without a checkpoint, while provider lifecycle events continue through admission without a current-status filter, preserving provider delivery as documented. The repair record reports that Busy, timeout, queue-full, and caller-cancellation were not deterministically exercised.

## Test audit and proof limits

- Keep the policy negative control: it fails specifically when command-epoch reset is removed. Keep the real-PTY route case: it proves route registration uses the worker snapshot rather than a deliberately stale Desktop cache. Keep the public Desktop and provider lifecycle tests for their distinct API and generation contracts.
- The policy hold test and real-PTY route test do not cover either finding above. Do not add a synthetic test that feeds checkpoint A after the policy owner has already consumed B; production holds B while A is pending, so that fixture would assert an impossible ordering.
- Do not add separate mocks for Busy, timeout, cancellation, and queue-full. The meaningful contract is route-start fail-closed behavior, provider-event preservation for the same live owner, and release of the target hold. Add a production-boundary failure case only if an existing Session/PTY or framed-stream setup can force it deterministically without a seam.
- No focused tests were run in this recheck; the results above are source traces plus the focused receipts already recorded by the task. `scripts/check` was running in the shared worktree and is outside this review verdict.
