# macOS local Codex route admission diagnosis

**Date:** 2026-11-12
**Scope:** Read-only diagnosis of Foundation run `43299857515`, macOS job
`131467797030`, on integration source `3b385182e43e66bde01100640b9c6cf213eac048`.
The only edits from this investigation are this reference and its task-status
link. I did not edit source, test, or index paths; concurrent shared-worktree
changes were preserved. Current `HEAD` is
`284b00fac59bb1fee1e478f500b1bd84ed5e2054`; the inspected route, control,
checkpoint, and fixture source paths have no diff from the hosted run's
integration commit.

## Finding

The strongest source-supported explanation is a deadline mismatch introduced
by asynchronous Desktop checkpoint admission. The local provider CLI gives a
pre-spawn `ProviderRouteStarted` request at most **150 ms end to end**, while
Desktop may leave that request pending for a Session worker checkpoint with a
**2 s deadline**. A valid checkpoint can therefore complete after the CLI has
already abandoned the request and entered its untracked fallback. Desktop then
tries to write its response to the closed local-control socket, matching the
hosted `Broken pipe` diagnostic.

This is a concrete, plausible root-cause class, not proof that the checkpoint
itself took more than 150 ms in this macOS run. The log contains no elapsed
time, checkpoint outcome, or client-side IPC error detail. Desktop command
queue delay or host scheduling delay could produce the same late response.
There was no native Mac reproduction in this investigation.

## Hosted symptom

The primary failure is
`automatic_local_codex_uses_its_exact_real_desktop_terminal_route_child` at
`apps/cli/tests/provider_hooks.rs:2334`. The shell and synthetic Codex daemon
start, then the CLI prints:

> automatic codex tracking unavailable (Desktop did not admit the pending provider route); launching once without lifecycle adapter

The control server logs `zmux local control response failed: Broken pipe`
(`crates/desktop-runtime/src/control.rs:524-525`). The child never produces
`CODEX_FIXTURE_READY`, so the enclosing assertion fails after its 15 s fixture
wait. The wrapper then fails at `provider_hooks.rs:2199`. The 13 later
provider-hook failures are mutex-poison cascades, not separate findings.
Evidence is `/tmp/zmux-osc7501-foundation-macos-failure.log:428-457`.

Foundation Linux passed, which is useful cross-platform evidence but does not
refute a macOS scheduling-sensitive deadline race. The Foundation failure is
not a provider-event receive failure: it occurs before the tracked provider is
spawned.

## Exact pre-spawn path and budgets

1. For local Codex, `crates/session-runtime/src/shell/provider.sh:14-19`
   invokes the shipped CLI's `hooks provider auto codex` command. This is the
   path exercised by the real Desktop fixture at
   `apps/cli/tests/provider_hooks.rs:2315-2324`.
2. `automatic_launch` in `apps/cli/src/hooks/provider.rs:240-258` first
   discovers and verifies the exact terminal. It then creates the provider
   event listener and launch configuration (`:283-347`). Before
   `command.spawn()` (`:387` onward), it calls
   `submit_provider_route(..., Started)` (`:349-363`). If that request returns
   an error, the code submits the exact `Cancelled` inverse and launches once
   without the lifecycle adapter (`:364-384`).
3. `submit_provider_route` caps its deadline with
   `options.timeout.min(LIFECYCLE_BUDGET)` at
   `apps/cli/src/hooks/provider.rs:910-917`. `LIFECYCLE_BUDGET` is 150 ms at
   `apps/cli/src/hooks.rs:31`; `lifecycle_operation` clamps it again at
   `hooks.rs:1181-1197`. The generic local-control request starts one absolute
   deadline and applies it across socket connect, request writes, and framed
   response reads (`apps/cli/src/main.rs:750-830`). Thus this is a 150 ms total
   route-start RPC budget, not 150 ms for each I/O phase.
4. The Desktop control server uses `COMMAND_TIMEOUT = 2 s`
   (`crates/desktop-runtime/src/control.rs:31-36`) for mutations
   (`:1198-1214`). `Desktop::execute_authorized` waits that long for the
   executor acknowledgement and sets a cancellation flag for a timed-out
   route start (`crates/desktop-runtime/src/lib.rs:907-941`). The server's
   `CLIENT_IO_TIMEOUT = 500 ms` is only its socket frame read/write timeout;
   it does not bound the time spent waiting for the Desktop command result
   (`control.rs:486-525`).
5. Desktop `command()` recognizes `ProviderRouteStarted`, requests a Session
   checkpoint with a separate 2 s deadline, stores the original envelope, and
   returns to its loop (`crates/desktop-runtime/src/lib.rs:2932-3005`,
   `PROGRAM_STATUS_CHECKPOINT_TIMEOUT` at `:595`). The loop polls pending
   checkpoints on its 16 ms update cadence (`:248`, `:1985-2005`). When the
   Session result arrives, Desktop validates the owner/launch, observes the
   checkpoint, resumes the saved lifecycle command, and only then provides its
   acknowledgement (`:2563-2651`, `:2680-2697`). Route admission binds the
   route to the checkpoint's session and command epochs (`:3753-3791`).
6. Session checkpoint admission itself is nonblocking: it uses
   `input_admission.try_lock()` and `try_send()`, then wakes the worker
   (`crates/session-runtime/src/lib.rs:896-930`). Once queued, the worker holds
   the checkpoint as an ordered barrier and waits for a PTY read drain or EOF,
   deferred output, pending writes, and causal replies to clear before
   publishing (`:1872-1941`). A Session command performing this ordered work
   can legitimately take longer than the CLI's 150 ms cap while remaining
   within its own 2 s deadline.

The two 2 s Desktop limits are not a nested guarantee: the control command's
acknowledgement timer starts before the Desktop worker schedules the Session
checkpoint, while the checkpoint timer starts later. The control timeout can
therefore fire first. Neither limit is visible to the CLI's 150 ms request.

## Concrete late-response interleaving

1. The shell wrapper sends `ProviderRouteStarted` before spawning Codex.
2. Desktop accepts the control request. Its executor queues the Session
   checkpoint and retains the acknowledgement envelope for asynchronous
   completion.
3. The Session worker drains already-read/available PTY and deferred work; the
   ordered checkpoint result arrives after the CLI's 150 ms absolute deadline
   but before the Desktop command's 2 s wait expires.
4. The CLI closes its request socket, treats route admission as unavailable,
   sends a best-effort cancellation, then starts the one untracked fallback.
   The control listener is single-threaded and calls `serve_connection`
   synchronously (`control.rs:330-337`), so it cannot accept that cancellation
   connection until the original command handler returns.
5. Desktop completes the checkpoint and route command. The control server
   attempts to write the response to the already-closed socket and logs the
   observed broken pipe. If the checkpoint is still pending at the server's
   2 s timeout, `execute_authorized` instead sets the route cancellation flag;
   that branch is also later than the CLI's 150 ms deadline.

The log proves a late response write after peer closure. It does not distinguish
step 3 from other Desktop executor delay, and it does not prove whether a late
route was briefly admitted before the queued inverse was processed.

## `provider-launch.py` is a different IPC path

The local pre-spawn wrapper is the shell function that calls the CLI, not
`provider-launch.py`. That Python helper is selected by the shell function for
the managed-SSH path (`provider.sh:3-12`); the local fixture also uses it as a
Codex `SessionStart` hook after provider spawn (`provider-synthetic-codex.py:
463-479, 507-518`). Local `send_local_event` connects to the private
provider-event listener with 100 ms connect and write timeouts
(`apps/cli/src/hooks/provider.rs:204-220`). It does not send the pre-spawn
`ProviderRouteStarted` control request and cannot explain this early fallback.
Do not change that post-spawn event deadline as a substitute for fixing the
route admission contract.

## Smallest robust repair direction

- Preserve the 150 ms budget for ordinary lifecycle hooks and provider events.
  Give only the pre-spawn `ProviderRouteStarted` handshake a separate,
  explicitly bounded budget; the automatic-launch outer options budget is
  already capped at 2.7 s (`EVENT_BUDGET`, `hooks.rs:30`).
- Make the budgets nested or derive them from one absolute route-start
  deadline: Session checkpoint deadline, Desktop command acknowledgement
  timeout, then CLI response deadline, with bounded scheduling/response
  margin at each outer layer. Today the equal 2 s Desktop values start at
  different times, and the CLI's 150 ms limit is shorter than both.
- Preserve fail-closed behavior: do not spawn the tracked provider until the
  exact route is acknowledged; on expiry, retire any pending start before or
  alongside the existing single untracked fallback. Avoid a retry of the
  provider process or a global lifecycle timeout increase.
- Validate at the existing production boundary with the real Desktop/control
  fixture and a real-PTY checkpoint interleaving: a checkpoint delayed beyond
  150 ms but completed within the bounded route-start window must still bind
  the exact current epoch before spawn. Also retain an over-deadline/failure
  case proving one fallback and eventual route cancellation, and verify the
  ordinary 150 ms lifecycle path remains capped. No sleep-only timing claim or
  fabricated checkpoint receipt is needed.

## Proof limits and work not performed

- The hosted Foundation log identifies the failing macOS test and response
  write error, but contains no timestamps for request send, checkpoint
  admission/completion, or response deadline. It cannot prove the precise
  interleaving or make the 150 ms mismatch the sole cause.
- `try_lock`/`try_send` can return `Busy`/queue failure immediately. Such a
  fast result should normally let the control server reply within the client
  budget; it does not fit the observed late response as directly as a pending
  checkpoint, though executor delay remains possible.
- No test or experiment was run here because the parent full `scripts/check`
  was in progress. No native Mac reproduction was available. Linux Foundation
  success is not Mac execution evidence, and the diagnosis is not a repair or
  acceptance result.
- I made no app, source, or test edits; no commit, push, workflow, or app action
  was performed. Concurrent shared-worktree changes were preserved.
