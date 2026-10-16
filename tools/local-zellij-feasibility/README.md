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

The credential fixture records both sides of authorization. An unrelated child's
send may fail with EPIPE/ECONNRESET when the parent has already rejected and closed
its socket; the child still must observe EOF. The parent accepts that outcome only
with its independent kernel-verified exact PID/UID and wrong-owned-PID rejection
record. The owned child still must send successfully and receive ACK. Other send
errors, unexplained closes and contradictory rejection records fail.

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

A separate real PTY-only child defers stdin reads while queued master input reaches
observed backpressure. After TERM it emits256 KiB with terminal-query-like bytes,
then independently checks its queued input for query replies during a bounded quiet
window. Exit0 requires no reply; any ESC response exits3. The owner drains output
without answering queries during its existing bounded TERM/KILL waits, retaining
only64 KiB of logs. Readiness still answers queries. Exit waiting and the post-Quit
drain are read-only so response writes cannot stall them. The regression implements
no Zellij protocol. Exit waiting checks its deadline after each bounded read and
continues immediately when bytes arrive, so short PTY reads do not incur a polling
sleep per chunk. A no-progress pause prevents readable EOF from causing a busy loop.
Requested signals, unreaped waitid PID/code/status, reaped exit
code and bounded PTY output remain visible even on failure. No environment dump is
collected, and failed/nonzero children remain failures.

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

## Experimental stdin interruption repair

The separate manual `Patched zellij production owner` workflow builds pinned MIT
Zellij 0.43.0 source with the original `Interrupted`-only stdin patch, then runs
the unchanged production owner checks on Linux and macos-15. It has its own
concurrency group and does not alter the official download pins or native workflow.
See the [failure and evidence](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/r03-zellij-eintr.md).

```sh
python3 -B tools/local-zellij-feasibility/produce-patched.py --output /absolute/fresh/producer
python3 -B tools/local-zellij-feasibility/run-patched-owner.py --producer-receipt /absolute/fresh/producer/receipt.json --output /absolute/fresh/owner
```

The producer requires Rust 1.95.0 and retains its executable at `artifacts/zellij`.
Real pipe/SIGUSR1 tests require the original client to fail at EINTR, the patched
client to read subsequent bytes once, and other read errors to remain fatal.
Build phases use retained process groups, bounded logs and deadlines; a preexisting
archive must also satisfy the byte/hash limits. Registry dependency caches may be
reused with explicit `--cargo-target` paths, but client/root source products are
cleaned before the baseline to prevent stale patched artifacts from passing as red.
The owner helper otherwise selects its own private Cargo target directory.

Receipts bind source, patch, toolchain, input hashes, configuration hashes, binary,
actual owner results and upstream prebuilt WASM inventory. Those WASM files are not
rebuilt. The workflow uploads immutable source inputs, logs, receipts and notices;
it excludes the compiled executable. Full binary distribution notices, packaged
native acceptance and a product acquisition decision remain
separate gates.

Patched-source run [38811822523](https://github.com/ashutoshpw/zmux/actions/runs/38811822523)
passed all 27 production CLI owner checks on Linux X64 and macOS 15.7.9 ARM64,
including real suspend/resume input and detach/reattach. Source/receipt/log hashes
were independently verified. This establishes that host-specific CLI boundary;
retained distributable binaries, complete notices, native app integration and
default acquisition remain separate. The
[dependency package audit](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/r03-dependency-package.md)
describes the proposed source-built plugin and notice package before binary uploads.

## Opt-in dependency package

The same manual workflow now exposes `dependency_package`, false by default.
When selected, it requires `reviewed_notice_policy_sha256` to match the exact
independently reviewed [notice policy](notice-policy.json). The
[policy review brief](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/r03-package-policy-review.md)
records four published-version provenance uncertainties; canonical standard-term
supplements do not turn them into recovered upstream notices. Policy approval and
fresh hosted Linux/Darwin package execution are separate gates.

Package mode rebuilds the ten release plugins from the pinned source and unchanged
lock, compiles each with pinned Wasmtime, captures actual final compiler units,
and reruns the production owner against the exact resulting executable. The owner
receipt requires every named baseline behavior, six successful suites, semantic
journey fields and unchanged executed source hashes. It records actual counts
rather than using a count as behavior parity proof.

```sh
python3 -B tools/local-zellij-feasibility/produce-patched.py --output /absolute/fresh/producer --rebuild-plugins
python3 -B tools/local-zellij-feasibility/run-patched-owner.py --producer-receipt /absolute/fresh/producer/receipt.json --output /absolute/fresh/owner
python3 -B tools/local-zellij-feasibility/check-package-gates.py --producer-receipt /absolute/fresh/producer/receipt.json --owner-receipt /absolute/fresh/owner/receipt.json --output /absolute/fresh/negative-gates
python3 -B tools/local-zellij-feasibility/package-patched.py --producer-receipt /absolute/fresh/producer/receipt.json --owner-receipt /absolute/fresh/owner/receipt.json --output /absolute/fresh/review --prepare-only
```

Local `--prepare-only` creates an explicitly `UNREVIEWED-` archive with
`promotion_ready=false`. It supplies material for review and never satisfies the
workflow's upload gate. Promotion requires `--reviewed-policy-sha256` with the exact
reviewed hash. Keep outputs in fresh directories and run the chain against stable
source; source or tooling changes require fresh corresponding evidence.

The package independently extracts the executable's embedded plugins and compares
all ten hashes, then retains component notices, checksum-verified registry
sources with explicit prebuilt exclusions (MPL sources remain pristine), filtered
upstream source/patch, exact Rust standard-library
notices, compiler/linker/ABI metadata and file/archive hashes. Actual compiler
components include conservatively marked host build support, with validator dev
closure excluded. Optional plugin UI behavior is untested. ABI records describe
only the executed host/binary; they establish no universal build, backward OS,
application signing or reproducible-build guarantee.

Negative gates use successful real evidence as a positive control before rejecting
missing/mismatched policy review, a replaced baseline behavior with unchanged test
count, incomplete semantic input proof, failed/altered producer or owner evidence,
missing notice supplements and stale embedded plugin hashes. The workflow always
uploads bounded metadata/source evidence and separately uploads executable package
bytes only after the opt-in package and all required gates succeed. Product pins,
ordinary acquisition and native app launch selection remain unchanged.

Source bundles retain no original prebuilt WASM bytes or unused rustix fallback
static archives. The upstream source archive excludes exactly fifteen WASM files;
explicit version/checksum-scoped registry source transforms omit one wit-parser
WASM fixture and sixteen rustix fallback archives. Every excluded member hash and
original URL/archive checksum remains in provenance, with full nonexcluded source,
unchanged lock, licenses, patch and exact rebuild instructions. Original archives
stay in private audit caches, outside package and workflow evidence uploads.
The two transformed registry bundles are named `-source.tar.gz`; all other registry
sources, including both MPL archives, remain pristine.

## Explicit retained native packaging

The verified package from run `38823056442` is an optional feasibility input.
`retained-native-package.json` binds its exact Darwin ARM64 archive, executable,
manifest, source/policy and seven-day GitHub artifact identity to the independent
download review. Existing `pins.json` and ordinary installed-binary selection are
unchanged. The admission tool performs no download, build or execution:

```sh
python3 -B tools/local-zellij-feasibility/accept-package.py --root /absolute/retained-artifact --target aarch64-apple-darwin
python3 -B scripts/build-macos-dev --architecture host --with-zellij --zellij-package-root /absolute/retained-artifact
```

Only a Darwin ARM64 host build has an accepted package. Intel/universal requests
fail before bundle replacement. The optional app retains the complete package,
including sources/notices/provenance, under `Resources/Dependencies/local-zellij`.
The builder verifies the dependency's existing signature and hashes every retained
member again after app signing; it never re-signs or rebuilds that dependency.
Ordinary app builds do not package or select Zellij.

The manual `local-zellij-native-package.yml` gate retrieves only the exact locked
artifact, checks its metadata/ZIP/package identity, builds the optional host app,
and reruns the actual production CLI owner against the final bundled dependency.
Its CLI is Cargo-built; the Zellij executable is the final app resource path.
Native workflow execution remains required for acceptance of this new consumer.
This slice adds no AppKit Start/Attach action, native persistence descriptor,
default terminal selection, Intel/universal or older-macOS support claim.

The GitHub input expires after seven days. Expired/missing exact retrieval fails
visibly, with no latest/refetch/rebuild fallback. Later default distribution needs
durable immutable owned storage and explicit promotion.
