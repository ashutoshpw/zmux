# R03 natural completion: live supervisor protocol proposal

Design recorded read-only on 2026-10-16; implementation evidence is appended
below. This addresses the successfully bound owner's normal finite-exit gap; it
is not native activation approval. The pre-binding fast-exit race and native readiness gate below remain
requirements, not waived scope.

## Recommendation and source basis

Use the original supervisor as a live authenticated terminal-result owner. Do not
add signing or a second daemon. Keep it alive after it has reaped its exact Child,
with a private completion endpoint and bounded durable terminal record. The
foreground CLI independently authenticates that endpoint against the full
supervisor identity captured at bootstrap. The native worker independently does
the same before accepting a Completed outcome. A private file on its own is never
authentication after the publisher has died.

Today `supervisor.rs::run_supervisor` observes and waits for its directly retained
Child, then immediately returns its mapped status. `create` authenticates the
supervisor and server, records the selected shell's full image/start identity,
persists the bound record and sends only a string `committed`. Bootstrap files and
socket are then deleted. `broker.rs::attach` and the native monitor can currently
prove only live preservation, so expected workload exit invalidates that proof.
The new protocol replaces that missing fact with a separately typed Completed
branch; it does not weaken `Record::verify_live`.

## Persistent identity and small schema

Add an immutable random nonnil `owner_generation` to new owner records and the
bootstrap. It is one lifetime of this owner, not an observer operation or Attach
launch generation. Persist a completion endpoint inode with the runtime root and
backend socket inode. Existing records lacking the capability remain live-only;
never infer or synthesize completion for them.

Replace the bootstrap string commit with a typed `CommittedBinding` containing:

- protocol version, owner UUID and owner generation;
- runtime root and backend socket device/inode identities;
- completion endpoint device/inode identity;
- the full original server, supervisor and workload `ProcessIdentity` tuples:
  PID, UID, parent PID, boot ID, start identity and executable path;
- a SHA-256 digest over a versioned canonical encoding of this binding.

The supervisor checks its current full identity and server parent, its retained
Child PID/generation and the agreed bound workload tuple before acknowledging this
commit. The starter persists the same binding before publishing Bound. The
existing authenticated one-use bootstrap channel carries it; it never enters the
workload environment or command text.

The immutable `TerminalReceiptV1` contains only:

```
version, domain = "zmux.local-zellij.terminal.v1"
owner_uuid, owner_generation, binding_digest
committed_binding
terminal_sequence = 1
workload_exit = { kind: "exited", code: 0..255 }
             | { kind: "signaled", signal: positive platform signal }
child_reaped = true
```

Only the physical supervisor constructs this value, after non-reaping terminal
observation and `Child::wait` agree. Stopped/Continued, ECHILD, a mismatching wait
status, failed binding commit, missing child identity or publisher identity drift
cannot construct it. No wall clock is an identity or ordering authority.

The workload tuple is the historical full identity actually agreed at binding.
The terminal fact means that the same directly retained Child lifetime was
reaped. It does not claim its executable image remained inspectable after death,
or that every intervening exec was detected. Live Attach still rejects changed
images. Supporting arbitrary live exec/layout changes remains a distinct reviewed
identity policy; polling cannot prove that no exec happened between samples.

## Durable publication and live authentication

Create `completion.sock` in the retained runtime root before `spawn_once`, and pin
its inode through bootstrap. Use a root directory FD plus no-follow relative
opens. Terminal body limit: 8 KiB; control frame limit: 16 KiB; at most four
simultaneous subscribed CLI consumers per owner, matching the current broker's
connection bound. A full control queue rejects a new subscription explicitly;
ordinary Child observation/reap must never wait behind a reader.

After observe/wait agreement, write one private exclusive sibling, sync the file,
publish `terminal-v1.json` without replacement, then sync the retained root
folder. Reuse the repository's existing descriptor-relative no-replace atomic
publication pattern. An existing byte-identical receipt is the same terminal
fact, not permission to run a workload again; different bytes for this generation
are an integrity failure. A visible replacement followed by failed directory sync
is unknown durability. Retain the supervisor and original bytes; do not overwrite,
replay Start or acknowledge durable consumption. No terminal writer touches S01's
workspace recovery path.

The live endpoint responds with the exact durable bytes and digest only after
successful sync. Each request has a fresh challenge echoed in its response plus
owner UUID/generation/binding digest. Request/response parsing is strict, bounded
and duplicate-field rejecting. Readers must validate kernel peer PID/UID/full
start/boot/image against the previously bound supervisor, current server lineage,
root/socket inodes and all receipt bindings. The supervisor also authenticates request-side consumers:
full live configured CLI identity, matching current observer/parent context for
native consumption, or the explicit local headless durable-consumption path. A
read-only status query does not grant consumption or retirement authority. A
readable terminal file is only storage consistency until that live peer confirms the same bytes/digest.

A restarted Desktop may use its trusted saved descriptor to resolve the fixed
profile and load the candidate owner record, but completion authority still needs
that exact currently live supervisor peer. A replaced/missing/reused PID, wrong
image/start/boot, replaced namespace/socket or unavailable peer is Unknown even
if a plausible terminal file exists. There is no automatic supervisor restart,
PID adoption, name lookup, Start replay or fall back to an ordinary shell.

## State transitions, status and resource cap

| State | Evidence and allowed behavior |
| --- | --- |
| Bound | Current existing live owner proof. No terminal result. |
| TerminalObserved | Retained Child terminal wait is known, durable write not yet confirmed. Not an acknowledged completed owner. |
| CompletedRetained | Immutable terminal file synced and live publisher confirms it. Workload reaped; original supervisor/root pane remain live. |
| ConsumedRetained | A native or local CLI consumer has durably recorded this exact receipt. Repeated consumption is idempotent. Publisher remains queryable. |
| Retiring | An explicit authorized cleanup request and durable consumption match the same receipt; all currently subscribed frontends have completed their own cleanup or disconnected. New subscription admission closes atomically before the subscriber/quiescence check; Busy reopens admission without self-exit. |
| Retired | Supervisor self-exited. Historical consumed receipt may remain, but absence cannot create a new live-authenticated completion claim. |
| Unknown | Missing source, identity drift, uncommitted binding, ambiguous terminal publication or crash without previously durably consumed evidence. No automatic recovery creation. |

The supervisor is the original Zellij root-pane command, so retaining it keeps
that pane/backend alive. Workload completion must therefore be signalled by this
control protocol, never by waiting for the pane/socket/server to disappear.

Use the existing profile registry cap of 64 total owner reservations, including
reserved, uncertain, running and completed-retained records. A completion consumes
its already reserved slot, so cap exhaustion never prevents recording an actual
exit. A full profile rejects new Start before reservation/child creation. Attach,
status and cleanup of existing owners remain available. Keep frontend Session
capacity separate and physically charged until actual Session join.

Make retained completed resources visible in `local-zellij list/status`: state,
receipt digest, exact workload exit, consumption state, original publisher
identity, and whether currently live-authenticated. Add a local-owner-only cleanup
command that selects UUID plus generation and freshly verifies this terminal
receipt, stores a durable local consumption record through the existing profile
owner lock/atomic writer, then asks that supervisor to retire itself. No arbitrary
PID signals or silent oldest-owner eviction. Unknown/live owners cannot use the
completed cleanup route. Cap exhaustion names this reclaim command.

Recommended first-slice policy: `AckConsumed` does not implicitly mean global
Retire. Explicit cleanup provides a bounded practical reclaim path while retaining
live reauthentication for detached/restarted consumers. A later automatic
`RetireWhenQuiescent` policy may be added only as an explicit product decision:
it trades future live reauthentication for already saved completed history.
It is not implied by a successful frontend close or by app shutdown.

## Native and CLI delivery; exact status propagation

A frontend CLI subscribes at OwnerBound using the current immutable observer
context. Persistent receipt identity stays independent of that context. Native
requires a fresh independent peer query of the supervisor's receipt, then matches
it to the retained CLI's typed `WorkloadFinished` event and its current operation,
namespace, surface, session and launch generation. Stale current-context events
cannot mutate a reused native target.

On CompletedRetained, the CLI gracefully detaches only its own retained Zellij
frontend, joins its broker, restores termios and emits a final receipt containing
`owner_outcome = LivePreserved | WorkloadCompleted(TerminalReceiptV1)`.
`original_owner_preserved=false` is not reused as a generic success switch.
Frontend client cleanup status remains separate from workload result. The native
surface immediately enters an explicit `workload_completed` state with structured
exit status and retained workload viewport; it is not a blank apparently running
terminal. Reject new input after authoritative completion. An already admitted
partial/opaque write keeps the existing precise loss/unknown accounting and is
never replayed. Completion is a separate input fence from final PTY cleanup.

Native saves an explicitly typed verified-completion fact on the canonical
surface through the existing S01 FIFO and accepts only its exact actual Saved
completion. That fact is persistable before the CLI's outer Session has exited;
it claims workload completion, not outer physical cleanup. This avoids a cycle in
which CLI waits for durable ACK while native waits for CLI exit before saving.
The private `CompletionSaved` authorization carries receipt digest, persistent
owner generation, current observer context and exact S01 I/O/revision seal. A
post-connected completion has one separate absolute acknowledgment deadline,
created once when its live proof is authenticated and carried through the private
event/seal. This does not renew the original startup deadline; neither queue wait
nor partial ACK transfer renews the completion deadline.

The observer v2 `completion_committed` message is accepted only after the matching
WorkloadFinished event, with strict sequence/nonce/correlation. The CLI forwards
AckConsumed over the authenticated supervisor channel. An equal digest and equal
consumer id/revision replay returns AlreadyConsumed; a different digest, stale
owner generation or conflicting sequence fails. Persist just one consumed receipt
digest per owner, not an unbounded consumer/revision history. Per-connection
challenge/sequence state lives only in the four bounded ephemeral subscription
slots. A freshly authenticated new consumer can consume that same digest under
its own new Saved seal; AlreadyConsumed never transfers an old context's
mutation authority. This ACK never launches anything
or consumes another owner's state. A fresh post-restart observer epoch can consume
the same receipt only after reauthentication and its own new actual Saved result;
old ACK bytes provide no authority to that new launch.

Only after the private native ACK or a separately durable local headless
consumption does the CLI return the workload's status. Preserve the structured
Exited(code) versus Signaled(signal) distinction. The public CLI convention is
exit(code) or 128+signal respectively. A supervisor/transport/durability/protocol
failure uses the existing explicit failure path and cannot become workload zero.
A legitimate nonzero workload completion is a known exit result, not failed
physical cleanup. Native must expose workload exit separately from
frontend_exit_code and cleanup status; it must stop requiring CLI0 as a condition
for all successful physical finalization of the new Completed branch.

S01 needs a versioned typed completed descriptor/history fact: it restores a
completed result without emitting Attach or Start. It is labelled previously
verified/durably consumed history after restart, not a new observation of dead
processes. A descriptor lacking this fact must reauthenticate a live publisher
or remain Unknown; a registry terminal file is not enough. No fake ordinary PTY
is allocated for restored completed history.

## Crash windows and multi-consumer behavior

| Window | Required outcome |
| --- | --- |
| Child exits before full binding commit | Startup uncertainty retained; no fabricated image binding or Completed proof. Fast-exit acceptance below is still required. |
| Supervisor crashes before successful terminal sync | Unknown; file fragments/ESRCH/CLI0 never establish completion. |
| Terminal sync succeeds, Desktop detached or crashes before receiving it | CompletedRetained supervisor stays live and reauthenticates the receipt on next Attach/status. |
| Native receives proof, S01 save refused or fails | No AckConsumed. Supervisor stays retained; native reports the real persistence issue. No Start or implicit retirement. |
| S01 Saved succeeds, native crashes before ACK | New Desktop restores the saved completed fact. If it needs to send ACK, it first queries the same live supervisor under a fresh current context. |
| ACK succeeds but response is lost | Same receipt/consumer/revision ACK is idempotent. It does not repeat effects or retire unless explicit retirement was requested. |
| Supervisor dies after prior verified S01 save | That consumer retains completed history; new consumers without that history still cannot infer authenticated completion from the dead publisher. |
| One of several clients ACKs | Other live subscribers retain delivery/cleanup rights. AckConsumed is not global cleanup. |
| Explicit cleanup races another subscriber | Refuse Busy while an existing subscribed frontend has not completed cleanup; no forced cut-off. Disconnected clients may later see explicit retired/unknown state, never a fabricated successful live proof. |

Supervisor control service failure must not kill/replay a still running workload.
After the workload is reaped, failure retains the bounded owner resource and
reports Unknown, rather than discarding the only authentication source. Root app
shutdown still ends only frontend Sessions and observes physical joins; it does
not automatically retire durable running or completed owners.

## Required behavioral acceptance before activation

1. Real finite workload: commit identity, actual input, exit0 and nonzero exit7;
   completion proven from the same supervisor, exact raw/mapped status, real
   frontend/broker/monitor/namespace/outer Session cleanup, no false preservation.
2. Actual signal termination stays distinct from ordinary exit; missing/crashed
   supervisor, copied receipts, PID reuse, image drift and replaced endpoint/root
   cannot create Completed or authorize cleanup.
3. Exit while Desktop detached, fresh Desktop restore/query and exactly once
   durable consumption. Crash at terminal sync, before/after S01 Saved, before/after
   ACK, response loss, duplicate ACK and two simultaneous clients.
4. Actual no-follow/directory-sync failure leaves source retained and denies ACK;
   conflicting receipt content never overwrites the first terminal generation.
5. Exhaust 64 owner slots without spawning a 65th workload; status/query/explicit
   completed cleanup remain available; cleanup reclaims only the selected live
   authenticated completed owner and its retained capacity.
6. Fast commands that exit before the current selected-shell-image sampling:
   the existing startup protocol has a race here (`create` samples process image
   after Child spawn). This proposal does not silently waive it. Require a real
   failing/passing control and a reviewed pre-user-code exec/binding barrier or
   equivalent OS-backed spawn identity evidence before claiming general finite
   command acceptance. No sleeps, fabricated shell ACKs or PID/name relaxation.
7. Packaged Darwin/AppKit, minimum OS and native usability/input readiness remain
   independent activation gates. Linux protocol success does not establish them.

Signing alternative: only needed if post-mortem source authentication is required
without a retained publisher. A signature whose verification key is reread from
the same writable registry does not solve that trust problem. It would need a
previously authenticated and independently trusted durable key/generation pin
across restart, with explicit key rotation/recovery semantics. No crypto dependency
is justified for this proposed live-owner protocol.

## Shared Rust seam agreement

The protocol owner supplies public, pure, bounded types under
`remote_runtime::local_zellij::completion`: `CommittedBinding { version,
owner_uuid, owner_generation, namespace, completion_socket, server, supervisor,
workload }`; `WorkloadExit::Exited(i32) | Signaled(i32)`; and
`TerminalReceiptV1 { version, domain, binding, binding_digest, terminal_sequence,
workload_exit, child_reaped }`, with `validate()` and `digest()`. These functions
validate data; the worker's separate live-peer check supplies authentication.

Private worker events carry `WorkloadFinished { correlation, receipt, deadline }`.
Private channel `Authorization` distinguishes `Reservation(CheckpointSaved)` from
`Completion(CompletionSaved { correlation, owner_generation, receipt_digest,
document_revision, io_id })`. None is a public CLI/generic JSON command.
`ClientReady.frontend` becomes optional for an already completed Attach: a real
retained CLI Session can deliver a verified terminal fact without ever claiming a
Zellij frontend or transport connection. The current startup owner and later
active consumer must both retain the same dedicated authorization sender until
actual outer Session cleanup.


## First completion slice: workspace and S01 evidence

The workspace schema is now version 7, with a typed completed-history variant
that preserves the immutable terminal receipt and an optional UTF-8 viewport
bounded to 16 KiB. Its restore consumes zero terminal IDs and emits no Attach or
Start effect. Version 6 live owners retain their attach-only reader. In-memory
history and large protocol variants use boxed payloads; the serialized shape is
unchanged.

The coordinator retains its private authorization sender after Ready. A validated
native WorkloadFinished event immediately fences new input, before save admission
or completion Saved. Direct input, paste, key and application-wheel paths,
targeted text and file-path pastes, delayed transfer insertion and native replies
share the same writable-session owner. Existing admitted partial reply writes
retain their existing truthful completion/partial/unknown accounting and cannot
be replayed.

The existing S01 FIFO worker owns the completed-history Save. Queue refusal
installs no history. An accepted failed write keeps the in-memory history and
latches authoritative protection but sends no completion authorization. Only its
actual exact Saved result, with unchanged correlation, target, slot generation,
receipt digest, owner generation and fixed completion deadline, can authorize the
private native completion ACK. Shutdown may drain the actual write but cannot
authorize that ACK.

The initial completion Save certifies the terminal fact and the last visible
bounded workload viewport captured when the authenticated completion event is
observed. That capture survives checkpoint deferral and frontend cleanup; Zellij
detach/restoration output never overwrites it. The Save does not wait for the
CLI/outer Session join: waiting would deadlock the private Saved/ACK protocol.
Actual outer Session cleanup clears only the logical session ID and refreshes
the normal autosave/shutdown baseline. The captured history remains immutable
through and after that transition. This is bounded saved screen output, not a
byte-complete PTY log, a final-output drain guarantee or a claim that every final
output byte is crash-safe at the initial ACK.

Workspace/runtime focused validation passed 37 tests: core descriptor/restore 18,
private S01/input 11, native notifications 7 and native transfer receipts 1.
Strict desktop-runtime library/tests Clippy passed. The private controls use
synthetic terminal facts and real FIFO storage; the input dispatcher control uses
an actual live PTY with positive pre-fence delivery and a post-denial FIFO barrier.
They do not claim live supervisor authentication. The exact commands/log hashes
are in `/tmp/zmux-r03-completion-workspace-validation-20261016-v2/receipt.json`
(SHA-256 `af86d418e1030bedad22070a760669a48cd9e95a7a43f3ce8a449821cdddf752`).
The input fence's same-command intentional red and repaired green evidence is in
`/tmp/zmux-r03-completion-input-fence-20261016/red-receipt.json`.

The real supervisor/CLI/native-worker lifecycle regression is separately owned
and reviewed with the protocol implementation. Fast exit before full binding and
packaged Darwin/AppKit terminal usability/readiness remain explicit required
activation gates. These focused results do not waive any of the wider crash,
concurrency, platform or capacity acceptance cases above.
