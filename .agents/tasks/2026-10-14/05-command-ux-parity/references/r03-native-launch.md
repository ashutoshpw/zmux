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
