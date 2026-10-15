# Public zellij peer/process feasibility

This standalone experiment compiles original C against public host APIs, downloads
a checksum-pinned official zellij 0.43.0 no-web executable into private scratch, and
exercises actual Unix peers and terminal input. It changes no application/runtime
dependency, Cargo lockfile, native workflow, user configuration or running app.
Read [provenance](NOTICE.md) and the [proposed owner design](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/r03-local-zellij-owner-design.md).

```sh
python3 -B tools/local-zellij-feasibility/run.py --output /tmp/zellij-peer-results
```

Use a fresh output directory. Requirements: Python 3.12+, a C compiler, Unix sockets
and network access to the exact pinned release. Darwin additionally needs Xcode
command-line tools and the public macOS SDK. The dedicated `Local zellij public peer
feasibility` workflow is manual-only, executes on macos-15, and uploads host receipts,
bounded terminal/error logs and the retained upstream license. It never modifies or
dispatches the existing native application workflow. Linux x86_64 is available for
local harness checks; a Linux pass establishes no Darwin behavior. Execution covers
only the runner's architecture, with no universal/runtime parity claim.

`public_identity.c` uses `getpeereid`, public `SOL_LOCAL/LOCAL_PEERPID`,
`proc_pidinfo(PROC_PIDTBSDINFO)`, `proc_pidpath` and `kern.boottime` on Darwin. It
includes SDK-declared structures directly. The small C/Python interchange is our own
fixture ABI; it is not a copied kernel layout. Linux uses `SO_PEERCRED` and public
`/proc` files. Missing peer-PID support fails the experiment before starting zellij.

The authoring gate is the observable platform/transport contract. An unsupported
Darwin peer option, incorrect SDK process fields, dropped credential guard or changed
zellij IPC behavior must make the real fixture fail. Existing portable tmux tests and
AppKit smoke never exercise these APIs or this proxy boundary. The fixture has no
production export, mock zellij protocol/server or invented success acknowledgement.

The actual boundary checks are:

- A child connects to a private listener. Public socket credentials must equal its
  independently owned Popen PID and UID; process executable and birth-time fields
  must match the actual spawned process. A second genuine same-UID process must be
  rejected against the first retained child identity. Both children independently
  verify ACK/rejection on their real socket.
- Real descendants retaining a pipe or closing stdio must stop when the bounded
  command owner ends. The retained-pipe control deliberately reaches its deadline;
  its typed expected-timeout result and actual descendant termination are required.
- The pinned real executable starts a detached owner with an explicit private config,
  layout, socket base and cache. A foreground wrapper reports its actual parent PID,
  which must equal the daemon PID returned by credentials on the connected backend
  descriptor. Process inspection must identify the exact downloaded executable.
- Two actual terminal clients attach through separate sole-name private brokers. Each
  connection authenticates the owned, unreaped child PID/UID and the backend process
  identity on broker-owned descriptors;
  ordered bytes remain opaque. Real terminal input independently writes the original
  shell PID. Same-UID unrelated input is rejected before a backend connection opens.
- Client TERM preserves the original server/shell identities and startup count one.
  Relay workers join before child reaping. Explicit owner close uses a third retained
  PTY client, confirms real terminal readiness and the original shell PID, then sends
  the fixture's explicit Ctrl+q Quit binding over its already authenticated transport.
  The original daemon, foreground shell and wrapper must end; client exit 0 alone
  is insufficient. Retained full shell/wrapper identities must disappear within a
  bounded wait before cleanup can succeed. No global PID-only kill fallback exists
  on Darwin.

Peer credentials and live process inspection are separate APIs. On Darwin,
`LOCAL_PEERPID` becomes unavailable after the peer disconnects, even while its child
PID remains unreaped. The short-lived real `kill-session` helper can exit before
broker authorization and is therefore unsupported by this Darwin transport design.
Missing credentials always fail; the fixture never substitutes the expected PID or
trusts a same-UID path. Retained interactive clients keep their connection alive
through authentication. Cleanup uses the explicit Quit binding of the only active
client, which terminates the pinned server; this establishes that long-lived shutdown
path only. A production short-lived administrative command requires a separately
supported owner transport. The non-child durable backend always requires its
reserved full process identity.

The C PTY launcher acquires a controlling terminal before exec. Python uses no
pre-exec callback or fork from the broker's threads. The headless owner's future
foreground job control, termios restoration, resize contract, stalled-peer workload,
buffer saturation, malicious ancillary messages, resurrection and prefix-race policy
are not established by this fixture. Those remain separate design/conformance work.
No semantic GUI `attached` receipt, AppKit integration or production readiness follows
from socket peer binding.

Each broker permits four simultaneous relay workers, 64 KiB per direction per
connection, 32 retained connection records and 16 rejection records. Socket I/O has
backpressure. Operations have explicit 2–30 second deadlines; downloads cap at 64 MiB
and 90 seconds, extraction at 160 MiB. Terminal/command/error logs retain at most
64 KiB each. These are fixture bounds, not measured app memory/performance budgets.
The runner uses nonreaping `waitid(...WNOWAIT)` before terminating owned process groups.
It never signals an adopted/global daemon PID. The foreground fixture owns an
unreaped interactive shell child and has a 75-second watchdog; its pane requests
close-on-exit as an additional containment measure.

Backend input rejection is fatal by default and retains its errno, byte count,
payload hash, forwarding/pending counters and phase observed before that send.
An explicit terminal operation may report an outcome with undeliverable input only
after independent proof: owner Quit requires the original server identity gone and
an unreaped clean child exit; attachment TERM cancellation requires an unreaped clean
child exit, the same original server and shell start identities, and startup count
one before and after joined transport cleanup. Only rejections observed after that
specific request can be classified. Opaque rejected bytes remain explicitly lost;
these outcomes make no input-delivery claim. Real socket controls keep rejection
fatal when a requested shutdown leaves the backend live or cancellation lacks a clean
child/shell-preservation proof.

Normal resets/broken pipes are recorded explicitly. Other I/O/credential/cleanup
failures fail the run and retain a private scratch path for diagnosis. Successful
cleanup requires the original daemon, shell and wrapper identities to have ended,
then removes only task-owned namespaces. Still-queryable original identities fail
closed; bounded public ps diagnostics distinguish a visible zombie from a running
process without calling either absent. Unavailable proof or cleanup is reported as failure, rather than softened
to a pass. The downloaded executable and compiled fixtures are excluded from uploaded
artifacts; only metadata, logs and notices are uploaded.

`receipt.json` records platform/architecture, SDK/compiler, repository SHA, actual
harness hashes, download hashes/version, real peers, buffered-byte/connection peaks,
rejections and physical cleanup. It labels its scope `host_public_peer_process_api_and_real_opaque_transport_only`
and semantic GUI attachment `not_tested`. A successful host run is evidence for that
host/API/binary combination only.

The raw transport regression uses a real child that closes its read half before
sending 256 KiB through the broker. The independent sink verifies exact ordered
bytes; receipts separate read, forwarded, discarded and pending bytes. A reverse
write failure no longer discards upstream input, and drain time remains bounded.
The PTY wrapper reserves distinct exit statuses for arguments (64), terminal
acquisition (71), exec ENOENT (127) and other exec failure (126); the missing-exec
case verifies unreaped and reaped status rather than requiring transient PTY stderr
to survive a child exit on every host.
