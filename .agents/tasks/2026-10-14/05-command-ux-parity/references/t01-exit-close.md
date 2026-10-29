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

`RUST_TEST_THREADS=2 scripts/check` passed on `c975f6b`. Hosted workflow
[`40951840905`](https://github.com/ashutoshpw/zmux/actions/runs/40951840905) passed Swift
type-check, both Linux scheduler helper builds, Rust and ABI checks, universal app build,
terminal-selection/browser checks, packaged app/native-rendering smoke, and sidebar/status
smoke. Its packaged SSH reconnect smoke passed after the fix: the explicit `--command`
that exits successfully remains visible with its reconnect control. The downloaded
artifact's build metadata and Mach-O slices were inspected; its checksum is recorded in
the task status.

The packaged SSH fixture exercises an explicit remote command, not typing `exit` into an
interactive local or SSH login shell. The coordinator handles those normal exits through
the same owner-bound close path, but direct local/SSH exit and Recently Closed reopening
still need user dogfood.
