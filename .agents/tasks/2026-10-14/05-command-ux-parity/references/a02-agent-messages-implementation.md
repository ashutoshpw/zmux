# A02a: durable local inbox implementation

This slice implements an original Rust inbox owner and thin local JSON CLI. It
covers durable admission, inspection, settings, replies, and a neutral local
consumer protocol. It does not implement provider hooks, wake, a native inbox,
MCP/remote access, or full frozen-reference CMD-025..029 parity. No cmux source,
test bodies, renderer prose, or assets were imported or translated.

## Owner and durability

`crates/desktop-runtime/src/agent_messages.rs` owns bounded runtime admission and
operation receipts; `agent_messages/domain.rs` owns message/consumer/lease rules;
`agent_messages/storage.rs` performs all private-file reads, validation, locking,
serialization, sync, and atomic replacement on its worker. It reuses the existing
secure persistence helpers. Each transaction reopens the authoritative journal
under `agent-messages.lock`; no stale cached journal is written back.

The installation journal is `agent-messages.json`, separate from portable S01
`SessionDocument`. Session import/export rejects the reserved journal/lock names.
The real restart regression also inspects the saved portable session and verifies
that it did not copy the inbox message identity. Transient desktops explicitly
refuse messaging rather than claiming durability.

Requests retain their executor model publication. Workspace titles, focus, and
positional selectors cannot silently retarget while the worker waits for I/O.
Sender attribution is resolved independently through the existing ambient caller
contract: a surviving surface determines its current workspace/window after a
move. Labels do not authenticate providers. Replies use the parent's exact stored
sender surface and thread, with no focus fallback.

A pending receipt is not Saved. The CLI polls the original request UUID and reports
success only after the worker confirms commit. Identical retained request input
returns its original admission receipt even after the original target closes;
conflicting UUID reuse fails. Status outside the retained horizon says
`unknown_or_expired`. The CLI never automatically submits a replacement send after
uncertainty. The retained horizon is every unsettled send plus the newest 256
settled mutation receipts, subject to the same journal byte budget.

## Handoff authorization and in-flight responses

The explicit cross-process linearization point is the installation-lock-protected,
fsynced transition of the exact lease to `handed=true`. That transaction validates
current consumer generation, namespace, live surface/session, held/interactive
state, effective enablement, and the original lease deadline before authorizing
context exposure. Claim reservation alone returns no text. A subsequent status
transaction performs the durable handoff authorization.

An already authorized first response can arrive after a concurrent foreign Desktop
supersedes that consumer, inside its original ten-second lease TTL. This is an
in-flight response, not renewed authority. There is no instantaneous cross-process
revocation promise between the fsynced authorization and later response delivery.
Every subsequent context-status lookup and every new acknowledgement revalidates
the authoritative generation; a superseded consumer cannot advance delivery.
Provider adapters must correlate the request's consumer generation, message IDs,
and exact lease, and account for stale in-flight responses. The implementation
never holds the installation lock across executor or network response delivery.

Local held-bearing polls, settings mutations, registration, and exact model/session
changes additionally invalidate cached exposure generations. Worker post-fsync,
completion polling, and final executor exposure all check the original owning
`Instant`; no cache or retry refreshes the lease TTL. Foreign readers use shared
system-monotonic lease timestamps, not wall-clock deadlines. Restart cannot adopt
an old runtime's capability or lease. Durable capability correlation contains
SHA-256 hashes only; plaintext bearer values stay in current-process memory.

Off before handoff fails queued unhanded work. Off after a valid prior handoff still
allows its exact retained acknowledgement, but prohibits another context exposure.
Read-turn names only exact IDs delivered to that consumer generation. Lost context
or acknowledgement can cause retry of the same immutable message under a new
lease: delivery is at least once, with no exactly-once provider-consumption claim.

## Bounds and executor isolation

Initial limits are 4 KiB UTF-8 body, 64-byte single-line sender label, 128 messages,
256 explicit settings, eight pending worker credits, 32 inbox rows/page, eight
messages/claim, 128 consumers/leases, and a 512 KiB serialized journal. The complete
socket request remains capped at 16,384 bytes. CPU-only bounded encoding also
protects trusted admission before hashing. Newline, carriage return, and tab are
preserved, including CRLF; other control characters are refused explicitly.
Unread/queued/leased work is never evicted to admit another send. Only read/failed
history is eligible for message pruning.

Canonical removals enter the same shared `Worker::action` commit path. Exact closure
IDs are acknowledged only after durable reconciliation. An unreconciled backlog
above 512 identities refuses that close explicitly; it never silently drops a
closure or blocks ordinary terminal input. Startup-before-restore, unrelated
namespaces, and orderly shutdown are not inferred as recipient closure.

Socket mutations retain their execution authorization across the durable commit
barrier on the I/O worker. Executor checks use nonblocking `try_check`; a queued
policy writer yields `authorization_unavailable` before mutation rather than
stalling the executor behind filesystem I/O. Accepted messages are not replayed.
Shutdown stops new admissions, drains accepted worker jobs, and joins the actual
worker before reporting persistence cleanup complete.

## Behavioral evidence and review repairs

All commands below use:

```sh
CARGO_TARGET_DIR=/data/PROJECTS/ashutoshpw/zmux/target/agent-validation/a02-agent-messages/cargo
```

- Initial committed regression `36bad64`: actual persistent Desktop, shipped CLI,
  real framed Unix socket, private filesystem, and two actual `/bin/sh` PTYs.
  `cargo test --locked -p zmux --test agent_messages durable_queued_message_survives_restart_without_terminal_input -- --nocapture`
  executed one case and failed on `unknown command agent`, after fixture readiness.
  The identical command passed after repair. Raw logs:
  `/tmp/zmux-a02-queued-red.log`, `/tmp/zmux-a02-queued-green.log`.
- Review caught live-target validation being applied to an already retained Send.
  The existing idempotence/restart owner test gained a post-close replay control;
  its actual red rejected the missing surface, then green returned the original
  hash-verified receipt. Command suffix:
  `cargo test --locked -p zmux-desktop-runtime --lib agent_messages::tests::retained_request_ids_deduplicate_conflict_and_survive_worker_restart -- --nocapture`.
  Logs: `/tmp/zmux-a02-retained-close-red.log` and `-green.log`.
- Review caught a completed cached claim exposing context after held state or lease
  expiry. One actual-worker table exercised both independently, including the real
  ten-second expiry. Red reported stale exposure for both `held` and `expired`;
  green withheld text and cancelled only the exact lease. Command suffix:
  `cargo test --locked -p zmux-desktop-runtime --lib agent_messages::tests::completed_cached_claim_rechecks_held_and_real_expiry_before_exposing_text -- --nocapture`.
  Logs: `/tmp/zmux-a02-cached-fence-red.log` and `-green.log`.
- Review identified policy writer fairness as an executor-stall risk. A Linux-only
  real socket test holds the journal lock and observes the actual worker's private
  state-directory descriptor, queues a real policy writer, then submits existing
  authorized action/status envelopes and unrelated trusted executor work. The
  product red timed out behind the writer; green returns explicit unavailability
  while trusted work completes before journal unlock. It releases the file lock
  and joins/shuts down owners before the final assertions. Command suffix:
  `cargo test --locked -p zmux-desktop-runtime --lib control::tests::message_io_and_queued_policy_writer_do_not_block_the_workspace_executor -- --nocapture`.
  Logs: `/tmp/zmux-a02-policy-io-red.log` and `-green.log`. An earlier descriptor
  readiness fixture sampled transient lock descriptors and failed setup; it was
  corrected before recording this actual behavioral red.
- The explicit in-flight ordering owner test caches the first durably authorized
  response, then registers a foreign consumer, receives that original exact
  response within TTL, and proves that fresh status and acknowledgement fail.
  Log: `/tmp/zmux-a02-inflight-order.log`. This protects the stated linearization
  contract rather than claiming instantaneous revocation.

The owner suite also covers concurrent journal writers, foreign lease preservation,
request retention/pruning, unread capacity, exact read-turn IDs, opt-out ordering,
reply ownership, stale targets, ambiguous titles, browser/history rejection, live
moves, fresh sessions, corrupt/symlink state, queue credits, and accepted-write
shutdown. Public CLI cases independently cover byte-preserving stdin/restart/no
PTY input, literal/help-valued arguments, grammar/UTF-8/body-limit refusals, and
read-only inspection versus mutation/capability denial. Existing offline discovery
coverage caught eager ambient caller validation; caller resolution now happens only
at command execution.

Final validation is recorded in `/tmp/zmux-a02-check-final.log`: `scripts/check`
passed 368 tests with one existing ignored test, plus Clippy, license inventory,
C ABI, and native profile-lease C ABI checks.
The license inventory retains its existing three upstream-text review notices.
Native macOS/AppKit execution and actual provider context delivery/wake were not
run and remain separate gates. Local Rust/C ABI checks do not establish those
behaviors or frozen-reference parity.

## Main integration

Integrated as `d429636` and `8c54b39`. Seventeen of twenty cohort files match
reviewed `983d2c2` byte for byte; the three merged files retain main's previously
accepted V2 workload dispatch, dependencies and history tests. Integrated
`scripts/check` passes 379 tests, one existing ignored, strict Clippy, notices
and both foreign ABI checks. Log:
`/tmp/zmux-main-a02-integration-check-20261016.log`.
Darwin/native/provider acceptance remains separate.
