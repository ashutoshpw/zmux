# Provider route-start deadline repair: independent recheck

**Date:** 2026-11-12

**Reviewed source:** working diff from `284b00f` in `apps/cli/src/{main.rs,hooks.rs,hooks/provider.rs}`, `crates/desktop-runtime/src/{lib.rs,sessions_tests.rs}`, plus the small `apps/macos/Sources/App.swift` JSON cast change.

**Verdict:** The repair closes the original 150 ms versus asynchronous checkpoint mismatch and preserves the ordinary budgets. One P2 deadline race remains at final route admission.

## Finding

### P2: Recheck the deadline and cancellation immediately before admitting a ready route

`Worker::poll_program_status_checkpoints` checks the cancellation flag and route checkpoint cutoff before calling `try_recv` (`crates/desktop-runtime/src/lib.rs:2584-2635`). If the checkpoint is ready, it removes the pending item, processes the snapshot and alerts, ends the checkpoint reservation, and resumes the lifecycle command (`:2636-2685`). The resumed `ProviderRouteStarted` action then adds the provider route (`:3799-3831`). After the pending item is removed, this path does not recheck `reply_deadline` or `cancellation` before adding the route.

A concrete interleaving is: the workspace worker observes the route as before its cutoff, is descheduled for longer than the remaining 250 ms margin, then resumes with a ready checkpoint. `execute_authorized` can time out and set cancellation at the two-second deadline (`:935-947`), but the resumed command can still admit the route without a deliverable acknowledgement. The eventual acknowledgement is a `try_send` whose error is ignored (`:3200-3220`). The CLI correctly sends `ProviderRouteCancelled` with the same target, route ID, runtime ID, and generation before its single untracked fallback (`apps/cli/src/hooks/provider.rs:364-388`), but ignores failure of that 150 ms cleanup request. If that inverse cannot be enqueued or processed, the late route can continue to reserve alert ownership for the current command epoch.

This is a narrow scheduling race, not evidence that it occurred in the Foundation macOS run. Recheck the caller deadline and cancellation at the final `ProviderRouteStarted` owner mutation before `add_provider_route`; reject the action if either has expired. Also roll back that exact route if its acknowledgement cannot be delivered. Together these make admission fail closed when the worker resumes after the checkpoint poll's initial check or misses the response deadline after route mutation.

## Reviewed behavior

- **Deadline nesting and queue delay:** Only `Started` bypasses the 150 ms helper and is capped by the caller timeout and 2.7 s event budget. `request_until` carries one absolute deadline across socket connect, writes, response reads, and decode. Desktop stamps its two-second route deadline before queueing, waits only for the remaining time, and caps the Session checkpoint at the earlier of its two-second limit or the Desktop deadline minus 250 ms. Delayed queue handling therefore shortens the checkpoint window instead of renewing it.
- **Failure cleanup and spawn ordering:** Expired/cancelled pending checkpoints call `checkpoint.cancel()` and `end_provider_checkpoint`; a non-cancelled expiry resumes the request with an error. The CLI waits for `accepted: true` before `command.spawn()`. Both failed admission and spawn failure use the exact matching route inverse, and admission failure reaches one `launch_untracked` call.
- **Budget preservation and blocking:** Other lifecycle requests still pass through the 150 ms clamp. Provider event connect/write timeouts remain 100 ms. The production worker uses nonblocking checkpoint admission and polling; the bounded acknowledgement wait remains on the dedicated `zmux-local-control` thread, not AppKit or the workspace executor. No new core/UI blocking wait was introduced.
- **Diagnostics:** The fallback output uses a coarse rejection/control-failure class and elapsed milliseconds; it does not include IPC errors, socket paths, request payloads, or provider data.
- **Test boundaries:** The 250 ms fake local-control peer test proves only that the CLI gives `Started` a longer transport budget while `Ended` and `Cancelled` retain the lifecycle cap. It does not prove Desktop admission. The real-PTY test uses the Session checkpoint owner and verifies exact command-epoch arbitration; its 225 ms pause is before the first Desktop poll, so it proves success after more than 150 ms but not a worker pause after the cutoff check. Its already-expired deadline is rejected before a pending reservation is established, then a real OSC report remains alertable. Those checks do not cover the late-ready-checkpoint interleaving described above. No fabricated checkpoint result or new production seam was introduced.

## Validation and proof limits

The current repair reference records CLI provider unit tests **3 passed**, the Desktop real-PTY regression **1 passed**, and `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture` **15 passed, 2 ignored, 0 failed**. It also records both intended negative controls red: restoring the 150 ms `Started` path fails the delayed-transport regression; removing the Desktop caller-deadline cap and expiration check admits the already-expired route. Those controls were restored. The focused provider-hook run preceded the final diagnostic text-only change; strict all-target Clippy compiled the final CLI and test targets afterward. Strict Clippy, formatting, and `git diff --check` are recorded as passing. I did not rerun tests or `scripts/check` while the parent aggregate check was running, and did not mutate source or tests.

The `App.swift` change maps optional title/progress values to `Any` before the existing empty-string/zero fallback, consistent with the documented strict Swift warning fix. The Linux source check is recorded as passing; hosted macOS Swift typechecking remains pending.

The hosted macOS log still establishes only the pre-spawn fallback and control-server `Broken pipe`. It does not establish checkpoint duration or this exact race as the cause. There was no native macOS reproduction or hosted compile/build acceptance in this recheck.
