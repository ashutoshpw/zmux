# Terminal process exit closes its tab

## Contract

When a local terminal shell exits, close the tab that owns its session. Do the same
for a managed SSH login shell after it reports a normal remote exit. A surface launched
with an explicit remote command (`--command` or trailing remote arguments) remains open
for output inspection and the existing reconnect action. Route automatic closes through
the existing `close_surface` action so Recently Closed and session cleanup stay consistent.
Do not close a sibling tab or a different session if the surface has since been reconnected.

Keep a managed SSH surface open when OpenSSH reports exit code 255. OpenSSH also
uses 255 for connection and transport failures, where the existing reconnect button
is useful. This leaves the uncommon remote `exit 255` case ambiguous.

## Implementation

`ApplicationCoordinator.reconcile` already receives revisioned model snapshots and
terminal frames for both local and managed SSH sessions. It finds terminal surfaces
whose exact session frame is `exited`, submits one tracked `close_surface` action, and
fences duplicate submissions by identity namespace, surface and session. If the core
refuses the close, the request is retried after a short delay. The Swift SSH endpoint
model decodes the existing `command` field in the serialized SSH config; one-shot remote
commands and SSH exit status 255 remain available to the existing reconnect affordance.

## Verification state

`RUST_TEST_THREADS=2 scripts/check` passed on commit `45c13a3`, before the packaged
smoke exposed that a one-shot SSH command exiting successfully was auto-closed before
reconnect could be checked. The follow-up predicate now distinguishes explicit SSH
commands from login shells. The hosted packaged reconnect smoke must pass on the updated
source before this parity item is marked complete; the smoke covers the explicit-command
tab/reconnect path, while interactive local/SSH exit and Recently Closed reopening still
need direct native acceptance.
