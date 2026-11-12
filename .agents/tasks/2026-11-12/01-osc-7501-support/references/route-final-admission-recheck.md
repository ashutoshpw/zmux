# Final route admission: independent recheck

**Date:** 2026-11-12

**Verdict:** Accepted within the requested scope. No actionable finding.

## Review

- `Worker::command` captures the exact target, route ID, runtime ID, and generation from either `AgentLifecycle` or `AgentLifecycleCheckpointed` `ProviderRouteStarted` requests. Other actions produce no route identity and retain their existing acknowledgement path.
- The caller deadline and cancellation flag are checked before execution, after execution, and again after publication/delta calculation before acknowledgement assembly. If expiry is observed after execution, the worker removes only the exact unconfirmed local reservation, clears the lifecycle receipt, returns `Timeout`, and publishes the rolled-back state. The last check is before acknowledgement construction; no hard OS scheduling bound is implied.
- If `SyncSender::try_send` fails, the unconsumed execution (including any accepted receipt) is dropped. Cleanup uses the same exact identity and `transport: None`; `Runtime::cancel_provider_route` removes only matching unconfirmed routes and preserves confirmed authority. The worker republishes when a route was removed, releasing its predictive source state. The closed-receiver path adds no source lock that could survive the call.
- Result, receipt, and snapshot behavior is consistent: timeout clears the receipt and publishes the post-cleanup snapshot; failed delivery exposes no receipt and republishes the cleanup state. Unrelated command results do not enter route cleanup.

## Regression coverage and limits

The added cases use real `Session` checkpoint results and advance the cached snapshot through the same worker helper used by the checkpoint poller before direct resumed-command execution. They cover an expired envelope, a cancelled envelope, and a dropped acknowledgement receiver followed by a real OSC report that must remain alertable. The stale-cache setup remains later in the test for the successful live-checkpoint route case. The repair notes report that both negative controls failed for their intended assertions and were restored; I inspected the test and references but did not rerun it while the parent full check is running.

This test establishes owner-side deadline/cancellation rejection and cleanup on failed internal acknowledgement. It does not force a deadline to elapse specifically between successful route mutation and acknowledgement assembly. It also does not establish that an external Unix-socket reply was consumed by the launcher, identify the sole cause of the earlier hosted macOS failure, or provide a hard real-time guarantee.
