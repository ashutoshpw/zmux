# Proposed headless zellij owner and macOS proof

2026-10-15. Design only; no production owner, transport or CLI source is implemented.
The [version-conformance record](r03-local-zellij-conformance.md) owns the actual
pinned Linux evidence. This proposal requires review and the native proof below.

## Recommendation

Build an independently authored headless owner around a private per-record backend
namespace/cache and a private per-attachment Unix socket broker. Use the installed,
explicitly supported zellij 0.43.0 executable; do not bundle/import upstream code or
assets. Keep the existing tmux identity/launch contract separate.

The broker gives the product both peer descriptors. It can verify the actual owned
CLI child and actual reserved server using public socket credential APIs, without
cross-process socket-table inspection or private Darwin kernel layouts. It forwards
ordered bytes and never redirects an established connection to a replacement server.
This is additional transport ownership, with real memory/process/shutdown costs.
Linux feasibility alone does not establish portable readiness.

The first delivered slice would cover detached start, typed registry listing/status,
interactive headless attach, explicit owner close, and stale-record cleanup preview/
prune. GUI creation/reuse/reopen/model migration and managed caller attribution follow
only after the headless contract passes on macOS. An interactive headless attach has
the child's terminal/exit contract; it must not emit an early JSON “attached” receipt.

## Upstream receipts and transport findings

The current official [CLI action documentation](https://zellij.dev/documentation/cli-actions#list-clients)
describes `list-clients` stdout as client ID, focused pane ID and running program.
That page is current documentation, not a frozen 0.43.0 specification. Pinned source
confirms the same fields in `SessionLayoutMetadata::list_clients_metadata`.

`Action::ListClients` routes through screen, plugin metadata and PTY metadata before
`ServerInstruction::Log` sends `ServerToClientMsg::Log` to the requesting CLI socket.
`cli_client::individual_messages_client` prints that Log to stdout. It does **not**
deliver the table into the user's terminal pane. The same receive loop also ends on
`UnblockInputThread`; the server broadcasts that message across clients. This is a
source-backed way for an unrelated unblock to race the asynchronous Log. Empty
stdout is not a successful client inventory. Neither the table nor `ClientAttributes`
contains an OS PID, owned-child nonce or per-PTY acknowledgement.

`ConnStatus`/`Connected` is an internal server-liveness exchange used by the public
listing CLI. It is not an acknowledgement of a particular terminal attachment.
The first slice should invoke the supported CLI through the fenced broker rather
than reproduce the versioned internal MessagePack schema. A sole-name, empty-cache
inspection namespace prevents an exited cache from turning a successful human
listing into a false live result. False negatives may be retried as inspection within
one deadline; creation is never replayed.

Pinned IPC transport inspection covered both sides and the locked dependency:

- `IpcSenderWithContext::send` writes MessagePack `(message, ErrorContext)` through
  `BufWriter<LocalSocketStream>`; the receiver decodes through `BufReader`.
- Client `connect_to_server` opens that stream and duplicates its local descriptor
  for reading/writing. Server `new_client` performs the corresponding local duplicate.
  A local `dup` is not descriptor transfer to the other process.
- `interprocess` 1.2.1 was downloaded only under the task's temporary directory and
  checked against the pinned Cargo checksum
  `81f2533f3be42fffe3b5e63b71aeca416c1c3bc33e4e27be018521e76b1f38fb`.
  Its invoked `Read::read`/`Write::write` paths use byte reads/writes. The dependency
  also offers ancillary APIs, but the inspected zellij IPC call paths do not invoke
  them or send descriptor-bearing messages.
- The inspected terminal IPC path does not call `peer_pid`, credential lookup or
  peer-PID authorization. The upstream server will see the broker as its socket
  peer; that does not change an observed authorization contract for this pin.
  Revalidate this on version upgrades and on the selected macOS release binary.
- Client `connect_to_server` retries failed connects forever. Product deadlines and
  physical child ownership are mandatory; an absent socket cannot be delegated to
  an unbounded direct exec.

There is no established supported CLI receipt tying an OS PTY child to an attachment.
The broker supplies an exact **transport peer binding**. A future GUI “attached”
receipt needs a separate semantic condition: for example, a bounded, reviewed
observer of an actual AttachClient/Render exchange, or an upstream supported receipt.
Byte counts, elapsed time, subprocess existence, a ConnStatus connection, or the
number of preflight connections are insufficient substitutes. That observer would
be a deliberate versioned protocol dependency and is outside this headless proposal.

## Identity, files and reservations

Use a distinct `LocalZellijIdentity`: logical UUID, full generated session name and
reserved server process identity. A platform process identity includes the PID,
process start identity and boot identity. Socket device/inode and namespace ownership
are additional fences, not a claimed zellij server-incarnation marker.

The profile registry stores the display name, logical identity, short namespace slot,
state and confirmed process binding. It never stores startup shell text, executable
paths, environment variables, app credentials or an arbitrary socket path. Derive
paths from validated installation configuration and a constrained random slot.
If later GUI state persists an identity, it must remain a pure descriptor.

Separate durable configuration from a short private runtime root. Zellij adds both
its version directory and full session name; nesting two full UUIDs under a long HOME
path is not portable. Allocate a user-owned mode-0700 short runtime profile directory
with an unpredictable installation token, then a constrained random per-record slot.
Validate each ancestor/entry, UID, mode, symlink policy and socket length before spawn.
Record and revalidate directory ownership/inode when cleaning up. Never glob-delete
another installation or user session. Missing runtime storage after reboot is stale
state, not an instruction to recreate the workload.

Every record gets its own backend socket base and cache. Every attach/helper gets
another empty private client namespace/cache containing only the exact broker socket
name. Unexpected sockets, serialized layouts or namespace substitutions fail closed.
Full UUID names remain necessary; upstream prefix matching is not an exact-ID fence.

Reserve the logical UUID/slot in a locked, bounded, mode-0600 atomic registry **before**
starting the owner. States distinguish reserved, live, uncertain, stale and closing.
After launcher success, capture the kernel peer PID and public process identity of
the sole new backend and confirm server liveness through an owned CLI helper. Commit
that binding before returning success. A crash, timeout or save failure after possible
creation retains an uncertain outcome and never retries startup. Do not silently
adopt an owner merely because a provisional name exists.

Use explicit false serialization settings on creation for this pinned version and
isolate cache regardless. Treat any existing resurrection layout as unsafe, including
while an owner looks live. Attach never uses create/background-create/force-run flags.
Version/config precedence controls from the conformance record must be retained.
The initial owned configuration/template policy must be explicit; preserving arbitrary
user theme/layout settings is a separate compatibility decision.

For an explicit startup command, create a bounded private transient KDL launch file
with an actual KDL writer, then remove it after the backend has parsed it. It is not
durable session metadata. Plain new-tab templates must not reference that command or
its removed file. Test a later ordinary tab as well as detach/reattach for startup-once
behavior; a root command pane must not accidentally become the default tab template.

## Broker and child lifecycle

Before launching the child, bind the exact broker socket in its private versioned
client directory. The broker may hold a bounded accepted connection until the parent
publishes the owned child's PID/start identity. It must forward no bytes before that
binding exists. Reject every other PID/UID before opening a backend connection.

For each accepted connection, connect to the exact reserved backend path, inspect
peer PID/UID on that connected descriptor, and compare its current process/start/boot
identity with the record. Once validated, retain that descriptor; replacement of the
pathname must never cause a retry/retarget of the established stream. On mismatch,
close both ends and fail. A preflight connection is subject to the same policy.

Use one bounded nonblocking relay owner per interactive child, with at most four
simultaneous connections and 64 KiB per direction per connection as an initial budget.
Stop reading when the opposite buffer is full. Preserve byte ordering, partial writes,
half-close delivery and backpressure. A reset/broken pipe from a terminating peer is
an explicit normal closure state; credential failures, unexpected I/O and deadline
errors retain context. The exact budget remains subject to the real workload tests.

The GUI's future `Session` resource or headless CLI supervisor owns the listener,
connections, buffers, child handle and physical relay worker. Track them until joined/
reaped. Cancellation of an attachment closes only its transport/client, preserving
the durable backend. Release pending-launch credit only after resources move to a
tracked live/retired owner. No detached per-connection threads or unbounded queues.

Headless attach inherits the user's actual terminal stdin/stdout and launches an
owned child in the appropriate foreground job-control context; no extra PTY proxy or
terminal byte parsing is necessary. The supervisor retains termios for restoration,
does not consume terminal input, and returns the child's exit outcome. Specify signal
handling so SIGINT, HUP, TERM, TSTP/CONT and resize reach the child without accidentally
ending the durable server. The parent retains the unreaped Child handle through all
cleanup; it must not signal a global PID after losing physical child ownership.

Owner close is separate from attachment cancellation. Run the exact `kill-session`
operation through a fenced helper/broker connection, then wait for the original server
identity to exit before deleting its isolated cache/registry record. Exit 0 from a
CLI helper alone is insufficient. `delete-session --force` performs upstream inventory
and cache work, so it is not the identity fence. If the verified server cannot finish
close within the deadline, retain a closing/uncertain record and report the failure.
Do not fall back to a PID-only signal on macOS or delete a live owner's socket as if
that terminated its process. Platform-specific stronger process handles can be a
separate reviewed capability; they are not required to justify unsafe fallback.

## Linux transport feasibility receipt

`/tmp/zmux-zj-0430-v7bhuqvn/broker-probe.py` exercised an original temporary broker
against the pinned real binary. It forwarded opaque bytes, validated both peer PIDs
and start identities on broker-owned descriptors, and accepted three child connections
for upstream preflight plus terminal attachment. Actual PTY input wrote the original
shell PID; client TERM left that shell alive with startup count one. A same-UID,
different-PID connection was rejected before forwarding. All relay workers ended;
normal reset/pipe closure was recorded explicitly and the final run had no unexpected
errors. `broker-results.json` retains the result.

An earlier probe's upstream inventory cleanup timed out; its exact remaining task
server was identified and terminated through a Linux pidfd. This is additional evidence
against relying on human listing/CLI exit status for physical cleanup. The successful
final probe uses bounded exact close plus identity-bound fixture cleanup; no user
sessions/configuration were changed. This temporary Linux transport is not production
code and does not establish macOS support, saturation behavior or GUI readiness.

## Isolated macOS proof plan

Use a separate, explicitly dispatched conformance workflow on an available macOS
runner. Download the matching official 0.43.0 no-web archive and published checksum,
verify both archive/binary hashes, record host architecture/macOS/SDK/source SHA, and
retain its upstream MIT notice. Use only short task-owned temporary directories and
processes. Do not launch, kill or modify the user's cmux/zmux app. A build for another
architecture is not runtime proof for that architecture.

First compile a small standalone probe against the runner's public SDK headers:
`<sys/socket.h>`, `<sys/un.h>`, `<unistd.h>`, and the public process inspection headers.
For broker-owned connected sockets, verify `getpeereid` and
`getsockopt(SOL_LOCAL, LOCAL_PEERPID)` return the known child/backend PIDs and UID.
`LOCAL_PEERPID` is declared in Apple's public Unix socket header; this is a candidate
supported API, not an assertion of tested runner behavior. Unsupported/missing PID
credentials must fail closed. Read process start/boot identity through public APIs
with SDK-declared types; do not copy kernel-private layouts or inspect kernel pointers.

Then run the real zellij/broker matrix:

1. Verify peer PID after daemonization, private modes, both backend/proxy socket lengths,
   and process identity across close/reuse. Reject a wrong same-UID child and a replaced
   backend; keep any unrelated fixture intact.
2. Prove live PTY input/output through the opaque broker, unchanged shell PID/count
   across two clients, config `quit` overridden to detach, resize reaching the real
   foreground shell, and no descriptor passing/peer-PID-sensitive upstream behavior.
3. Reproduce the independent real serialized-layout control, ordinary revival when
   guards are bypassed, cache refusal without creation, absent exact socket and prefix
   shadow refusal. A false config and false CLI flag must still suppress serialization.
4. Stall each direction/peer, force partial writes and disconnect during preflight/
   attachment. Verify fixed buffers/connection caps, prompt cancellation, fully joined
   workers, reaped child, no owner loss and no socket cleanup outside owned inodes.
5. Exercise foreground job control and termios restoration for normal detach, SIGINT,
   TERM/HUP, TSTP/CONT, child crash and deadline cleanup. Verify actual `stty size` after
   resize. No source-level signal forwarding check substitutes for a real terminal.
6. Explicit close must end the original server/foreground fixture shell. A stuck helper
   or server must retain a truthful failed/uncertain receipt. Collect process/socket
   state before and after; leave no task-owned daemon behind.

Retain bounded logs, structured receipts, executable/license hashes, process identities,
socket credentials and termios/size observations as artifacts. A host-native pass is
the gate for that host; additional macOS/architecture coverage remains named explicitly.
After this proof and design review, authorize the production headless implementation.
