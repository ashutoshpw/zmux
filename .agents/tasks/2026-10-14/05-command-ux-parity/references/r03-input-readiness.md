# R03 authoritative frontend input readiness

Read-only design, 2026-10-16. No production implementation is authorized by this
document. Workload V2 execution admission, native CLI fork-to-exec identity repair,
durable completion, and frontend input readiness are separate contracts.

## Recommendation

Extend the pinned MIT Zellij dependency with an optional authenticated lifecycle
channel. Publish one `InputReady` event only after both client-owned conditions:

1. The startup ANSI-query input gate is closed: the main client consumed
   `DoneParsingStdinQuery`, and the actual shared `StdinAnsiParser::should_parse()`
   is false under its existing mutex. Do not infer this from elapsed time outside
   the client, responses written by the emulator, or `TransportConnected`.
2. The main client has processed its first typed server `ClientInstruction::Render`
   after loading, and every corresponding stdout write and flush succeeded.

The event comes from these internal state transitions, never from parsing terminal
text or escape markers. Leave the upstream query policy's own timeout intact;
Zmux adds no startup sleep and does not copy its 500 ms constant. Cache-hit startup
still goes through `DoneParsing` and the same two-condition admission.

`TransportConnected` stays transport progress. The native launch owner must not
return an interactive `ClientReady`, install a writable Session, or describe the
frontend as input-ready until it validates this new event. If output needs to be
published earlier, install a read-only Session with one shared startup input fence;
do not create an alternate writable path. The narrower initial implementation can
continue retaining the Session on its existing launch worker until readiness.

## Source evidence and why both conditions matter

Inspected source is retained under
`target/agent-validation/zellij-patched-0.43.0-v6/source/` followed by
`zellij-23bb542548d517f7f20a7fdd14a943f1a610df37/`. It is pinned upstream MIT Zellij
0.43.0 plus the already recorded stdin EINTR repair. No cmux code or assets were
consulted for this design.

| Owner and source location | Observed contract |
| --- | --- |
| `zellij-client/src/lib.rs:303-357` | Connects and sends `AttachClient` before starting the stdin/input threads. A socket connection cannot prove these threads are initialized. |
| `zellij-client/src/stdin_handler.rs:33-80` | A cache miss emits queries, starts the query timer, then deliberately parses and drops other stdin bytes while `should_parse()` is true. A cache hit queues `DoneParsing` directly. |
| `zellij-client/src/stdin_ansi_parser.rs:59-100` | Query emission arms a 500 ms deadline. `should_parse` tests that actual deadline; receiving all expected replies does not close it early. This source has no later caller that rearms `terminal_emulator_query_string`. |
| `zellij-client/src/input_handler.rs:227-235` | Relays `StartedParsing`/`DoneParsing` to the main client instruction queue. |
| `zellij-client/src/lib.rs:520-554` | Buffers ordinary client instructions while loading. Consuming `DoneParsingStdinQuery` changes `loading` to false. Human text `done` is incidental output, not a control protocol. |
| `zellij-server/src/route.rs:973-1013` | A key arriving before client keybind/mode registration can be logged and dropped. Timer completion alone is weaker than attachment completion. |
| `zellij-server/src/lib.rs:800-881` | Processes `AttachClient` asynchronously, installing saved/runtime configuration and input mode before sending Screen `AddClient` and focus instructions. |
| `zellij-server/src/screen.rs:4410-4454`; `tab/mod.rs:1049-1093` | Adds/focuses the client and forces/render-schedules its view. A real render addressed to this client follows that screen membership. |
| `zellij-client/src/lib.rs:574-591` | Processes typed server `Render` only after loading ends; writes its content and optional sync sequences, then flushes. This is the second condition's production owner. |

Current original Zmux evidence:

- `remote-runtime/local_zellij/broker.rs` publishes `observer.connected` when an
  authenticated frontend IPC connection exists. It intentionally forwards opaque
  bytes and does not parse the Zellij IPC protocol.
- `desktop-runtime/local_zellij/worker.rs` currently returns `Ready` on that
  `TransportConnected` frame after identity checks. Those checks are necessary
  but do not establish startup query completion.
- `desktop-runtime/zellij_coordinator.rs` then records `transport_connected` and
  installs the Session through the ordinary launch path.
- `desktop-runtime/lib.rs::writable_session` already owns shared input admission
  for text, keys, paste, application wheel, targeted writes, notification replies,
  and transferred-path insertion. Local history scrolling/resize are separate.
- `apps/macos/Sources/App.swift` wires terminal input callbacks whenever the pane
  has a Session ID. Presentation must explicitly reflect starting/unavailable
  input rather than silently swallowing keystrokes while this gate is closed.

There is no existing exported, authenticated input-ready event in the inspected
`ClientToServerMsg`/`ServerToClientMsg` enums. `Connected` is a liveness response;
`UnblockInputThread` is reused for command execution and is not initial admission.
`TerminalResize` can originate from the signal thread, so observing it is not a
correlated query-completion acknowledgement. No existing public CLI listing or
cache file is a substitute.

## Bounded channel and authority

Create one short, private readiness socket and regular descriptor in the existing
per-attachment `a<UUID>` namespace before spawning the frontend. This is an
additional channel owned by the retained foreground CLI, not a daemon, a new
server, an inherited FD that portable-pty might close, or an arbitrary socket
selection API. Keep the broker's existing opaque IPC relay unchanged.

Recommended separate readiness protocol version: **1**. Its bounded descriptor
names a fresh attachment UUID, owner UUID/generation, one-use random nonce, exact
CLI parent identity, short endpoint, and pinned namespace/socket identities.
The readiness frame can be at most 1 KiB because kernel peer inspection supplies
the sender's full process identity; the descriptor can be at most 8 KiB. Strict
decoding rejects unknown/duplicate fields, invalid versions, wrong generations,
duplicate readiness, and trailing frames. Fix the allowed message count before
implementation; no user commands or terminal content belong in this channel.

Set the descriptor environment variable only on the exact frontend `Command`,
after the existing environment scrub. It must not enter the version probe,
original backend, supervisor, workload, startup command, registry, or saved
workspace. It carries no permission to create/exec a workload. Audit any
dependency frontend subprocess paths before implementation so the optional
descriptor cannot escape into another command's environment.

The CLI must authenticate the connected kernel PID/UID and full identity against
its directly retained Zellij Child, including current executable, parent, boot,
and start identity; then validate nonce, attachment generation, and pinned local
namespace. A same-UID sibling, old attachment, or plausible file cannot assert
readiness. Do not adopt a PID, look up a client by name, relax image equality,
retarget the endpoint, or recreate a missing original owner namespace.

Keep the frontend/control connection alive until its receipt has been
acknowledged or cancelled, so Darwin peer inspection never depends on a departed
short-lived publisher. A small reporter owned and joined by the frontend can
handle partial nonblocking writes and the acknowledgement without blocking the
main render/input loop. Its queue has capacity one; it never accepts user input.
No unbounded writer thread, detached retry loop, or new independent timeout owner.

The CLI forwards the validated readiness as a typed native observer event using
the existing exact observer context, authenticated CLI peer, owner identity and
attachment UUID. The native monitor revalidates the currently live configured
frontend identity and its exact operation/launch generation before admission.
This is an explicit trust chain: the native host authenticates its CLI, which
authenticates its Zellij Child. It is not an independent native query of the
Zellij client's internal state; that state's truth comes from the reviewed
dependency implementation.

## Observer versions and shared input policy

The concurrent V2 execution-admission owner confirmed its isolated contract:
native observer descriptor/wire **3**, legacy **1/2** preserved; descriptor 3 can
carry legacy V1 or admitted-lifetime V2 terminal facts. Its completion-control
version 2 is separate from legacy completion-control version 1.

Reserve observer descriptor/wire **4** with explicit input-readiness capability
negotiation. Do not add a frame that descriptor 3 silently treats as Ready, and
do not reinterpret descriptor 3 `TransportConnected`. V2 admission itself does
not enable interactive input. Resolve the exact version-4 schema with that owner
after its migration lands; this document does not mutate its types.

Inventory the legal frame budget. A new-Start completion path containing Reserved,
reservation ACK, Bound, TransportConnected, InputReady, WorkloadFinished,
completion ACK and FrontendFinished totals eight existing observer sequence
positions. If the chosen capability handshake adds observer messages, review the
bound rather than silently exceeding the current eight-message limit. Keep the
separate readiness channel's handshake outside this observer sequence.

Readiness must be requested only for a dependency that explicitly supports this
capability. A plain `--version` result of 0.43.0 is insufficient. A bounded,
read-only capability probe or the separately approved retained-package contract
must reject unsupported native selection before Start reserves/spawns an owner.
Ordinary foreground CLI use without this requested capability can preserve its
existing truthful transport-only behavior. There is no timeout fallback to
pretending a stock binary became input-ready.

Before readiness, refuse user writes explicitly at the common admission owner;
never report them physically accepted, buffer them invisibly, or replay them when
readiness eventually arrives. The initial policy should not introduce a pre-ready
user-input queue. AppKit must present this as terminal startup and return an
explicit unavailable result for keyboard/IME, paste, files/images and application
mouse/wheel attempts. Request-queue acceptance alone is not terminal delivery.
If preserving a typed draft is later desired, it needs its own bounded,
user-visible editor policy; it is not a transport recovery feature.

After readiness, keep existing bounded Session input queues, physical-write
receipts and causal ordering. QueueFull/Busy/partial/unknown delivery stays an
explicit result. Never retry an accepted text chunk, raw key, paste or opaque
Zellij IPC byte range after cancellation, reconnect, completion or failure.

## Terminal replies, completion and failure ordering

Ordinary emulator replies must continue during startup while user input is
fenced. Default vt100 does not return staged terminal reply batches. The optional
engine's `session-runtime` worker physically writes causal replies before
committing its associated frame and before loading later queued user input. Do
not route those engine-owned bytes through `writable_session`, the readiness
queue, the clipboard path or a user-input rejection policy. Preserve its existing
reply deadline, cancellation, partial-write and recovery rules.

Physical query-reply acceptance is not proof that Zellij has closed its query
parser. Conversely, `InputReady` does not establish that every optional capability
reply was understood, that there are no late protocol replies, or that a workload
has consumed a user byte. Rendering/capability fidelity and final input delivery
remain separate evidence. The new gate removes the known deliberate early-input
discard window without claiming a complete terminal protocol implementation.

| Event | Required result |
| --- | --- |
| Valid transport, no readiness | Progress only; no interactive Session or user write admission. Keep pumping output, replies, completion and cancellation. |
| Both dependency conditions, valid authenticated frame | Admit once for this attachment only, after the native operation's current cancellation/generation check. Reattach always needs its own proof. |
| Workload completes before readiness | Authenticate/save/ACK completion immediately through the existing separate completion deadline. Never wait for a first render or manufacture InputReady. Return a typed terminal-completion handoff that retains actual frontend/Session cleanup ownership without admitting user writes. |
| First render/write/flush fails | No ready frame; actual child failure/cleanup is reported. A readable loading message, partial frame, or queried capability cannot override it. |
| Cancellation/shutdown before or concurrent with readiness | Cancellation wins the admission check. Cancel only retained CLI/frontend resources, join reporter/monitor/Session, keep the original owner/result policy, and never replay Start/input. |
| Missing/bad/duplicate/full-channel readiness | Fail closed with bounded explicit unknown/not-ready diagnostics; no interactive fallback. Keep completion evidence independent. |
| Original startup deadline expires | Use the existing fixed 10-second native startup budget; do not reset it on progress, receipt chunks or retries. Bounded cleanup is separate from authorizing input. |

Track transport observed, frontend identity, interactive admission, terminal fact
and physical joins separately. The existing `frontend: Option<ProcessIdentity>`
must not become a hidden readiness flag: a real frontend can exist without
readiness, and completed-before-ready can still require its physical cleanup.
No blocking wait belongs on the UI thread or core action executor.

## Smallest strong verification plan

The primary real acceptance is the existing native Rust/Desktop + real CLI + real
patched Zellij path with a **silent** retained workload. Wait for the new typed
`input_ready` receipt, immediately submit one unique line exactly once through
ordinary Desktop input, and have the workload record the exact line and count in
a private file/control pipe. Require one matching delivery, original workload
startup count one, and unchanged frontend/owner identities. Do not wait for a
printed workload marker, viewport substring, extra key, or wall-clock sleep.
Then exercise retained frontend detach/reattach and send one different line only
after that attachment's fresh readiness proof. This proves usable declared
readiness without using workload output as its oracle.

Add the focused production monitor/coordinator regression that supplies valid
transport progress while withholding the distinct readiness event: no
interactive admission or input callback may succeed. This should fail on today's
TransportConnected-to-Ready behavior for the intended reason. It tests the shared
admission boundary; it does not pretend to validate dependency internals. Preserve
its red result before the fix and retain the real end-to-end acceptance above.

Necessary controls, shared/table-driven where practical:

- Query-active/no-ready and post-query/no-first-render both remain noninteractive;
  the normal uncached managed attachment is the primary real branch. Cover the
  upstream cache-hit branch at its dependency owner without inventing emulator
  capability facts or claiming managed attachments reuse their fresh cache root.
- User text/key/paste, application wheel, notification reply and file/image path
  insertion are explicitly refused before Ready; emulator replies still progress.
  Hold the real ready event rather than racing an arbitrary sub-500 ms assertion.
- Missing capability, wrong peer/nonce/attachment/generation, duplicate frames,
  truncated/oversized frames, full queue/backpressure and first-render failure do
  not admit input. Bound child/control cleanup on every failure.
- Cancellation and completion before Ready preserve no replay, actual saved
  completion authority and physical joins. Later readiness cannot clear a terminal
  completion fence or resurrect an old operation.

Pure protocol tests cannot replace actual first-line delivery. The silent-workload
test must be demonstrated red/green where it exposes actual loss; if scheduler
timing hides that loss, do not claim a deterministic transport-timing red. The
withheld-readiness admission regression supplies the deterministic shared-owner
red, and dependency state tests independently guard the two emission conditions.

## Dependency implementation and provenance gates

This requires a new original patch to pinned MIT `zellij-client` (optional bounded
reporter/capability and emission at the two client conditions), alongside the
existing EINTR patch. Server IPC decoding/modification is not required by this
proposal. Do not edit the retained v6 source/binary or reuse its old receipt for
the changed producer.

Extend the explicit source producer, source bundle/rebuild instructions,
hash-checked patch inventory, component notices and acceptance tooling. Preserve
upstream archive/lock identity and existing native EINTR/owner controls. New
dependencies, if any are needed for safe peer/descriptor APIs, need explicit pin,
license and source-inventory review. Rebuild only named source products in the
nominated cache; the earlier stale-artifact producer failure remains relevant.
Record a new producer/binary receipt and native architecture checks. Existing
stock-binary acquisition pins, app packaging and native Start/Attach activation
remain unchanged until separately authorized.

Pinned inspected query/input sources, for later exact patch review:

```text
zellij-client/src/lib.rs                0f28391f1faa3e3d785f61959107072619cf5d7c20a4563010a7f47b818e95e5
zellij-client/src/stdin_handler.rs      6a202011eb4ae118517270727ca55b259e4e202cd6b0a1704a12c3d9405f0967
zellij-client/src/stdin_ansi_parser.rs  240e3c32a69143000573cd5c57cd962155d36cb6b3401aa03019fd85b63896b9
zellij-client/src/input_handler.rs     1155f517d5df2dbad280c626da3fd3cc943f5d0f5a737547a5b1d1aa7326434a
zellij-client/src/os_input_output.rs   319421a17ad8d177db106ee0f862da8d42093698a8b0a06b9d634eb25b34c66b
```

No new test, dependency build, runtime mutation, package publication or native
activation was performed for this design.
