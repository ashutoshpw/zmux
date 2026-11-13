# Receipt-bearing Claude Event checkpoint repair

**Date:** 2026-11-13
**Scope:** Complete the pending provider-state repair for receipt-bearing Claude
lifecycle Events and close its two independent test gaps. The provider-state
repair and recheck remain unchanged. Parent-owned status/plan, Ghostty, final
review, aggregate check, and delivery work were preserved.

## Repair and decision

- A lifecycle `Event` with `hook_id: Some` is checkpoint-backed authoritative
  work. The CLI carries the original absolute monotonic hook deadline to the
  control server, bounded by the existing 2.7 second `EVENT_BUDGET`. The control
  handler accepts deadline metadata only for this Event shape and dispatches it
  with `execute_authorized_until`, so the existing two-second generic command
  timeout cannot race the caller's checkpoint-and-response window.
- Desktop admits the Event with the caller deadline and cancellation flag. Its
  Session checkpoint keeps the existing two-second maximum and reserves 500 ms
  for the outer control response. Queue admission, checkpoint polling, and the
  final lifecycle owner all check expiry/cancellation. Expired or cancelled
  work is retired before it can update lifecycle state or publish an alert.
- The 150 ms ordinary lifecycle path remains for no-hook Events and cleanup;
  the private provider-event socket remains at 100 ms. Register and
  ProviderObservation retain their separate provider-state treatment and
  existing 2.7 second outer budget. Queues remain bounded and no blocking work
  moved to the UI or core action executor.
- After any request byte may have been sent, a missing or untrustworthy receipt
  becomes an internal `HookOutcome::Unknown`. It is neither a handled receipt
  nor a definitive no-commit result: the client does not retry and does not
  write the unkeyed fallback. A known typed rejection, unrouted/pre-send error,
  or explicit `NotHandled` receipt retains its existing fallback behavior.
  This is the smallest change that prevents a fallback plus a late alert without
  changing the shared lifecycle disposition enum or adding fallback IDs.

The tradeoff is deliberate: if an ambiguous request did not commit, withholding
the visual fallback can leave that hook without a notice. The client cannot
distinguish that case from a committed Event whose response was lost; replaying
or selecting fallback would reintroduce duplicate notices.

## Test authoring gate

1. **Observable contract:** A receipt-bearing Event can wait for its owning
   Session checkpoint within the original hook deadline. Only a confirmed
   receipt selects the lifecycle disposition; expired/cancelled work cannot
   mutate provider state or publish an alert, and an ambiguous post-send result
   cannot select a duplicate fallback. Ordinary no-hook and cleanup budgets
   remain 150 ms.
2. **Credible regression:** The former 150 ms CLI helper could return
   `NotHandled` while Desktop later applied the Event. A response lost after
   commit could create the same duplicate even after increasing the wait.
   Desktop could also apply a ready checkpoint after caller expiry. Reverting
   only `Adapter::register_lifecycle` to the generic helper would restore the
   prior Register timeout.
3. **Coverage gap:** The prior delayed transport case called the provider-state
   helper without exercising `Adapter::register_lifecycle`; the 225 ms real-PTY
   provider-state cases called `Worker::command`; and the zero-timeout auth
   case expired before Session checkpoint enqueue. The extended transport test
   now calls the production Adapter methods. The existing real-PTY test admits
   pending Session checkpoints before expiry/cancellation, polls their
   retirement, resumes real checkpoint snapshots at the final Event fence, and
   runs delayed successful provider observation and Event requests through
   `execute_authorized`. These are distinct CLI transport and Desktop owner
   contracts, not duplicate checks of one helper.
4. **Production seam:** None added. Coverage uses a real local Unix socket,
   production Adapter methods, a real `/bin/sh` PTY and Session checkpoint
   queue, the Desktop poller and `execute_authorized`, plus the existing
   real-Desktop CLI fixture.

## Regression evidence

- `provider_state_and_receipt_event_transport_keep_their_owner_budgets` calls
  `Adapter::register_lifecycle`, submits ProviderObservation, and submits a
  receipt-bearing Event to a local Unix-socket peer that delays replies 250 ms.
  The receipt-bearing Event succeeds beyond 150 ms and carries its absolute
  deadline. The no-hook Event still hits the 150 ms boundary. For the final
  Event, the peer consumes the request and drops the reply; the test asserts an
  `Unknown` outcome, no retry, and no fallback selection. The peer does not
  itself mutate Desktop state, so this is client-side ambiguity proof, not an
  end-to-end lost-response-after-commit reproduction.
- `provider_observation_and_registration_use_deadlined_live_checkpoints` uses a
  real PTY-backed Session. Expired and cancelled receipt Events first appear in
  the pending Desktop checkpoint queue; the test then polls and verifies
  retirement, unchanged provider identities, and no alert. Separate resumed
  owner cases use snapshots returned by that Session and prove expired or
  cancelled Event work cannot publish a late alert. A timed-out
  `execute_authorized` Event is fenced through PTY queue drainage. The delayed
  success phase runs both a provider observation and a receipt Event through
  `execute_authorized` while PTY writes hold their real checkpoints; both wait
  at least 300 ms, the observation becomes visible, and the Event returns
  `AlertCovered` with one alert.
- The unmodified `automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli`
  integration fixture invokes its isolated real-Desktop child. The complete
  `provider_hooks` target passes its fallback assertions, including the original
  single-fallback assertion.
- The control server accepts the deadline field only for receipt-bearing
  lifecycle Events, reconstructs it from the monotonic clock with the same
  2.7 second cap, and calls the absolute-deadline dispatch path. This preserves
  the generic two-second command budget for other control commands.

## Negative controls

Each temporary mutation was restored immediately after the expected failure:

- Reapplying the 150 ms cap to the receipt Event CLI helper failed at the
  transport test's `>= 200 ms` assertion.
- Routing `Adapter::register_lifecycle` back through `lifecycle_operation`
  failed with `request deadline elapsed` before the delayed response.
- Removing receipt Events from the final Desktop state-expiry predicate made
  the expired real-checkpoint Event reach the lifecycle owner; the expected
  `Timeout` assertion failed.
- Selecting the fallback for `HookOutcome::Unknown` made the lost-reply case
  produce `Some("attention_silent")` instead of `None`.

These controls fail at the intended owner assertions, not at unrelated policy
or fixture guards.

## Validation

- `cargo test --locked --offline -p zmux provider_state_and_receipt_event_transport_keep_their_owner_budgets -- --nocapture` — 1 passed.
- `cargo test --locked --offline -p zmux --test provider_hooks -- --nocapture` — 15 passed, 2 ignored parent-invoked child tests; the real-Desktop parent fixture passed.
- `cargo test --locked --offline -p zmux-desktop-runtime provider_observation_and_registration_use_deadlined_live_checkpoints --lib -- --nocapture` — 1 passed.
- `cargo test --locked --offline -p zmux-desktop-runtime provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read --lib -- --nocapture` — 1 passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `cargo clippy --locked --offline -p zmux -p zmux-desktop-runtime --all-targets -- -D warnings` — passed.

## Proof limits and handoff

The PTY owner tests ran on Linux and do not establish native macOS timing or
prove that this was the sole cause of the earlier hosted fallback failure. The
transport lost-reply case intentionally leaves commit status unknowable; it
proves the CLI response policy, not a real mutation followed by a dropped
control response. The existing real-Desktop fixture separately proves normal
receipt/fallback behavior. Full `scripts/check`, hosted gates, native artifact
acceptance, commit, push, and delivery remain with the parent task owner.
