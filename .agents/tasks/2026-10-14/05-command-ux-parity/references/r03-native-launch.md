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
identity before reservation; EOF, unexpected input and final-publication failure
while the original workload remains alive; and descriptor-key absence in the
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
Independent source review and actual Darwin observer execution remain pending.
The accepted packaged headless execution at `9ec70f7` predates this candidate and
cannot establish its new observer behavior.
