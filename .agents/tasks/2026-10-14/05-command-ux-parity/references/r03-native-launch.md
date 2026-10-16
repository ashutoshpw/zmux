# Native Zellij launch consumer

2026-10-15. Design only. The foreground CLI owner and graceful Session policy are
prerequisites; no production native launch selects Zellij or Graceful yet. The
shared shutdown implementation is tracked in [S01](s01-shared-shutdown.md).

## Owner and launch boundary

Keep ordinary new terminal/tab commands on the configured shell. Add an explicit
local-Zellij operation to the existing Rust command/action executor. Resolve
workspace, caller, remembered binding, current workspace, then create a workspace;
reuse an exact live client unless a new client was requested. Route resolution
must retain identity namespace, surface/session ID and launch generation fences.
Native menu, palette and CLI requests use this same action owner.

Persist an attach-only descriptor containing a validated installation profile ID
and reserved owner UUID. It is mutually exclusive with SSH and local-tmux identity.
The profile resolves private registry configuration on a background worker; the
surface never stores an arbitrary socket, startup command or environment. Undo,
restore and reopen attach that owner UUID. They never reconstruct Start or infer
an owner by display name. Uncertain/missing identities stay explicit failures.

Add the bundled CLI executable as explicit installation configuration passed from
Swift to DesktopOptions/FFI creation. current_exe in the native host is the app,
not the CLI. The launch worker runs that executable directly in the existing
Session PTY with typed argv and ShutdownPolicy::Graceful. Reuse the production
foreground LocalZellij bootstrap/broker; do not duplicate it in desktop-runtime or
parse stderr/terminal output into an owner receipt. Launch configuration is
resolved off the core executor. The Session/PTY/CLI child remains capacity-charged
until its worker completes and is joined, including failed launch cleanup.

## Authenticated lifecycle channel

Create one short private Unix observer namespace per launch, using existing checked
canonical /tmp selection, private modes, inode fencing and kernel peer identity.
One listener, one accepted exact CLI process, at most eight versioned messages of
32KiB each and one cancellation-aware startup deadline are sufficient for this
slice. The channel carries typed lifecycle facts, never shell or terminal input.
No network endpoint, inherited general-purpose application credential or durable
bearer token is introduced. The one-use nonce stays in a private transient launch
descriptor; remove observer markers before the CLI creates its durable workload.
Both peers independently verify the other's actual connected identity against the
native launcher and Session PID/full identity, rather than trusting a supplied PID.

The parent correlates every message with installation epoch, operation ID, session
ID, launch generation and strictly increasing sequence number. A stale surface
cannot receive a completion or regain capacity. Peer/nonce/generation mismatch,
oversized/duplicate/out-of-order frames, EOF and deadline are explicit failure.
There is no reconnect or automatic Start retry.

The proposed state sequence is:

1. `OwnerReserved(owner_uuid)` follows the CLI's durable reservation, before
   workload authorization. The native persistence owner must save the attach-only
   descriptor and return its exact acknowledged revision before permission to
   proceed. A lost acknowledgment cannot authorize replay or a second owner.
2. The existing authenticated supervisor/server lineage and spawn_once handshake
   run unchanged. `OwnerBound` carries independently validated identity evidence;
   a failed/cancelled native target preserves any potentially created owner.
3. `TransportConnected` follows actual broker admission of the retained frontend
   and actual backend peer. It permits a truthful transport-connected presentation.
   Opaque forwarded bytes do not establish semantic Zellij readiness or that a
   shell consumed input. Product code does not inject shell probes for readiness.
4. `FrontendFinished` carries the existing retained-child/worker/transport-loss
   receipt after actual owner checks. It cannot establish completed outer Session
   cleanup until its worker join/is_finished gate also holds. Missing receipt or
   forced escalation leaves supervisor preservation explicitly unknown.

The terminal UI may expose a launch/transport state while actual input/output
continues through the Session worker. No output marker, byte counter, preflight
socket connection or process name can promote this state to semantic attachment.

## Shutdown and terminal-engine coordination

Use S01's one 10-second native ticket and monotonic deadline. Cancel active and
pending launch owners concurrently at shutdown initiation. request_close_until
clamps the selected graceful interval and reserves the existing two-second PTY
post-exit drain within the root budget. The foreground CLI still owns cancellation
of its retained client; neither the app nor Session signals adopted server/workload
identities. Force cleanup is a failure/unknown receipt, never a clean detach.
Native termination remains deferred while physical cleanup is pending, including
after deadline expiry. Resource release requires actual joins, not State::Closed.

A terminal engine remains worker-owned and default vt100. An explicitly selected
optional engine stages one bounded generation; finish any admitted user partial
write before its causal reply bytes. No new PTY reads, engine resize/scroll or user
dequeue occurs while a reply suffix is pending. Only the complete physical reply
write permits generation commit of frame/metadata/notices. Cancellation or reply
failure aborts once and retains the last good publication, without replay/fallback.
Resize stages engine state first, then changes kernel PTY size, drains replies and
commits. Kernel failure aborts visibly; engine/kernel rollback is not claimed.
These contracts are independent of whether the frontend is a shell, SSH or Zellij.

## Delivery order and unresolved acceptance

Land shared shutdown first. Then add descriptor validation and authenticated CLI
observer receipts, followed by one explicit native Start/Attach vertical slice.
Each slice needs real owner-boundary red/green checks and Linux/macOS execution.
Test cancellation before reservation, after reservation, after spawn authorization,
stale launch completions, native termination with blocked output, retained capacity,
startup-once and actual detach/reattach preserving the original workload. Native
save/close/reopen, two-client routing and failure presentation need packaged AppKit
receipts before feature closure.

Full default Zellij layout/configuration parity is unresolved: the current owner
uses its private supervisor pane and selected interactive shell. Later user exec
or closing the original pane can invalidate the pinned workload identity even
while other Zellij tabs survive. Bundled app/CLI updates can also change the
reserved supervisor executable path. Neither problem permits weakening identity
to same PID/name/UID; define a reviewed compatibility policy before claiming
cross-update or arbitrary-layout restoration. Managed caller attribution, broader
close/prune and detached creation remain separate supported-owner work.

## Next dependency-ready slice

2026-10-16. Retained-package native execution is being diagnosed separately in
`r03-native-package.md`; a successful package gate will not enable GUI Start/Attach.
The next source slice owns the authenticated lifecycle channel in the existing
foreground CLI path. It adds no native launch action, placeholder UI state or new
workspace/persistence writer. Ordinary headless invocations keep their current
behavior when no authenticated observer is configured.

Use a typed launch envelope with schema version, installation epoch/profile ID,
operation ID, identity namespace, session/surface UUIDs, launch generation, native
launcher full process identity, and one-use nonce. These values are transient,
bounded and bound to the actual connected peers. A supplied PID never substitutes
for kernel socket credentials. The CLI is the Session's direct retained child;
the host checks that exact PID/full identity and the bundled CLI executable, while
the CLI checks the actual listener peer against its native launcher identity.
Fail closed on either mismatch. The descriptor and observer environment must not
reach Zellij, its supervisor or the durable workload.

The owner sequence extends existing production transitions rather than recreating
them in the consumer:

1. `files.rs::start` persists `reserved`, then sends `OwnerReserved` containing
   the owner UUID and full correlation envelope. Wait, with foreground cancellation
   and the original startup deadline, for `ReservationCommitted` identifying that
   exact owner and acknowledged document revision. No supervisor creation or
   `spawn_once` may occur before this acknowledgment. Missing/invalid acknowledgment
   preserves the reservation, reports uncertainty and forbids automatic replay.
2. The existing supervisor handshake and durable bound record remain the source
   of `OwnerBound`. Attach validates the selected existing UUID and emits its
   validated bound identity; attach never creates a missing owner.
3. The broker publishes `TransportConnected` only after independently admitting
   the actual retained frontend and backend peers. The channel carries that typed
   fact, never a terminal marker or claim that a shell consumed input.
4. `FrontendFinished` follows the existing relay join, retained-child reap,
   termios restoration and owner-preservation checks. It contains the actual exit,
   requested cancellation, bounded opaque-input accounting and preservation
   outcome. Native Session cleanup still requires its separate physical join.

The future v6 document descriptor contains only validated `profile_id` and
`owner_uuid`. Resolve executable paths and private registry root from installation
configuration on a worker; do not save them as an arbitrary launch program or
socket. Descriptor validation must enforce exclusivity with SSH/local tmux.
Restore, undo and reopen always issue Attach by exact UUID, including after an
uncertain Start; they cannot reconstruct or retry Start.

Reservation persistence is a real dependency, not a call to existing ordinary
Save. `sessions.rs::stable_document` currently rejects `pending_launches`, and
autosave acknowledges no reservation-specific barrier. The existing persistence
owner must add a narrowly typed transactional checkpoint that saves the validated
attach-only candidate through its current queue and returns the exact successful
revision. Only then may the action owner send `ReservationCommitted`. Save failure,
stale namespace/generation, surface removal or shutdown cancels authorization;
the CLI reservation remains retained. No acknowledgment follows a queued-only
save, and no second writer or sidecar is introduced.

Ownership and delivery order:

- Foreground owner slice: new `local_zellij/observer.rs`, minimal integrations in
  the existing remote-runtime owner and CLI dispatch, and real private-socket/CLI
  owner tests. Verify nonce/peer/envelope/sequence bounds, fragmented frames,
  cancellation/EOF/deadline before acknowledgment, and no workload before a real
  exact-revision acknowledgment. Preserve existing headless owner tests.
- Workspace/persistence slice, coordinated after the current v5 todo freeze:
  v6 typed descriptor/restore and the existing S01 persistence checkpoint owner.
  Cover save failure, stale generations, restore-as-Attach and no Start replay.
- Consumer slice: new desktop-runtime local-Zellij operation owner with minimal
  existing executor/launch hooks, explicit DesktopOptions/FFI bundled-CLI profile,
  direct Session PTY launch with Graceful shutdown and retained capacity. The
  operation snapshot reports launch/transport facts, not semantic attachment.
- Native presentation slice: explicit Start/Attach entries sharing that action,
  bounded failure presentation and a packaged AppKit owner smoke. New presenter
  and smoke files should carry most changes; coordinate shared App/Models hooks
  with browser/metadata owners. Test real input/resize, startup-once, two clients,
  save/close/reopen, blocked-output quit and preservation on uncertainty.

The foreground slice can proceed without editing active metadata/browser files.
Do not admit native Start until the checkpoint contract exists. An Attach-first
vertical consumer is a possible smaller user-visible milestone once the typed
descriptor and authenticated observer are ready; it must still preserve truthful
transport state and exact-owner restoration.

## Foreground observer implementation candidate

The next foreground slice is implemented behind the explicit private
`ZMUX_LOCAL_ZELLIJ_OBSERVER` launch descriptor. It reuses the same public headless
Start/Attach owner; there is no native action or second bootstrap/broker. The
descriptor's versioned correlation fields are typed nonnil UUIDs plus nonzero
operation/generation IDs. Its private regular file and socket share one canonical,
mode0700 runtime directory directly under the checked short `/tmp` base. Socket
owner/mode/type and directory/socket inodes are verified around connection. Actual
kernel peer identity must match the declared full launcher identity, which must
also be the foreground CLI's actual parent. Version and downstream processes do
not inherit the descriptor environment key.

Frames have a four-byte little-endian length and at most 32KiB of strict JSON.
The immutable envelope includes version, context, nonce and monotonically
increasing sequence; at most eight messages are admitted. A Start publishes
`owner_reserved` only after the durable registry reservation, then accepts exactly
one matching `reservation_committed` response with a nonzero declared document
revision. Observer connect, observed registry wait, acknowledgment and existing
supervisor creation share one startup deadline and foreground cancellation poll;
fragmented input cannot renew that budget. Ordinary unobserved startup retains
its previous supervisor timing. The existing version preflight remains its
separate bounded acquisition step.

`owner_bound` is emitted from validated owner evidence. `transport_connected`
includes the exact retained frontend identity and requires an actual broker
authentication. Once running, any observer EOF or unsolicited input fails the
frontend through its existing retained-child cleanup. Lifecycle/final writes are
nonblocking and cannot add a shutdown timeout; partial/backpressured publication
fails without retry or replay. `frontend_finished` is emitted only after broker
join, client reap and terminal restoration. It carries owner facts and transport
accounting; it does not override a later nonzero CLI result or establish the outer
Session's physical join. Missing/truncated final publication is an unknown outcome.

The nonce is correlated protocol admission, not a consumed durable token. The
foreground implementation authenticates its actual parent and validates the
response, but cannot prove the consumer saved its declared revision or accepted
only one retained CLI. The future native owner must accept exactly its retained
Session child, enforce one use, save through the existing persistence transaction,
and clean its inode-fenced observer namespace after physical join. Keeping or
reusing a descriptor path does not establish those consumer guarantees. No native
Start is exposed before that checkpoint exists.

Focused Linux evidence is under
`target/agent-validation/r03-native-observer-20261016/`. The actual CLI regression
failed on baseline because a workload started before any observer acknowledgment,
then passed through the integrated production path. Current focused checks passed
three existing CLI grammar/authentication tests, all seven foreground owner tests
and five local-Zellij runtime socket tests. Coverage includes actual observed
Start/Attach input, startup-once and original live identities; fragmented protocol
acknowledgment; nine refusal/cancellation/deadline controls; changed full launcher
identity before reservation; actual EOF and unexpected input while the original
workload remains alive; and descriptor-key absence in the
actual version probe, backend, frontend, supervisor and workload environments.
Only the boolean key-presence result is inspected; environment bytes are neither
logged nor retained in evidence. A real Unix-socket backpressure control separately
forces a partial lifecycle frame and proves immediate failure without replay.

Test-authoring gate: these checks protect the new external framing, process-peer,
reservation-authorization and environment-isolation contracts at the real CLI and
kernel socket boundaries. Prior headless tests had no observer and could not catch
premature spawn, stale acknowledgments or lost lifecycle publication. The lower
socket control covers deterministic partial writes that a normal small CLI receipt
cannot force without adding a production seam. Tests use no new production export
or injected timing flag. Their revision acknowledgment is explicitly synthetic
protocol admission, not durable checkpoint evidence. A rejected-handshake fixture
initially raced /proc lookup after the CLI had exited. The preauthorization
negative control now verifies the production refusal without authenticating an
already-disconnected peer; Darwin may no longer expose LOCAL_PEERPID then.
Positive admission checks the actual kernel PID and full live identity before
authorization. Product identity checks were not weakened.

The isolated V2 snapshot at committed base
`498f01bb57fcaa917db0edd8ed48a66ab2a630d0` plus only these eight owned files
passed required `scripts/check` in 79.313s (311 Rust tests, strict Clippy, notices
and C ABI), the ten feature-enabled actual CLI tests in 13.185s, and strict
feature-test Clippy. Its complete source inventory and live owned inputs remained
unchanged. Receipt: `target/agent-validation/r03-native-observer-20261016/frozen-v2/receipt.json`,
SHA256 `8d075e5b9a399c867146261c4951904ad71e28975e0f7eef442b90b9bc96c749`.
The first snapshot exposed a test-module placement lint; moving the module after
the shared framing function fixed it without an allow/skip. The final V3 change
records this validation note only; Rust verification is retained from V2.
The accepted packaged headless execution at `9ec70f7` predates this candidate and
cannot establish its new observer behavior.

### Darwin observer test-oracle correction

Native run `38832049927` at exact source
`6949d8f44cf114647e848c59c9b9081f3b529e3e` failed on macOS 15.7.9 ARM64.
Package admission, native build/signature/version, 20 Session owner tests, five
remote socket owner tests (including deterministic partial lifecycle publication),
three CLI grammar/authentication tests and six of seven foreground tests passed.
The observed Start/Attach test reached its final extra fixture after actual input,
clean TERM/HUP final receipts, downstream descriptor scrub and live original-owner
preservation after unsolicited input and actual observer EOF had passed. The only
failure expected exit 1 after the peer called `shutdown(Read)` and the CLI received
TERM; Darwin instead accepted the final small frame and the CLI exited 0.

Peer `SHUT_RD` is not a portable oracle for a physical writer failure. The narrow
correction removes that unsupported extra fixture, retains actual EOF failure and
original-owner identity checks, and retains the deterministic real Unix-socket
partial-write/no-wait/no-replay test. Production code is unchanged. An accepted
physical write does not acknowledge peer consumption: the consumer still needs a
complete correlated final frame, the CLI outcome and physical Session join before
classifying completion. The positive test now emits a bounded protocol receipt
which explicitly marks revision 42 as synthetic and does not claim a native
durable checkpoint. Version-probe environment scrub remains its separate test.

Failed-run evidence is retained under
`target/agent-validation/r03-native-observer-hosted-38832049927/`; its receipt is
failed and no final app ZIP was produced. This run does not establish complete
native owner acceptance, GUI Start/Attach or a durable native checkpoint.
Independent verification matched all 262 source inputs at that SHA, all nine
phase log hashes/byte counts, artifact `12017438533` (62,691,033 bytes, SHA256
`b5d90a281e4fa3f53cf79c97fd3d3fe10c56a91336bda230fb01982bb1a907b8`),
and all 1,237 retained package files, including its manifest and binary hashes.
No downloaded executable was run during verification. Verification receipt:
`target/agent-validation/r03-native-observer-hosted-38832049927/verified-failure-receipt.json`.

The isolated correction at that committed base plus only the test/doc delta
passed the ten CLI grammar/foreground tests (22.021s including build), the
partial-write owner test (2.078s), scoped rustfmt and strict feature Clippy
(7.710s). Complete snapshot inputs remained unchanged. The prior full
`scripts/check` acceptance is retained from observer frozen V2; this test-only
correction uses focused delta validation. Evidence:
`target/agent-validation/r03-native-observer-correction-20261016/frozen-v1/receipt.json`,
SHA256 `dbebb2d33a00dccc6d791f5da8c67b6c996145f2a84d8c7705433faba2e70414`.
The final frozen V2 adds only this evidence note.

Reviewed diagnostic source `4b96b48b777a46933d604b0481712c287a7b31ff` passed
native run `38833738922` on macOS 15.7.9 ARM64. All 35 owner tests executed
successfully (suite counts 2, 15, 3, 5, 3, 7), including observed Start/Attach actual
input, startup-once, clean TERM/HUP final receipts, actual observer EOF and
unsolicited-input rejection preserving the original live owner, version/downstream
descriptor scrub, the nine acknowledgment/cancellation/deadline controls,
deterministic partial lifecycle write refusal and the queued-input PENDIN probe.
The emitted protocol receipt explicitly declares synthetic revision 42 and
`native_durable_checkpoint_proven: false`.

Independent verification matched all 262 source hashes, all nine complete phase
logs, final dependency signature/version receipts and all 1,237 app-bundled
dependency files against the retained package. The app ZIP is 72,048,293 bytes,
SHA256 `7fe2640c4a62fef3ee80c67c3fa1e5dbc6487cbfef32806d7ce1463016afe539`.
Outer artifact `12017848676` is 133,415,007 bytes, SHA256
`4fe21be1a868f2a717dbc4ddf0d6943bab31c7db32c90c813b33a0c0e0cd8dd1`.
The source/ARM64 Mach-O/plist/build metadata and retained producer/binary hashes
also matched. No downloaded executable was run during verification. Receipt:
`target/agent-validation/r03-native-observer-hosted-38833738922/verified-receipt.json`,
SHA256 `aff06f4e8cfd314877a32368157adcda6c33a4f0484eac4a72fc739232c4b998`.

This accepts the Cargo-built CLI observer path using the final app-bundled
retained dependency on that host. It does not accept AppKit Start/Attach, a native
durable checkpoint, Intel/universal or minimum-OS execution, or default/durable
distribution. The failed `6949d8f` run remains a separate failed receipt.

## V6 descriptor and S01 reservation checkpoint proposal

2026-10-16, reviewed against committed `6949d8f`. The checkpoint remains design
only; the first pure-core descriptor candidate is described below. No checkpoint
or native Start implementation is admitted by this note. The foreground
observer is committed, but its synthetic acknowledgment tests do not establish a
native durable checkpoint. The S01 owner and foreground/consumer owner coordinated
this proposal; the latter has no overlapping core/persistence edits.

### Existing owner constraints

- `workspace-core/src/restore.rs:124,451,612,645` owns saved terminal variants,
  capture, validation and preparation. V5 required todo arrays already apply to
  later versions (`:387`); preserve exact UUID/order/state/origin, bounds and
  canonical window membership. Generic snapshots remain display-only.
- `desktop-runtime/src/sessions.rs:246` rejects every pending launch for ordinary
  Save/export/autosave capture. `:573,590` commits a complete prepared restore or
  removes every provisional session on failure. Undo also owns provisional
  surfaces; no checkpoint may make either restore candidate authoritative.
- `sessions.rs:495` retains the latest safe model document in `p.committed`;
  `:502` advances `saved_document` when a write is merely submitted. That cache is
  not durability evidence. `:613-710` drains existing writes and flushes
  `p.committed` at shutdown. A checkpoint that only writes a file could otherwise
  be erased by this older fallback document while its launch is still pending.
- `persistence.rs:124,160,263,307` provides one FIFO file worker with eight credits
  covering queued, executing and unpolled completions. `:662,727` validates bounded
  documents and performs private sibling write, file sync, atomic commit and
  directory sync. A directory-sync error can follow visible replacement; it must
  not be reported as an unchanged file or authorize a workload.
- `local_zellij/files.rs:290-292` reserves durably, waits for the observer's exact
  acknowledgment, then reaches supervisor creation. `observer.rs:223-278` accepts
  only the matching full context/nonce/owner and nonzero declared revision. No
  supervisor creation or Start retry belongs in the native consumer.
- `desktop-runtime/src/lib.rs:2078,2174,2276` owns launch effects, charged workers,
  generation-fenced completion and eventual Session adoption. `Slot::drop` cancels
  its worker (`:844`). `shutdown.rs:235-318` cancels owners and waits for physical
  joins under one root deadline; logical launch failure does not release capacity.

Paths above are under `crates/`. Source inspection establishes existing behavior;
this proposal has no new execution receipt.

### Attach-only schema and recovery

Add one original MIT terminal descriptor containing exactly nonnil `profile_id`
and `owner_uuid`, represented as an exclusive v6 `SavedContent` variant. Reject
that variant in v1-5, missing/null/nil IDs, unknown fields and command/path/socket/
environment additions before reserving or launching. V6 still requires explicit
v5 todo arrays and the v4 canonical window layout; v1-5 readers stay explicit.
The descriptor counts as one terminal under the existing resource limits.

Canonical surface construction and capture must reject mixed SSH, local-tmux and
local-Zellij identity, including browser misuse. Do not add a serialization match
priority that silently drops one identity. One shared pure descriptor validator
serves direct actions, capture and restore. Installed-profile resolution, registry
checks and all executable/process work stay on workers. Missing or changed
profiles/owners produce explicit Attach failure; no shell or display-name fallback.

Restore, undo, reopen and retry of an uncertain launch emit only Attach for the
exact saved UUID. The core descriptor and its recovered launch effect have no
workload Start alternative. A generic `StartSession` effect may create the outer
PTY/CLI client, but its Zellij mode is always Attach. Full restore remains atomic;
failed startup retains the original file/protection latch and existing explicit
S01 recovery behavior. A starter workspace after a reported failed startup is not
a reinterpretation of the failed Zellij descriptor as an ordinary shell.

### Reserve identities before the foreground child

The immutable observer envelope needs native surface/session UUIDs before CLI
spawn. Add an opaque, pure core reservation ticket: consume IDs from the live
allocator only after `ensure_reserved` verifies the durable allocation block,
then hold route, namespace and allocated IDs in the pending operation. The ticket
may reserve the workspace/pane IDs needed by a new-workspace route as well. Its
IDs remain burned after failure/cancellation. Cloning an uncommitted allocator
without advancing the live floor would let unrelated actions reuse them.

There is no canonical ordinary-shell placeholder and no initial `StartSession`
effect. The operation owns its provisional route, immutable correlation, original
deadline, cancellation, actual Session/CLI child, observer namespace and charged
capacity. The core ticket grants no filesystem/process authority. Installation
must revalidate the live target and consume the opaque ticket once; no public raw
ID insertion API is exposed through Action/FFI.

After `OwnerReserved`, installation creates an attach-only surface with those same
IDs and emits no second launch effect. The already-running retained Session is
adopted into its exact reserved slot when the consumer completes; it is not
respawned or reconstructed from an arbitrary PID. Before adoption, close/Slot
cancellation must still reach the worker holding that physical Session/observer.

### Checkpoint admission and durability acknowledgment

Use a narrowly typed `SessionPersistence` checkpoint operation wrapping the
existing `PersistenceWorker::submit_save` queue, not public ordinary Save, another
worker, or a sidecar. Initially admit at most one reservation checkpoint at a
time. Its private ticket seals the complete candidate document, capture revision,
I/O operation ID, full observer correlation, profile/owner and launch generation.

At authenticated `OwnerReserved`, plan against the latest canonical model and
insert only the ticket's attach descriptor. Validate target identity, allocated
ID bounds, browser profiles, canonical todos and the full enriched 512 KiB saved
document using the same capture/serializer as S01. All checks on the executor are
bounded and pure. Unrelated pending shell/SSH/tmux launches, startup/load, full
restore and provisional undo prevent this admission; waiting, if used, consumes
the original launch deadline and cancellation budget rather than renewing it.

Submit the sealed candidate first. A rejected queue submission installs nothing
and sends no acknowledgment; consumed IDs and the CLI's reserved owner are not
reused or deleted. Once the queue accepts, atomically install the validated
attach-only candidate and its pending slot/checkpoint metadata on the serialized
action owner. This is a model commit, not a durable-success receipt. Model edits
can continue afterward; never restore an older whole-model clone over them.

Treat that installed descriptor as recovery-safe intent. Record its document in
`p.committed` at queue acceptance/model installation, before any authorization,
so cancellation/shutdown cannot fall back to a pre-reservation document. Subsequent safe captures may advance this
baseline. An older Saved completion must never replace a newer `p.committed`.

Normal autosave submission pauses while this typed checkpoint is in flight, though
safe capture can keep the shutdown baseline current. Previously queued writes are
ordered before it by the same FIFO writer. An unrelated Saved completion cannot
clear the checkpoint or authorize it. On actual matching `Saved`, record the sealed
document as saved, update its exact receipt/revision, then allow autosave to catch
up from the latest canonical document. `saved_document`, a queued request, any
other successful Save, or a completed PTY spawn cannot satisfy the checkpoint.

Only then may the consumer issue one `ReservationCommitted`, after rechecking the
installation epoch, operation, namespace, current surface/session IDs and launch
generation, exact profile/owner descriptor, original deadline and cancellation/
shutdown state. Correlation and the I/O ticket identify the sealed saved revision;
an unrelated later topology or todo change need not invalidate this owner. Do not
require equality with the latest whole-model revision: todo-only writes preserve
that revision, and unrelated edits are not owner authorization. The acknowledgment
returns the sealed revision, never a guessed/current counter.

### Narrow pending-launch capture and rollback policy

Do not remove the ordinary pending-launch guard globally. An internal recovery
capture may allow pending launches only when **every** pending launch is certified
as the exact active R03 operation/slot with a known attach-only descriptor. Count
comparison alone or the presence of any R03 launch is insufficient. Reject full
restore, provisional undo, unbound reservations, unrelated pending launch kinds
and unmatched/stale worker generations. Public Save/export can retain their
existing stricter stability behavior. Use this same narrow helper for autosave's
committed baseline and shutdown capture, preserving later v5 todo edits while
known R03 clients are still starting.

Before queue acceptance, cancel the provisional operation and discard its candidate
without undo history; other canonical state is unchanged and IDs stay burned.
After acceptance, persistence failure/unknown completion retains the exact failed
attach descriptor, cancels the frontend, disables authoritative overwrite through
S01's existing failure latch, and never acknowledges or automatically retries
Start. Disk replacement may already have happened. There is no compensating write
of an older file or rollback over concurrent workspace/todo edits. Explicit user
close may remove the tile; only a newer successful current-state save supersedes
the older checkpoint through the existing queue.

If close, stale generation or shutdown occurs before a late successful completion,
record its actual persistence outcome but send no authorization. Shutdown cancels
R03 jobs concurrently before draining/checkpoint decisions; even a completion
already queued cannot regain authority. Drain accepted writes, preserve the latest
safe `p.committed`, then perform the existing final flush/join. A successful or
partially written acknowledgment followed by lost transport is uncertain: retain
the attach-only owner identity and never construct another Start. Capacity and the
observer namespace remain held until the actual Session/launch/observer workers
have physically finished and joined under the existing root ticket.

### Proposed verification and delivery boundary

1. Pure core/document regression: exact v6 descriptor roundtrip, mixed identities,
   injected variant in v1-5, malformed IDs/extra executable fields, preserved v5
   todos/windows, live allocator burn despite cancellation/concurrent allocation,
   and restore/undo effects that can only Attach. No new Start action is exposed.
2. Real S01/observer owner regression: a reservation acknowledgment requires the
   matching successful writer result and the exact descriptor on disk; queue-only,
   wrong-operation, failed write, stale target/generation and canceled/shutdown
   completion cannot authorize supervisor creation. Keep the actual foreground
   owner and actual Session child boundary; a synthetic Saved receipt alone is not
   durability proof. Test-only production exports/environment timing flags are
   not needed; any isolated state-machine completion control must be labeled as
   correlation evidence rather than filesystem evidence.
3. Real filesystem/PTY cases: later todo edits and canonical topology are not
   overwritten by checkpoint completion or final flush; restart attaches the same
   owner, never creates another; failed full restore/undo retains source/history;
   cancellation before reservation, while queued, after durable save and after
   authorization preserves the owner and retains charged capacity until joins.
4. Preserve S01 corrupt-file, no-follow, final-save failure and shared shutdown
   suites, plus the frozen foreground observer's actual socket/CLI tests. Native
   Start remains closed until this checkpoint contract is approved and implemented.
   Packaged Darwin save/close/reopen, blocked-output quit and actual owner
   preservation remain separate required consumer/native acceptance.


### Checkpoint acceptance matrix (implementation still pending)

| Boundary | Canonical/disk policy | Authorization and physical ownership |
| --- | --- | --- |
| Cancel before OwnerReserved | No canonical target; IDs remain burned | No acknowledgment or retry; join provisional CLI/observer |
| Queue rejects candidate | No candidate installation or new recovery write | No acknowledgment; reserved registry owner remains explicit |
| Queue accepts; write pending | Install validated attach intent and safe recovery baseline; saved cache is not proof | Retain launch charge; no acknowledgment |
| Accepted write errors or result is unknown | Retain failed exact descriptor; protect authoritative file, which may have changed | Cancel frontend; never authorize/replay Start |
| Saved arrives while current fences match | Record exact sealed successful revision; do not regress newer committed capture | One acknowledgment only while original deadline and cancellation permit |
| Cancel/close precedes or follows Saved, before acknowledgment | Record actual I/O result; retain failed intent or explicit canonical close | No acknowledgment; late success cannot revive operation |
| Acknowledgment fully/partly sent then channel lost | Keep exact attach-only identity; no compensating old-file write | Outcome uncertain; no new Start; retain until physical joins |
| Shutdown with accepted checkpoint | Revoke authorization first, drain existing writer, flush latest safe canonical baseline | Cancel launch/observer concurrently; root deadline does not release unjoined owners |

Known canceled R03 workers must retain their generation tickets until join. A
recovery capture must distinguish an exact known canceled/retiring R03 ticket
(with no authorization and an intentionally omitted closed surface) from an
unknown worker generation. Otherwise a shutdown fallback could resurrect a tile
closed while its checkpoint was queued. Capture/close ordering must also account
for an unrelated ordinary launch admitted before the next poll; this is an explicit
checkpoint implementation test/design boundary, not permission to serialize
unrelated provisional topology. The first pure-core slice does not change S01
capture or shutdown policy.

### First pure-core implementation boundary

The approved first slice adds the v6 attach descriptor, an existing-pane opaque
reservation ticket and a prepared candidate. New-workspace route reservation,
checkpoint dispatch/acknowledgment and native launch remain separate gates. The
candidate is fully validated before queue submission and becomes a store by an
infallible ownership transfer; the caller must submit and install on one serialized
owner turn, with no interleaving mutation. Preparation emits no launch effect and
cannot spawn a second client. Core restore/undo emits only the typed
`AttachLocalZellijSession`; until its authenticated consumer exists, both runtime
action and session-restore admission reject it explicitly before any candidate
publication or process spawn. This prevents an ordinary-shell downgrade while
retaining S01's protected-file recovery starter.

The descriptor regression first fails on baseline's unknown v6 variant and passes
after the original core implementation. Tests cover malformed/old-version payloads,
terminal resource admission, burned/replayed/foreign/stale/capacity-limited tickets,
latest todo preservation, and workspace/last-tab/last-pane failed Attach recovery.
The real runtime refusal fixture verifies the original v6 bytes remain untouched
through shutdown and candidate todos never appear in the fallback workspace.
Runtime-only identity lookup, checkpoint durability and native execution are not
claimed by these pure-core/unsupported-admission checks.
