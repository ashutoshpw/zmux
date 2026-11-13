# Provider state budget independent recheck

**Date:** 2026-11-12
**Scope:** Read-only review of the runtime diff in `apps/cli/src/hooks.rs`,
`apps/cli/src/hooks/provider.rs`, `crates/desktop-runtime/src/lib.rs`, and
`crates/desktop-runtime/src/sessions_tests.rs` against `b11b7ab`. The committed
Ghostty linkage files were reviewed at `8efe84b` against the already accepted
`/tmp/zmux-osc7501-ghostty-byte-review.json`; this did not repeat the broad
archive or Mach-O review.

## Verdict

The deadline plumbing is correct by source inspection, and I found no production
logic defect in this scoped repair. Register and ProviderObservation receive
bounded caller deadlines, Desktop stamps its own two-second acknowledgement
cutoff before its bounded enqueue, Session checkpoints use the earlier cutoff,
and the poller plus final lifecycle owner both fence expiry and cancellation.
The existing route-start rollback path is retained. Two regression-coverage
gaps remain; the committed linkage evidence consistently pins the updated
review copy while preserving the accepted artifact facts.

## Findings

### P2: The Desktop timeout test does not exercise retirement of an admitted checkpoint

At `sessions_tests.rs:581-590`, the real `execute_authorized` case passes
`Duration::ZERO`. The Desktop stamps an already elapsed reply deadline, then
`Worker::command` subtracts the 250 ms response margin before requesting the
Session checkpoint (`lib.rs:3061-3092`). `Session::request_program_status_checkpoint`
returns `Timeout` before enqueue when that deadline is elapsed
(`crates/session-runtime/src/lib.rs:899-906`). Thus this case checks the final
owner's handling of an expired request, but it does not enter the Desktop pending
checkpoint queue or test its cancellation/expiry retirement branch.

The four direct cases at `sessions_tests.rs:518-555` do test the final owner
fence with actual Session snapshots, but construct `AgentLifecycleCheckpointed`
directly. The production poller checks expiry/cancellation before and after
`try_recv` and cancels/removes pending work (`lib.rs:2631-2674`); that particular
queue interleaving remains source-inspected rather than covered by this test.
The successful 225 ms cases also enter through `execute_lifecycle_on_worker`,
which calls `Worker::command` with a hand-built envelope
(`sessions_tests.rs:308-358,479-492`); they do not exercise successful
`execute_authorized` deadline stamping and bounded queue admission.
The repair reference's claim that the zero-timeout `execute_authorized` case
observes state “through checkpoint retirement” is therefore stronger than the
test demonstrates. The recorded red control for the old route-only admission
class is useful, but it does not establish the current in-flight queue-cancel
branch.

### P2: The CLI Register transport test bypasses the production Register caller

`provider_state_transport_budget_covers_registration_and_observation_only`
constructs a Register request and calls
`authoritative_provider_state_operation` directly (`hooks/provider.rs:1224-1237`).
The production Claude registration caller is `Adapter::register_lifecycle`
(`hooks.rs:1518-1532`). Reverting only that caller to
`lifecycle_operation` would restore the 150 ms limit on the affected Claude
SessionStart path while this test would still pass. The helper's delayed-socket
contract and the observation path are tested; the Register call-site wiring is
currently protected only by source inspection, not by the named regression.

## Positive findings

- `Register` preserves its incoming hook deadline into the specialized helper;
  the helper only caps it at the 2.7 second event budget
  (`hooks.rs:1209-1221,1531`). `ProviderObservation` creates one deadline at
  helper entry before validation and keeps it through the same helper
  (`hooks/provider.rs:849-874`). The CLI control transport applies that absolute
  instant to connect, writes, response reads and decode (`apps/cli/src/main.rs:750-845`).
- The ordinary lifecycle helper still clamps to 150 ms (`hooks.rs:1183-1187`),
  and the private provider-event socket path still has 100 ms connect/write
  bounds (`hooks/provider.rs:204-220`). Provider-route start retains its
  existing deadline class.
- The committed Ghostty linkage manifest pins the current independent review
  JSON (`linkage.json:14`), and the JSON's `review.document_sha256`
  (`independent-review.json:276-282`) matches the committed review Markdown.
  Compared with the accepted `/tmp` review, only that document hash changed;
  the source, receipt, archive, thin libraries, fixture evidence, proof limits,
  and original candidate-linkage snapshot are unchanged.
- `execute_authorized` creates the Desktop deadline and cancellation flag before
  `try_send`; the command queue is bounded and nonblocking (`lib.rs:962-995,
  1681-1685`). The Session checkpoint request also uses `try_send`, and its
  handle is polled with `try_recv` (`session-runtime/src/lib.rs:896-930`). The
  pending Desktop checkpoint queue retains its configured capacity check
  (`lib.rs:3083-3115`). The synchronous acknowledgement wait is on the local
  control connection handler (`control.rs:486-504`), not the Desktop worker or
  UI executor.
- The checkpoint cutoff is the earlier of the two-second Session cap and the
  original Desktop deadline minus 250 ms. Queue delay consumes the original
  deadline. Expired or cancelled checkpoint work is cancelled before it can
  advance the snapshot; the resumed command checks again before entering the
  lifecycle mutation owner (`lib.rs:2631-2674,3031-3046,3189-3200`).
- Route-start timeout and failed-ack rollback remain in place: the exact route
  identity is captured, expiry calls `cancel_unacknowledged_provider_route`,
  and an undelivered acknowledgement cancels the same unconfirmed route
  (`lib.rs:3008-3029,3233-3242,3291-3346`).
- The `audit-test-cases` review criteria support retaining the real Unix-socket
  transport case and real-PTY owner case because they cover distinct contracts.
  The final-owner negative case is useful, though the synthetic checkpointed
  command should not be described as exercising the pending queue.

## Validation and limits

I reran these bounded existing tests; all passed:

- `cargo test --locked --offline -p zmux provider_state_transport_budget_covers_registration_and_observation_only -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p zmux-desktop-runtime provider_observation_and_registration_use_deadlined_live_checkpoints --lib -- --nocapture`: 1 passed.
- `cargo test --locked --offline -p zmux-desktop-runtime provider_route_admission_uses_live_prompt_epoch_and_survives_fast_exit_before_journal_read --lib -- --nocapture`: 1 passed.

The reported negative-control mutations were not replayed in this read-only
review. The parent reports `scripts/check` passed 759/0/7 against the stable
repair source. Hosted linkage run 43324892073 was still executing at the latest
parent update; I did not verify its final result. No native macOS runtime
acceptance is established here. The separately reported duplicate Claude
attention-fallback failure in `provider_hooks.rs:2069` was outside this repair
review and was not changed.
