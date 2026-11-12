# Remote route retirement recheck

**Date:** 2026-11-12
**Scope:** independent read-only review of route confirmation, predictive source release, exact remote transport retirement/replacement, and the framed SSH regression. The shared `main` checkout and its existing index/dirty work were preserved.

## Verdict

**Accepted for this scoped repair. No severity-ranked implementation findings.** The reservation/confirmation split closes the reported stale provider-source lock after remote transport loss while retaining authority backed by provider evidence or a normal spawned-invocation `Ended` phase. This verdict does not accept the broader OSC 7501 task or its pending full validation and delivery gates.

## Review evidence

- `crates/desktop-runtime/src/program_status.rs:284-378` validates the target/provider owner, nonnil route/runtime/provider-generation IDs, route-ID binding, and that the captured session/command/cursor is not older than admitted status. A new route is unconfirmed unless the caller supplies accepted lifecycle evidence.
- `program_status.rs:381-538` keeps the outcomes distinct. `Ended` confirms and retains the exact route; `Cancelled` removes only the exact unconfirmed route; transport retirement matches target plus runtime/generation and removes only unconfirmed reservations. Predictive source release additionally matches session epoch, command epoch, and route identity. Confirmed routes become ended owners and retain source ownership for their command epoch.
- `program_status.rs:631-759,940-955` advances the transition cursor while processing suppressed transitions. Prompt/command changes, journal gaps, and owner pruning retire stale route state; source release does not rewind the cursor or replay a consumed alert. With multiple reservations, cancellation/retirement affects only the matching route/transport, leaving other reservations available to own subsequent transitions.
- `crates/desktop-runtime/src/lib.rs:2139-2200,2250-2475,2717-2795` gates frames on the exact current stream binding and fences control transitions by the stable target, launch, runtime, and nonce owner. `Lost` and replacement `Connected` retire the exact lost/previous generation, even if the shared binding has already advanced. The remote start path registers a reservation before queueing its receipt, cancels it if queue admission fails, confirms it only on provider lifecycle evidence or `Ended`, and removes it on `Cancelled`.
- `crates/desktop-runtime/src/remote_agent_lifecycle.rs:181-200,619-632,838-910` binds receipts to the exact stream generation, reports queue admission rather than remote delivery, and discards queued commands whose binding no longer matches the writer's generation.

I found no fence mismatch across target, provider, route ID, runtime ID, provider generation, transport generation, session epoch, or command epoch in these paths. A provider alert promotes a predictive lock to established provider authority; later cancellation or transport retirement cannot erase it. A normal end is retained as provider ownership, while pre-spawn cancellation is released.

## Test strength and limits

The policy regressions cover predictive-lock release after retirement, promotion before cancellation, and normal-ended ownership (`program_status.rs:1431-1550,1573-1831`). The framed SSH case (`remote_agent_lifecycle.rs:1927-2234`) observes a real route frame on G1, queues its route receipt through the real stream owner, observes G1 loss and a G2 `Connected`, then checks later OSC admission after policy retirement. The existing exact-binding receipt test separately checks serialized nonce/generation/route binding (`remote_agent_lifecycle.rs:939-993`).

The SSH case does **not** establish that the receipt remained in the local writer queue until loss: the peer leaves stdin unread, but the writer may already have written the receipt into the SSH pipe buffer. It also does not exercise the Desktop worker dispatch end-to-end; the test applies the received route to the policy owner and retires that exact transport directly. These are proof limits, not fabricated peer-admission evidence. Without a remote acknowledgement of receipt consumption, a receipt may have reached the wrapper and started a provider whose lifecycle observation or normal `Ended` frame never arrived before loss. Releasing that unconfirmed reservation is the chosen recovery behavior and cannot distinguish that case from an undelivered receipt.

The SSH test intentionally returns early if Python/OpenSSH prerequisites are absent. The task record reports the focused SSH regression as 1 passed, but that count alone does not show whether the prerequisite skip branch ran. I did not rerun focused tests because the parent reports the full `scripts/check` is already running. The task record also reports 14 Program Status policy tests, the exact-binding receipt test, provider-state gap coverage, and the public Desktop PTY projection test passing; these are recorded results, not results independently reproduced in this recheck. Full `scripts/check`, hosted/native acceptance, and artifact delivery remain outside this bounded review.
