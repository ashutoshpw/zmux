# Provider state deadline repair

**Date:** 2026-11-12
**Scope:** Narrow caller-budget repair for authoritative `Register` and
`ProviderObservation` lifecycle operations that wait for a Desktop Session
checkpoint. Ordinary lifecycle events and cleanup retain their existing
150 ms bound. The private provider-event socket remains at 100 ms.

## Repair

- The CLI now sends `Register` and `ProviderObservation` with one absolute
  caller deadline, capped by the existing 2.7 second event budget. Registration
  carries the existing hook-event deadline into the request helper. Observation
  creates its deadline at helper entry, before validation, and carries it
  through local-control connect, writes, response reads, and response decode.
  The later helper caps an existing deadline; it does not renew it.
- `lifecycle_operation` still applies its 150 ms clamp. `Event`, terminal
  cleanup, and provider-route end/cancel actions continue through that path.
  Provider-route start keeps its existing deadline class.
- Desktop now stamps an absolute acknowledgement deadline and cancellation flag
  before queueing `Register` and `ProviderObservation`, as it already did for
  provider-route start. Its Session checkpoint uses the earlier of the existing
  two second checkpoint cap and the command deadline minus the existing 250 ms
  response margin. Queue time consumes this budget.
- The checkpoint poller checks cancellation and expiry before polling and again
  after a ready checkpoint arrives, before advancing the cached snapshot or
  resuming the command. The resumed command checks the state deadline and
  cancellation before entering the lifecycle owner. Expired or cancelled
  checkpoint work cannot apply a late registration or observation. The pending
  checkpoint queue remains bounded.

## Owner anchors

- `apps/cli/src/hooks.rs:1181-1221,1518-1538` keeps the ordinary 150 ms helper
  and routes registration through the authoritative provider-state helper.
- `apps/cli/src/hooks/provider.rs:849-880` creates and carries the observation
  deadline. Its transport regression is at `:1168-1270`.
- `apps/cli/src/main.rs:750-756,765-769` already carries the absolute deadline
  into the socket transport; no transport implementation change was needed.
- `crates/desktop-runtime/src/control.rs:35,1198-1214` retains the existing
  two second command timeout.
- `crates/desktop-runtime/src/lib.rs:606-658,962-995,2626-2690,3020-3088,
  3199-3242` carries the Desktop deadline/cancellation and fences checkpoint
  consumption and resumed state application.
- `crates/desktop-runtime/src/sessions_tests.rs:308-620` contains the real-PTY
  authoritative state checkpoint and expiry/cancellation regression.

## Test authoring gate

1. **Observable contract:** Registration and provider observations can wait
   longer than 150 ms for the owning Desktop checkpoint while retaining their
   caller deadline. An expired or cancelled request must not mutate published
   provider state. Ordinary lifecycle events still time out at 150 ms.
2. **Credible regression:** A control peer that responds after 250 ms makes the
   old CLI helper time out. At Desktop, a ready Session checkpoint resumed after
   expiry or cancellation could apply provider state after the caller stopped
   waiting.
3. **Coverage gap:** Existing delayed-transport coverage exercises provider
   route start and cleanup, not registration or observation. Existing real-PTY
   coverage exercises route admission, not authoritative provider state. The
   new socket test covers the client transport contract; the Desktop test covers
   real Session checkpoints and resumed owner mutation. Neither duplicates the
   other's boundary.
4. **Production seam:** No new seam is used. The transport case uses a real local
   Unix socket. Desktop cases use a real `/bin/sh` PTY, Session checkpoint API,
   Worker poller/command owner, and `execute_authorized` timeout path. Resumed
   commands receive actual checkpoint snapshots returned by that Session.

## Regression evidence

- `provider_state_transport_budget_covers_registration_and_observation_only`
  delays each Unix-socket response by 250 ms. Registration and observation
  succeed after the ordinary 150 ms boundary. An ordinary lifecycle event still
  fails under the 150 ms bound. Restoring the old lifecycle helper made the
  registration assertion fail with `request deadline elapsed` at 150 ms.
- `provider_observation_and_registration_use_deadlined_live_checkpoints` runs
  both operations through real PTY-backed Session checkpoints after a 225 ms
  poll delay. It also resumes actual Session checkpoint results with expired and
  cancelled envelopes and asserts the provider identity snapshot is unchanged.
  Its real `execute_authorized` case times out registration and verifies the
  published provider identities remain unchanged through checkpoint retirement.
- Negative controls were restored before final validation. Disabling the
  resumed provider-state fence made the real-PTY test fail because expired state
  was applied. Restricting `execute_authorized` to the previous route-only
  deadline class made the timed-out registration replace the published
  provider identity.

## Validation

- `cargo test --locked --offline -p zmux
  provider_state_transport_budget_covers_registration_and_observation_only
  -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p zmux
  route_start_transport_budget_exceeds_the_lifecycle_cap_but_cleanup_does_not
  -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture`:
  15 passed, 2 ignored parent-invoked fixture tests.
- `cargo test --locked --offline -p zmux-desktop-runtime
  provider_observation_and_registration_use_deadlined_live_checkpoints --lib
  -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p zmux-desktop-runtime
  provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read
  --lib -- --nocapture`: 1 passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo clippy --locked --offline -p zmux -p zmux-desktop-runtime
  --all-targets -- -D warnings`: passed.

## Proof limits

The delayed Unix-socket peer proves CLI transport timing, not Desktop
acknowledgement. The PTY test exercises the real Linux Session and Desktop
worker owner, including a direct `execute_authorized` call, but it is not a
native macOS reproduction or a complete control-socket round trip. These checks
show the budget mismatch is closed at the tested boundaries; they do not prove
that it was the sole cause of the hosted macOS attachment failure or establish
a hard scheduling guarantee. `scripts/check`, hosted checks, commit/push, and
artifact delivery remain with the parent task owner.
