# Terminal process exit closes its tab

## Contract

When a local terminal shell exits, close the tab that owns its session. Do the same
for a managed SSH login shell after it reports a normal remote exit. Route the close
through the existing `close_surface` action so Recently Closed and session cleanup
stay consistent. Do not close a sibling tab or a different session if the surface
has since been reconnected.

Keep a managed SSH surface open when OpenSSH reports exit code 255. OpenSSH also
uses 255 for connection and transport failures, where the existing reconnect button
is useful. This leaves the uncommon remote `exit 255` case ambiguous.

## Implementation

`ApplicationCoordinator.reconcile` already receives revisioned model snapshots and
terminal frames for both local and managed SSH sessions. It now finds terminal
surfaces whose exact session frame is `exited`, submits one tracked `close_surface`
action, and fences duplicate submissions by identity namespace, surface and session.
If the core refuses the close, the request is retried after a short delay. Exited SSH
status 255 remains available to the existing reconnect affordance.

## Verification state

`RUST_TEST_THREADS=2 scripts/check` passed on this source tree. No focused regression
case was added for the AppKit auto-close behavior. Native macOS compilation, packaged
local/SSH exit behavior, Recently Closed reopen and SSH transport-reconnect behavior
remain to be verified before marking this parity item complete.
