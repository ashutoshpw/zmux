# macOS round-two Codex attachment diagnosis

**Date:** 2026-11-12

**Scope:** Read-only diagnosis of Foundation run `43315039608` on source
`7bf0864`. No source or test edits, workflow actions, commits, or pushes were
made for this diagnosis.

## Finding

The primary failure has moved past the automatic route-start stage. The fixture
reaches `CODEX_FIXTURE_READY`, receives the Codex SessionStart thread ID, and
then fails while trying to attach its watcher. The most specific source match
for the terminal's `waiting for zmux ... control.sock failed: request deadline
elapsed` is the Zmux `ProviderObservation` RPC that publishes the initial
Codex baseline from `Watcher::attach`.

That RPC still receives the CLI's 150 ms lifecycle deadline. Desktop now
asynchronously checkpoints `ProviderObservation` against the Session worker,
which has a two-second checkpoint cap. The observation envelope does not carry
an absolute deadline or caller cancellation into that checkpoint. A checkpoint
or Desktop scheduling delay can therefore outlast the CLI's request, after
which Desktop may continue the operation and try to acknowledge a closed
control socket. This is the strongest source-supported explanation for the
observed timeout and `Broken pipe`; the hosted log does not establish that this
interleaving was the actual macOS cause.

## Hosted evidence

`/tmp/zmux-osc7501-foundation-macos-round2.log:446,449` records the control
server's `Broken pipe` and the primary assertion at
`apps/cli/tests/provider_hooks.rs:2359`. The terminal output at line 449
contains `CODEX_FIXTURE_READY`, then:

> Codex thread could not be attached to the existing app-server (waiting for
> zmux at <fixture>/Library/Application Support/zmux/control.sock failed:
> request deadline elapsed; the command was not retried); activity remains
> Unknown

The synthetic producer reports `phase=connected;error=peer_closed` while the
daemon PID and socket remain alive. That is consistent with the watcher
closing its app-server connection after attach returned an error. The
subsequent provider-hook failures are mutex-poison cascades from this primary
assertion. Foundation Linux passed; that does not rule out a macOS scheduling
or control-worker delay.

## Exact operation and budget path

1. Codex SessionStart sends its thread ID to the private provider-event socket
   (`apps/cli/src/hooks/provider.rs:148-219`). That socket has its own 100 ms
   connect/write bounds. The outer supervisor receives the notification and
   calls `codex::Watcher::attach` (`provider.rs:746-762`). The fixture's
   `CODEX_FIXTURE_READY` and subsequent attach diagnostic show this path
   progressed past private event delivery.
2. `Watcher::attach` connects to the existing Codex app-server, obtains the
   thread snapshot, then publishes a baseline before returning
   (`apps/cli/src/hooks/provider/codex.rs:289-328`). `publish` calls
   `submit_observation` (`codex.rs:533-560`; `provider.rs:849-879`), which
   sends `LifecycleAction::ProviderObservation`.
3. `submit_observation` sets its deadline to
   `now + min(options.timeout, LIFECYCLE_BUDGET)`. `LIFECYCLE_BUDGET` is 150 ms,
   and `lifecycle_operation` clamps it to 150 ms again
   (`hooks/provider.rs:860-873`; `hooks.rs:31,1181-1206`). The CLI control
   client applies that one absolute deadline through connect, writes, and
   response reads. The exact `waiting for zmux ... request deadline elapsed`
   wording is produced when its response read exhausts that deadline
   (`apps/cli/src/main.rs:827-831,875-879`). The RPC is not retried.
4. In the Desktop control owner, mutations use `COMMAND_TIMEOUT = 2 s`
   (`crates/desktop-runtime/src/control.rs:34-35,1198-1214`).
   `execute_authorized` puts a reply deadline and cancellation flag on the
   envelope only for `ProviderRouteStarted`; other actions receive neither
   (`desktop-runtime/src/lib.rs:908-948`). The worker checkpoints
   `Register`, `Event`, `ProviderObservation`, and `ProviderRouteStarted`
   (`lib.rs:2991-3037`). It starts a fresh two-second checkpoint deadline
   after the envelope reaches the worker, and only `ProviderRouteStarted`
   derives its cutoff from the envelope's reply deadline
   (`lib.rs:595-596,3005-3037`). The poller also applies the expiry and
   cancellation checks only to route-start work (`lib.rs:2591-2627`).
5. On checkpoint completion, Desktop resumes the saved local command
   (`lib.rs:2721-2732`). If the CLI has already timed out and closed its socket,
   the control response writer logs `zmux local control response failed` on
   `Broken pipe` (`control.rs:524-525`). The log has no request ID linking that
   line to the observation, so the association is plausible, not proven.

The Codex app-server WebSocket is a separate transport with its own two-second
`RPC_TIMEOUT` (`codex.rs:29,636-648`). The hosted error explicitly names the
Zmux `control.sock`, and the source maps that exact message to the Zmux response
read. This distinguishes the observed timeout from the private 100 ms event
socket and from the Codex WebSocket timeout. The log alone cannot say how long
the snapshot or Session checkpoint took.

`Register` has the same 150 ms CLI helper and is also checkpointed by Desktop
(`apps/cli/src/hooks.rs:1501-1516`; `desktop-runtime/src/lib.rs:2991-3037`).
It is a related budget-contract concern, but the failing Codex attach path
publishes `ProviderObservation`; this fixture does not establish a `Register`
failure. Desktop also checkpoints lifecycle `Event`. This Codex SessionStart
notification uses the private provider-event socket rather than
`LifecycleAction::Event`, so the hosted failure does not exercise that
ordinary hook path.

## Recommendation for repair

Use a narrow deadline class for checkpoint-backed authoritative provider state
operations, beginning with the evidenced `ProviderObservation` and its
analogous `Register` action:

- Establish one caller-side absolute deadline, capped by the existing
  `EVENT_BUDGET` of 2.7 seconds, and carry it through CLI control socket I/O.
  Reuse an existing hook deadline when one is already available; do not restart
  that client budget inside a later helper.
- At Desktop control admission, establish the existing two-second command
  acknowledgement deadline before queueing these actions, with a cancellation
  flag like route start already uses. Cap the Session checkpoint at the earlier
  of its existing two-second limit and the remaining command deadline minus a
  response margin. Do not start a fresh two-second checkpoint timer after
  worker-queue delay. If the Desktop command deadline expires, cancel pending
  checkpoint work and do not apply the observation or registration afterward.
  The bounded CLI outer deadline leaves room for the Desktop acknowledgement
  and response transport.
- Keep the private provider-event transport at 100 ms, the ordinary lifecycle
  hook/event and cleanup limits at 150 ms, and the checkpoint queue bounded.
  Do not retry or replay the provider process, and do not lengthen the shared
  lifecycle helper for every action. If lifecycle `Event` is intended to wait
  on the same checkpoint, its latency contract should be decided separately;
  this log does not implicate it.

## Proof limits

The source establishes a concrete 150 ms versus asynchronous-checkpoint budget
mismatch and that the failing control read received no response before its
deadline. The hosted evidence does not record request timing, checkpoint start
or completion, Desktop queue delay, or a request ID for the `Broken pipe`.
Consequently it cannot prove the Session checkpoint exceeded 150 ms, identify
macOS scheduling as the cause, or prove that the broken-pipe response belongs
to this exact observation. No native macOS reproduction was available in this
diagnosis. No tests or aggregate checks were run here.
