# Ghostty linked Session test diagnosis

**Date:** 2026-11-13

**Hosted run:** `43335306931`

**Diagnostic artifact:** `13997490318`

**Source:** `9048d943eea63ff18218f0ba1b0dbfe158810ccf`

## Finding

The hosted failure is in two existing real-PTY Session tests, before the
Ghostty-specific linked Session acceptance test runs. The ARM64 linked Rust
compile, four staged-engine tests, and linked terminal-adapter Program Status
test passed. `linked_real_pty_orders_query_before_da_and_retires_only_after_final_drain`
was compiled but not executed (`optional_session_arm64_executed=false`). The
failed diagnostic run and its artifact do not establish linkage acceptance or
app delivery.

The observed failures follow the configured PTY ordering contract and fixture
synchronization:

1. The Darwin ARM64 capacity probe attempted 4096-byte writes and accepted 1022
   bytes before `WouldBlock`. In the failing tests, `block_input()` admits 16
   chunks, or 13 when reserving three queue slots. Zero completed receipts means
   no full 4096-byte receipt completed; it does not mean that no bytes were
   accepted.
2. The child does not read PTY input until the test creates `release`. The
   Session already has a partially written input and captures the earlier
   admitted inputs before reading the child's query output. It queues that
   output behind those inputs and starts an ordering deadline equal to
   `EngineOptions.reply_timeout`.
3. Since the child will not drain the PTY until `release`, the active write
   cannot complete, so deferred output cannot advance to terminal parsing and
   no `CausalReply` exists yet. The 500 ms case therefore reaches
   `deferred_ordering_deadline()` and fails with `causal PTY ordering deadline`,
   not `terminal causal reply deadline`. The existing assertion asks for a
   failure path this interleaving does not reach.
4. The suffix-order case waits for `Snapshot.bytes_read` to advance before it
   sends `release`. The worker increments its private counter when it reads, but
   copies that counter into a public snapshot only in `publish_viewport()`. The
   deferred query does not publish a frame while it is waiting behind input.
   The wait consequently reaches the failure snapshot at the 4 s ordering
   deadline; its predicate is checked before the failed-state check, and the
   subsequent resize returns `Closed`.
5. There is a further latent byte-count mismatch. With three reserved queue
   slots, 13 `p` chunks are admitted before the child emits its query. The test
   currently expects `(completed + 1) * 4096` bytes before the reply, which
   counts the completed chunks and one partial active chunk but omits the other
   already-admitted chunks. Under the ordering fence those earlier inputs must
   precede the query reply. On the probed Darwin capacity, `completed == 0`, so
   the current expectation is 4096 bytes while all 13 admitted chunks (53,248
   bytes) precede the query output.

## Source anchors

- [`engine.rs`](../../../../../crates/session-runtime/tests/engine.rs:196): the
  backpressure helper admits the reserved number of 4096-byte chunks and treats
  100 ms without receipt completion as blocked quiescence.
- [`engine.rs`](../../../../../crates/session-runtime/tests/engine.rs:296): computes
  the suffix test's prefix as `completed + 1`; lines 303-308 wait on the
  published byte count before resize; lines 354-371 expect the causal-reply
  timeout for the deferred-output case.
- [`lib.rs`](../../../../../crates/session-runtime/src/lib.rs:1397): deferred work
  advances only when there is no pending write. Lines 1457-1474 distinguish a
  pending-input deadline, a causal-reply deadline, and a deferred-output
  ordering deadline. Lines 1514-1542 read PTY output and defer it while an
  earlier write remains pending.
- [`lib.rs`](../../../../../crates/session-runtime/src/lib.rs:1981): output deferred
  behind admitted input receives the bounded ordering deadline. Lines
  2036-2089 advance queued actions in order; lines 2113-2120 create a causal
  reply only after the output reaches the terminal owner. Lines 2300-2304 copy
  `bytes_read` into the public snapshot during publication.
- Hosted receipt and logs are retained under
  `/tmp/zmux-osc7501-linkage-43335306931/`, especially
  `receipt.json`, `probe-pty-capacity.log`, and `execute-session-arm64.log`.

## Minimum robust repair

Keep the existing deadlines, input order, bounded queue behavior, deferred
publication, and abort/recovery rules. Do not enlarge timeouts to make the
fixture pass.

1. Make the deadline test match its actual interleaving: with pre-admitted input
   holding the query behind a blocked write, assert the deferred-output ordering
   deadline. If Session causal-reply write expiry is a required contract, cover
   it in a distinct real-PTY case that demonstrably reaches `CausalReply` before
   its write stalls.
2. Give the suffix-order test a reliable, production-owned read-progress
   barrier. The smallest candidate is a progress-only snapshot update for
   deferred PTY reads: advance `bytes_read` while retaining the last committed
   viewport, status records, and Session state. Wait until the complete query
   payload has been read before enqueuing the post-query suffix. This must not
   commit the deferred terminal batch or release its reply early. If the
   existing snapshot contract should not expose that progress, redesign the
   test around another public Session boundary instead of adding a test-only
   production seam.
3. Calculate the pre-query prefix from all admitted chunks, for example
   `(completed + pending_receipts.len()) * 4096` at the current helper return,
   rather than assuming only one chunk can be outstanding.

These are real-PTY Session-owner behaviors and remain worth testing. The fix
should preserve tests for old committed publication, resize ordering, reply
before post-query input, partial-write accounting, close/deadline abort, no
replay, and cleanup. The current evidence does not show a Ghostty-specific
ordering defect; it shows that these two existing tests cannot complete their
handshake under the measured Darwin capacity and current snapshot publication
semantics.

## Local proof and limits

- On the Linux host, the unlinked default-engine real-PTY test
  `default_real_pty_orders_query_before_da_and_retires_only_after_final_drain`
  passed 1/1 with `CARGO_TARGET_DIR=/tmp/zmux-osc7501-linkage-diagnosis-target`.
  The checkout had advanced to `b3334d2` during this read-only investigation;
  the lockfile and relevant Session, terminal-adapter, and program-status source
  and test hashes still match the `9048d94` hosted receipt.
- No macOS host is available locally, so the failing Ghostty-linked Session
  tests and linked acceptance test were not reproduced locally. The hosted
  receipt establishes compile and the preceding ARM64 execution results only;
  it does not establish the skipped Session acceptance test, Intel execution,
  or app delivery.
- No source, test, index, workflow, build artifact, or running application was
  changed by this diagnosis.
