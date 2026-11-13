# Managed SSH Codex route-admission diagnosis

**Date:** 2026-11-13
**Source:** `6230bd4fb6842d15901f1c02daab7fa91dfb9dea`
**Hosted evidence:** Foundation macOS job `131795589236`, run `43398620022`; full decoded log at `/tmp/zmux-osc7501-final-6230bd4/foundation-macos-failed-job-full.log` (SHA-256 `b01d36619b7151b3df37aed699de0cfbc3d63d10bb3f8834f7238d3b5f412af3`).

## Verdict

The test's `CODEX_FIXTURE_READY` wait is not a valid readiness signal for the
untracked fallback branch. The fake Codex prints it only after its SessionStart
hook successfully writes to the per-process provider-event socket. A route
admission failure intentionally launches Codex once without that hook config or
socket, so that branch cannot print the expected marker. This explains why the
test can time out after the wrapper has selected fallback; it does not prove the
provider was never launched.

The immediate reason Desktop route admission was unconfirmed remains unknown.
The log records lifecycle startup retries that ended before the Desktop reader
observed `Hello` or `Active`. The test had waited only for the terminal PTY to be
`running`, and the producer reads the lifecycle generation marker with no wait.
That establishes a startup-ordering race candidate, but the retained log has no
route-stage or generation-marker diagnostic to prove this was the exact cause.
The later `EPERM` cancellation messages do not establish why the stream ended.

No production timeout or retry change is supported by this run. In particular,
the local `submit_provider_route` budget repair is a separate path from this
remote Python producer's `ProviderRouteReceipt` gate.

## Observed interleaving

1. `apps/cli/tests/ssh.rs:323-336` waits for the newly opened PTY to report
   `state == "running"`, then sends `codex fixture-daemon-start && codex`. It
   does not wait for the managed lifecycle stream to be active.
2. In the hosted terminal snapshot, `fixture-daemon-start` reports the synthetic
   app server as running. The managed wrapper then logs
   `Desktop provider-route admission was not confirmed` and selects its
   one-time untracked fallback.
3. The lifecycle supervisor reports two startup retries with
   `hello=false, active=false`. The source accepts a route frame only after the
   remote stream's Hello/Activate/Active handshake, so those attempts did not
   reach Desktop route admission.
4. `screen()` times out at `apps/cli/tests/ssh.rs:166` while the terminal remains
   running at a shell prompt with no `CODEX_FIXTURE_READY`. The snapshot is
   consistent with the fake provider returning before its SessionStart hook,
   not evidence that the remote shell or PTY failed to start.

The hosted log contains no provider route ID, lifecycle generation value,
desktop checkpoint result, remote receipt send/receive result, or elapsed time
for `begin_provider_route`. It therefore cannot distinguish a missing generation
marker, FIFO write failure, desktop rejection/checkpoint failure, or lost/late
receipt. The `EPERM` lines are emitted during lifecycle child cancellation near
the stream-close reports; there is no evidence that cancellation caused the
admission failure.

## Source path and timing

| Owner | Admission step | Bound and meaning |
|---|---|---|
| `provider-launch.py:93-121,150-235` | Bind an exact 0600 datagram endpoint; read the current lifecycle generation with `wait_seconds=0`; write a `starting` frame to `agent-events`; wait for an exact nonce, generation, route-ID receipt. | `PROVIDER_ROUTE_RECEIPT_TIMEOUT = 2s`, starting after the FIFO write. Missing generation returns immediately. On reject or timeout, emit `cancelled`; on no accepted receipt, run the raw Codex command once. |
| `bootstrap.sh:97-104`; `claude-lifecycle-stream.py:48-105,447-512` | Bootstrap creates the lifecycle FIFOs. The remote reader opens them, sends Hello, validates Activate, publishes `agent-lifecycle-generation`, then sends Active. | FIFO discovery may wait up to 20s. The Desktop supervisor's first-handshake deadline is 30s (`remote_agent_lifecycle.rs:38,424-449`), but `begin_provider_route` does not wait for that readiness. |
| `claude-lifecycle-stream.py:597-660` | After activation, validate the starting route and endpoint, retain a pending receipt, then forward a sequenced `ProviderRoute` frame. | The remote pending-receipt window is 3s. It is independent of the producer's 2s wait. |
| `desktop-runtime/src/lib.rs:2276-2290,2313-2352,2495-2550` | Queue a worker-ordered Session checkpoint for route start. Admit only from that checkpoint and exact target/transport identity; queue an accepted or rejected receipt. | Checkpoint deadline is 2s. `send_provider_route_receipt(true)` proves bounded local queue admission only, not remote delivery. |
| `remote_agent_lifecycle.rs:619-739,814-835,869-909` | After Active, write queued receipts to SSH stdin; discard stale bindings and bound each client-frame write. | Client-frame write deadline is 2s. A failed or absent receipt leaves the producer on its single untracked launch path. |

These are independent relative timers, not one propagated end-to-end deadline.
The 2s producer wait can expire while a remote checkpoint or receipt write is
still pending; the helper's 3s pending window does not extend the producer's
wait. That is a source-level risk under slow admission, not a demonstrated cause
here: the logged startup attempts did not reach Hello/Active, so the log does not
show a route frame entering the checkpoint/receipt path.

The CLI route-start repair's 2700ms `EVENT_BUDGET`, 150ms ordinary lifecycle
budget, and 100ms private provider-event transport are separate local/observer
contracts (`apps/cli/src/hooks/provider.rs:885-929`, `apps/cli/src/hooks.rs:30-31`).
This managed SSH wrapper does not call that CLI `submit_provider_route` path.
Keep those budgets, the route checkpoint ordering, bounded queues, exact
generation retirement, and one-provider/no-unknown-replay behavior unchanged.
The route receipt described here is also distinct from per-hook receipts.

## Test-owner finding and recommendation

The marker coupling is confirmed by source:

- `provider-launch.py:981-1043` adds the SessionStart config and
  `ZMUX_PROVIDER_EVENT_SOCKET` only for the tracked child. On failed admission,
  `codex_untracked()` calls the executable once without either injection.
- `provider-synthetic-codex.py:463-479,507-518` manually invokes the hook and
  prints `CODEX_FIXTURE_READY` only if it returns success.
- `write_hook_identity()` rejects a missing event-socket path
  (`provider-launch.py:343-374`).

The smallest fixture correction is to put a bounded readiness barrier before
the Codex command, using the existing nonce-scoped
`$ZMUX_SHELL_INTEGRATION_DIRECTORY/agent-lifecycle-generation` marker that is
published after Activate. If the marker does not appear, fail at that barrier
without starting Codex. Have the synthetic executable print a separate process
entry marker before its hook so an untracked launch is observable. Add a red
control that withholds lifecycle activation and proves the test does not mistake
PTY `running` for stream readiness or start the provider before the barrier.
Keep the separate fallback behavior one launch only.

If the barrier succeeds but route admission still falls back, the production
cause remains unclassified. The next evidence slice should add bounded,
allowlisted stage and elapsed diagnostics at `begin_provider_route` and the
remote receipt/checkpoint boundary (generation unavailable, FIFO write failure,
receipt timeout/rejection, or stale/malformed receipt). Do not log route tokens,
socket paths, provider payloads, raw IPC errors, or increase a timeout before
that evidence identifies a budget mismatch.

## Proof limits and preserved decisions

- Read-only inspection at the exact source commit and the supplied full hosted
  log; no test, build, workflow, application, source, status, plan, index,
  commit, or push action was performed. This diagnosis created only this
  reference; concurrent and pre-existing paths were left untouched.
- The fixture marker's dependency on successful hook ingress and the lack of a
  stream-ready precondition are source-proven. The particular reason for the
  hosted route-admission failure is not.
- Existing provider epoch/checkpoint semantics, absolute local Started deadline,
  final-admission rollback, and remote exact-generation retirement remain in
  force. See `provider-epoch-repair.md`, `route-start-budget-repair.md`,
  `route-final-admission-repair.md`, and `remote-route-retirement.md`.
- This review does not cover the separately assigned per-hook receipt fixture.
