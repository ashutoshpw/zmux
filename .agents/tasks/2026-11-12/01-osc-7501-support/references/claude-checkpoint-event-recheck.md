# Claude checkpoint Event independent recheck

**Date:** 2026-11-13
**Scope:** Read-only review of the combined CLI/Desktop working diff against
`8efe84bd1ab721e8afc5222e0a3fcfa33f753260`, plus the parent-owned `9048d94`
Ghostty/evidence commit. Unrelated task, profile-lifecycle, and preservation
files were left untouched.

## Verdict

The deadline plumbing, bounded queues, authorization path, and the two previously
identified test gaps are addressed. The CLI/Desktop repair is **not yet accepted**:
I found two P2 cases where a response that does not prove non-commit can still
select the visual fallback. Native macOS runtime and actual linked-app acceptance
remain pending; this review does not establish either.

## Findings

### P2: An Event can commit, cross the reserved cutoff, and return a fallback-worthy rejection

`crates/desktop-runtime/src/lib.rs:3089-3096` defines provider expiry as the
original deadline minus the 500 ms response reserve. `Worker::command` checks
that predicate before dispatch at `3242-3282`, then runs the checkpointed Event
owner through `execute_command` (`3462-3466`). The owner applies the lifecycle
Event and can append its alert at `3875-3879` and
`crates/desktop-runtime/src/agent_lifecycle.rs:1888-1923`.

If the worker is descheduled, or the owner takes long enough, to cross the
`deadline - 500 ms` cutoff after that apply, the post-owner check at
`lib.rs:3285-3293` clears the receipt and replaces the result with
`DesktopError::Timeout`. `control.rs:1317-1332` maps that `Execution.result`
timeout through its catch-all to `command_rejected`; the separate
`desktop_error()` mapping to `outcome_unknown` applies only when the outer
`execute_authorized_until` call itself returns `Err` (`control.rs:1486-1505`).
The CLI treats `outcome_unknown` specially, but treats this `command_rejected`
as `Err` (`hooks.rs:1331-1338`). The adapter then defaults that error to
`NotHandled` (`hooks.rs:1823-1825`), which `fallback_for_outcome` can render
(`hooks.rs:1152-1166`). This permits a committed alert plus its fallback.

Once the lifecycle owner has been entered, a timeout no longer proves that the
Event did not commit. Preserve a valid receipt when available, or return an
unknown outcome that suppresses fallback. The existing PTY cases cover expiry
before the final owner and ordinary successful admission, but not this
post-apply cutoff interleaving.

### P2: A malformed success result is fabricated into `NotHandled`

At `apps/cli/src/hooks.rs:1340-1350`, every `accepted` value other than JSON
`true`, including a missing or wrongly typed field, becomes
`Receipt(NotHandled)`. In the current lifecycle owner, successful receipts set
`accepted: true` and rejections travel as control errors; there is no documented
`accepted: false` receipt (`agent_lifecycle.rs:1622-1627,1918-1923`). A malformed
success result therefore is not evidence that the request did not commit. The
fallback path can turn it into a duplicate notice after an Event was applied.

Treat absent or invalid `accepted` data as `Unknown`. Keep fallback for a valid
explicit `NotHandled` receipt or a typed rejection that establishes no commit.

## Accepted source checks

- The first hook Unix transport carries the original bounded expiry in
  `expires_monotonic_ms`; its owner reconstructs a deadline bounded by the
  initial 2.7 second budget (`hooks.rs:929-966,1972-1994`). The control transport
  sends an absolute monotonic nanosecond deadline for receipt-bearing Events;
  the control parser rejects metadata on other commands and accepts only
  `Event { hook_id: Some(_) }` (`main.rs:758-765,782-800`,
  `control.rs:547-573,617-635,1266-1297`). The control conversion caps it at
  2.7 seconds. `execute_authorized_until` retains the supplied `Instant` and
  derives its wait from that instant without renewing it (`lib.rs:1002-1012`).
- Peer UID checks and `ControlGate::admit` remain in force. Deadline parsing
  removes only the metadata field; `password` is retained and passed to the
  gate before dispatch (`control.rs:503-523,560-571,637-645`). The admitted
  authorization is passed through `mutate_until` to the Desktop executor.
- The extended slow budget is limited to Register, ProviderObservation, and
  receipt-bearing Events. Ordinary lifecycle work remains capped at 150 ms
  (`hooks.rs:30-31,1259-1299`); the private provider-event socket retains its
  100 ms connect/write limits (`hooks/provider.rs:210-213`). Cleanup still uses
  the ordinary no-hook lifecycle path. The existing ProviderRouteStarted
  deadline class remains separate and unchanged.
- Both Unix request writers track whether any request byte was sent. A
  pre-send failure remains fallback-eligible; a partial/full send followed by a
  missing or invalid receipt becomes `Unknown`, with no retry. `Unknown` skips
  fallback (`hooks.rs:964-980,1009-1035,1152-1167`; `main.rs:829-869`). The two
  findings above are the remaining cases where a response is incorrectly
  classified as a definitive rejection.
- Checkpoint admission remains bounded. The real-PTY test now admits pending
  Session checkpoints, exercises cancellation and expiry retirement in the
  poller, resumes real snapshots at the final owner fence, and verifies delayed
  successful ProviderObservation and Event calls through
  `Desktop::execute_authorized` with one resulting alert
  (`sessions_tests.rs:362-900`). The transport test now calls production
  `Adapter::register_lifecycle`, closing the prior Register call-site gap
  (`hooks/provider.rs:1169-1378`). Route-start expiry and failed-ack rollback
  still cancel only the captured unconfirmed route, including undelivered
  acknowledgement cleanup (`lib.rs:3060-3073,3288-3293,3343-3387`).
- The lost-reply Unix peer consumes the request and drops its reply but does not
  mutate Desktop state. It proves client-side `Unknown`, no retry, and no
  fallback selection, not commit-then-drop behavior. The separate real-Desktop
  fixture proves the ordinary receipt/fallback behavior. This is a clear proof
  limit, not a reason to add a duplicate test that claims the peer committed.
- `9048d94` changes `owner.rs:484` to call the existing
  `MetaState::terminal_closed_snapshot` method at `metadata.rs:143`. The nested
  `independent_review_sha256` matches the reviewed Markdown document
  (`f613fbf9f58020ffa3e0176a4c85c5b9567fb207b2d4f5cd6dfcb9d0b34eb32b`), and
  the outer linkage manifest hash matches the reviewed JSON
  (`998fe58137318448bd3111a31ee7dbbf20c3b1898c242a2abf26321ce47a3096`). No
  broader artifact-byte review was repeated.

## Focused verification and limits

Reran existing focused checks only:

- CLI transport budget test: 1 passed.
- `provider_hooks`: 15 passed, 2 ignored; the real-Desktop parent fixture
  `automatic_install_to_runtime_lease_is_atomic_against_real_cleanup_cli`
  passed.
- Real-PTY checkpoint test: 1 passed.
- Route admission/rollback owner test: 1 passed.

The parent reports `scripts/check` passed 759/0/7 in 282.22 seconds against
stable source fingerprints, with unrelated preservation checks matching. I did
not rerun that aggregate check. The PTY tests ran on Linux. No native macOS
runtime proof or actual linked-app acceptance is established here, and the
hosted linkage rerun after `9048d94` remains pending in the task status.

**Acceptance:** bounded deadline propagation, authorization, budget isolation,
queue retirement, the newly closed test gaps, route rollback, and the scoped
Ghostty pin/method repair are accepted by this review. The CLI/Desktop repair
remains not accepted until both P2 response-classification findings above are
resolved. No source changes, commits, pushes, or workflow runs were made by this
review.
